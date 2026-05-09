//! Streaming PCM playback sink built on top of `cpal`.
//!
//! Used by `ElevenLabsProvider` to play raw 16-bit little-endian mono PCM
//! frames as they arrive from the network — no MP3 decode, no temp file,
//! no `afplay` process. The first audible sample fires roughly when the
//! first network chunk lands rather than when the entire response has been
//! buffered, which is the whole point of the rewrite.
//!
//! Lifecycle:
//!   1. `PcmSinkHandle::start(sample_rate)` opens the default output device
//!      and spawns a dedicated thread that owns the `cpal::Stream` (the
//!      stream type is `!Send` on macOS, see `audio/native_capture.rs`).
//!   2. The audio callback drains a shared ring buffer; underruns produce
//!      silence so the stream never auto-stops mid-utterance.
//!   3. Caller pushes samples via `push(&[i16])`.
//!   4. `drain_and_stop(timeout)` blocks until the ring is empty (or the
//!      timeout fires) and then drops the stream.
//!   5. `stop_now()` drops the stream immediately — used by cancel.
//!
//! Cancel safety: `stop_now()` is idempotent; calling either drain or stop
//! after the stream has already torn down is a no-op.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;

use crate::error::{AppError, AppResult};

/// Cross-thread handle to a running PCM output stream.
pub struct PcmSinkHandle {
    ring: Arc<Mutex<VecDeque<i16>>>,
    /// Sentinel sender; dropping it tells the audio thread to stop.
    stop_tx: Option<std::sync::mpsc::Sender<()>>,
    stopped: Arc<AtomicBool>,
}

impl PcmSinkHandle {
    /// Open the default output device and start a mono `i16` PCM stream.
    pub fn start(sample_rate: u32) -> AppResult<Self> {
        let ring: Arc<Mutex<VecDeque<i16>>> = Arc::new(Mutex::new(VecDeque::with_capacity(
            (sample_rate as usize).saturating_mul(2),
        )));
        let stopped = Arc::new(AtomicBool::new(false));

        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<AppResult<()>>();

        let ring_for_thread = ring.clone();
        let stopped_for_thread = stopped.clone();

        std::thread::Builder::new()
            .name("tts-pcm-sink".into())
            .spawn(move || {
                if let Err(e) = run_output(sample_rate, ring_for_thread, stop_rx, ready_tx) {
                    tracing::warn!("pcm sink thread: {e}");
                }
                stopped_for_thread.store(true, Ordering::SeqCst);
            })
            .map_err(|e| AppError::Audio(format!("spawn pcm sink thread: {e}")))?;

        // Block until the audio thread reports the stream is playing (or
        // bubble up a device-init failure). Without this, callers would race
        // the first `push()` against device setup.
        match ready_rx.recv_timeout(Duration::from_secs(2)) {
            Ok(Ok(())) => Ok(Self {
                ring,
                stop_tx: Some(stop_tx),
                stopped,
            }),
            Ok(Err(e)) => Err(e),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(AppError::Audio(
                "pcm sink stream did not start within 2s".into(),
            )),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(AppError::Audio(
                "pcm sink thread exited before becoming ready".into(),
            )),
        }
    }

    /// Append samples to the ring buffer. No-op once the stream has stopped.
    pub fn push(&self, samples: &[i16]) {
        if self.stopped.load(Ordering::SeqCst) {
            return;
        }
        let mut ring = self.ring.lock();
        ring.extend(samples.iter().copied());
    }

    /// Block until the ring buffer is empty (the audio thread has played
    /// every queued sample) or `timeout` elapses, then stop the stream.
    pub fn drain_and_stop(mut self, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.stopped.load(Ordering::SeqCst) {
                break;
            }
            let empty = self.ring.lock().is_empty();
            if empty {
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        self.stop_internal();
    }

    /// Stop the stream immediately, discarding any unplayed samples.
    pub fn stop_now(mut self) {
        self.stop_internal();
    }

    fn stop_internal(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            drop(tx); // closes the channel; audio thread breaks out
        }
    }
}

impl Drop for PcmSinkHandle {
    fn drop(&mut self) {
        self.stop_internal();
    }
}

/// Run the audio output thread. Owns the `cpal::Stream`. Sends a single
/// readiness signal to `ready_tx` once `stream.play()` returns, then waits
/// for the controller to drop `stop_tx` before tearing down.
fn run_output(
    sample_rate: u32,
    ring: Arc<Mutex<VecDeque<i16>>>,
    stop_rx: std::sync::mpsc::Receiver<()>,
    ready_tx: std::sync::mpsc::Sender<AppResult<()>>,
) -> Result<(), String> {
    macro_rules! ready_err {
        ($e:expr) => {{
            let err = AppError::Audio($e);
            let _ = ready_tx.send(Err(err));
            return Err(format!("device init"));
        }};
    }
    let host = cpal::default_host();
    let device = match host.default_output_device() {
        Some(d) => d,
        None => ready_err!("no default output device".into()),
    };
    let supported = match device.default_output_config() {
        Ok(s) => s,
        Err(e) => ready_err!(format!("default_output_config: {e}")),
    };
    let device_channels = supported.channels() as usize;
    let device_sr = sample_rate;

    let cfg = cpal::StreamConfig {
        channels: supported.channels(),
        sample_rate: cpal::SampleRate(device_sr),
        buffer_size: cpal::BufferSize::Default,
    };
    let sample_format = supported.sample_format();
    let err_fn = |e| tracing::error!("pcm sink stream error: {e}");

    let ring_for_cb = ring.clone();
    let stream = match sample_format {
        cpal::SampleFormat::F32 => device
            .build_output_stream(
                &cfg,
                move |out: &mut [f32], _: &_| {
                    let frames = out.len() / device_channels.max(1);
                    let mut ring = ring_for_cb.lock();
                    for f in 0..frames {
                        let s = ring
                            .pop_front()
                            .map(|i| (i as f32) / (i16::MAX as f32))
                            .unwrap_or(0.0);
                        for c in 0..device_channels {
                            out[f * device_channels + c] = s;
                        }
                    }
                },
                err_fn,
                None,
            )
            .map_err(|e| format!("build_output_stream f32: {e}"))?,
        cpal::SampleFormat::I16 => device
            .build_output_stream(
                &cfg,
                move |out: &mut [i16], _: &_| {
                    let frames = out.len() / device_channels.max(1);
                    let mut ring = ring_for_cb.lock();
                    for f in 0..frames {
                        let s = ring.pop_front().unwrap_or(0);
                        for c in 0..device_channels {
                            out[f * device_channels + c] = s;
                        }
                    }
                },
                err_fn,
                None,
            )
            .map_err(|e| format!("build_output_stream i16: {e}"))?,
        cpal::SampleFormat::U16 => device
            .build_output_stream(
                &cfg,
                move |out: &mut [u16], _: &_| {
                    let frames = out.len() / device_channels.max(1);
                    let mut ring = ring_for_cb.lock();
                    for f in 0..frames {
                        let s = ring.pop_front().unwrap_or(0);
                        let u = (s as i32 + 32768) as u16;
                        for c in 0..device_channels {
                            out[f * device_channels + c] = u;
                        }
                    }
                },
                err_fn,
                None,
            )
            .map_err(|e| format!("build_output_stream u16: {e}"))?,
        other => return Err(format!("unsupported output format {other:?}")),
    };

    if let Err(e) = stream.play() {
        let err = AppError::Audio(format!("output stream play: {e}"));
        let _ = ready_tx.send(Err(err));
        return Err("stream play".into());
    }
    let _ = ready_tx.send(Ok(()));

    // Wait for the sentinel. `recv()` returns once (either the controller
    // sent a `()` or dropped `stop_tx` causing a disconnect); both mean
    // "tear down".
    let _ = stop_rx.recv();

    drop(stream);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lifecycle test: start a sink, push a known number of samples, ensure
    /// `drain_and_stop` finishes within a sane window and no panic on Drop.
    /// Skipped on systems without a default output device (CI).
    #[test]
    fn lifecycle_drains_within_timeout() {
        let sr = 22_050;
        let handle = match PcmSinkHandle::start(sr) {
            Ok(h) => h,
            Err(_) => {
                eprintln!("no audio output device — skipping pcm_sink lifecycle test");
                return;
            }
        };
        // 0.1s of 440Hz sine.
        let n = sr as usize / 10;
        let samples: Vec<i16> = (0..n)
            .map(|i| {
                let v = (i as f32 / sr as f32 * 440.0 * std::f32::consts::TAU).sin() * 0.2;
                (v * (i16::MAX as f32)) as i16
            })
            .collect();
        handle.push(&samples);
        let started = Instant::now();
        // Generous timeout — allow 4× expected duration for CI jitter.
        handle.drain_and_stop(Duration::from_millis(800));
        assert!(
            started.elapsed() < Duration::from_millis(1500),
            "drain_and_stop took {:?}",
            started.elapsed()
        );
    }
}
