//! Native microphone capture via cpal.
//!
//! Why this exists: WKWebView on macOS denies `getUserMedia()` silently by
//! default and the embedding app never appears in System Settings → Privacy
//! → Microphone. By calling `cpal` directly from the Rust binary, macOS
//! recognizes the app's main bundle as the mic-using process, prompts the
//! user via TCC the first time, and adds it to the Privacy list.
//!
//! Usage from a Tauri command:
//! - `start(session_id, app_handle)` opens the default input device and
//!   begins streaming. f32 samples are downsampled to 16kHz mono i16 and
//!   appended to a shared buffer. Every ~750ms a partial whisper pass runs
//!   on the cumulative buffer and emits `transcribe:partial`.
//! - `stop(session_id)` halts the cpal stream and returns the final
//!   transcript (caller is responsible for invoking whisper one last time).
//! - `cancel(session_id)` halts without finalizing.
//!
//! cpal Stream is `!Send` on macOS, so we own it on the audio callback
//! thread and use a oneshot channel to signal teardown.

use std::sync::Arc;
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use parking_lot::Mutex;
use tauri::{AppHandle, Emitter};

use crate::audio::streaming::{write_temp_wav, StreamRegistry};
use crate::error::{AppError, AppResult};

const TARGET_SR: u32 = 16_000;
const PARTIAL_INTERVAL: Duration = Duration::from_millis(900);

#[derive(Clone)]
pub struct NativeCaptureRegistry {
    inner: Arc<Mutex<std::collections::HashMap<String, Handle>>>,
}

struct Handle {
    /// Sentinel sent on drop to wake the audio thread for clean teardown.
    _stop_tx: std::sync::mpsc::Sender<()>,
}

impl Default for NativeCaptureRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl NativeCaptureRegistry {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }
    }

    pub fn is_active(&self, session_id: &str) -> bool {
        self.inner.lock().contains_key(session_id)
    }

    pub fn start(
        &self,
        session_id: String,
        lang_hint: String,
        stream_registry: StreamRegistry,
        app: AppHandle,
    ) -> AppResult<()> {
        if self.inner.lock().contains_key(&session_id) {
            return Err(AppError::Audio(format!(
                "session {session_id} already capturing"
            )));
        }

        // Stage the streaming session up front.
        stream_registry.start(&session_id, &lang_hint);

        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
        let sid_for_thread = session_id.clone();
        let app_for_thread = app.clone();

        // The cpal Stream type is !Send on macOS — we have to own it on a
        // single thread. Spawn an OS thread that owns the stream and pumps
        // partials until told to stop.
        std::thread::Builder::new()
            .name(format!("mic-capture-{session_id}"))
            .spawn(move || {
                if let Err(e) =
                    run_capture(sid_for_thread, stream_registry, app_for_thread, stop_rx)
                {
                    tracing::error!("mic capture thread: {e}");
                }
            })
            .map_err(|e| AppError::Audio(format!("spawn capture thread: {e}")))?;

        self.inner
            .lock()
            .insert(session_id, Handle { _stop_tx: stop_tx });
        Ok(())
    }

    /// Stop a running session — sends the sentinel to the audio thread,
    /// which exits its run loop and drops the cpal Stream cleanly.
    pub fn stop(&self, session_id: &str) -> AppResult<()> {
        let popped = self.inner.lock().remove(session_id);
        match popped {
            Some(_) => Ok(()),
            None => Err(AppError::Audio(format!(
                "no native capture session {session_id}"
            ))),
        }
    }
}

#[derive(serde::Serialize, Clone)]
struct PartialEvt {
    session_id: String,
    text: String,
    duration_ms: u64,
}

fn run_capture(
    session_id: String,
    stream_registry: StreamRegistry,
    app: AppHandle,
    stop_rx: std::sync::mpsc::Receiver<()>,
) -> AppResult<()> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| AppError::Audio("no default input device".into()))?;
    tracing::info!(
        "mic capture device: {}",
        device.name().unwrap_or_else(|_| "<unnamed>".into())
    );
    let supported = device
        .default_input_config()
        .map_err(|e| AppError::Audio(format!("default_input_config: {e}")))?;
    let device_sr = supported.sample_rate().0;
    let channels = supported.channels() as usize;
    let sample_format = supported.sample_format();
    tracing::info!(
        "mic capture config: sr={device_sr} channels={channels} format={sample_format:?}"
    );

    // Per-stream resampler state: keep a fractional position so chained
    // callbacks don't introduce gaps. Simple linear resampler — Whisper is
    // very forgiving about input quality, and this stays allocation-free.
    let resample_state = Arc::new(Mutex::new(ResampleState::new(device_sr, TARGET_SR)));
    let stream_registry_for_cb = stream_registry.clone();
    let sid_for_cb = session_id.clone();

    let err_fn = |e| tracing::error!("cpal stream error: {e}");

    let stream = match sample_format {
        cpal::SampleFormat::F32 => device
            .build_input_stream(
                &supported.into(),
                {
                    let resample_state = resample_state.clone();
                    move |data: &[f32], _: &_| {
                        let mut rs = resample_state.lock();
                        let mono = downmix_f32(data, channels);
                        let i16s = rs.resample_f32(&mono);
                        let _ = stream_registry_for_cb.append(&sid_for_cb, &i16s);
                    }
                },
                err_fn,
                None,
            )
            .map_err(|e| AppError::Audio(format!("build_input_stream f32: {e}")))?,
        cpal::SampleFormat::I16 => device
            .build_input_stream(
                &supported.into(),
                {
                    let resample_state = resample_state.clone();
                    move |data: &[i16], _: &_| {
                        let mut rs = resample_state.lock();
                        let mono: Vec<f32> = downmix_i16(data, channels)
                            .into_iter()
                            .map(|s| (s as f32) / (i16::MAX as f32))
                            .collect();
                        let i16s = rs.resample_f32(&mono);
                        let _ = stream_registry_for_cb.append(&sid_for_cb, &i16s);
                    }
                },
                err_fn,
                None,
            )
            .map_err(|e| AppError::Audio(format!("build_input_stream i16: {e}")))?,
        cpal::SampleFormat::U16 => device
            .build_input_stream(
                &supported.into(),
                {
                    let resample_state = resample_state.clone();
                    move |data: &[u16], _: &_| {
                        let mut rs = resample_state.lock();
                        let mono: Vec<f32> = downmix_u16(data, channels)
                            .into_iter()
                            .map(|s| ((s as f32) - 32768.0) / 32768.0)
                            .collect();
                        let i16s = rs.resample_f32(&mono);
                        let _ = stream_registry_for_cb.append(&sid_for_cb, &i16s);
                    }
                },
                err_fn,
                None,
            )
            .map_err(|e| AppError::Audio(format!("build_input_stream u16: {e}")))?,
        other => {
            return Err(AppError::Audio(format!(
                "unsupported sample format {other:?}"
            )))
        }
    };

    stream
        .play()
        .map_err(|e| AppError::Audio(format!("stream play: {e}")))?;

    // Pump loop: every PARTIAL_INTERVAL run a whisper pass and emit a partial.
    let mut last_partial = Instant::now();
    let mut last_pass_samples = 0usize;
    loop {
        match stop_rx.recv_timeout(PARTIAL_INTERVAL) {
            Ok(()) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                if last_partial.elapsed() < PARTIAL_INTERVAL {
                    continue;
                }
                let snapshot = stream_registry.snapshot(&session_id);
                if let Some((samples, lang)) = snapshot {
                    if samples.len() <= last_pass_samples {
                        continue;
                    }
                    if samples.len() < (TARGET_SR as usize) {
                        // <1s — don't bother whisper yet.
                        continue;
                    }
                    last_pass_samples = samples.len();
                    let started = Instant::now();
                    if let Ok(path) = write_temp_wav(&samples) {
                        let result = crate::audio::whisper::transcribe_blocking(&path, &lang);
                        let _ = std::fs::remove_file(&path);
                        if let Ok(text) = result {
                            let _ = app.emit(
                                "transcribe:partial",
                                PartialEvt {
                                    session_id: session_id.clone(),
                                    text,
                                    duration_ms: started.elapsed().as_millis() as u64,
                                },
                            );
                        }
                    }
                    last_partial = Instant::now();
                }
            }
        }
    }

    // stream is dropped here — cpal calls pause and tears down the OS stream.
    drop(stream);
    Ok(())
}

fn downmix_f32(data: &[f32], channels: usize) -> Vec<f32> {
    if channels <= 1 {
        return data.to_vec();
    }
    let frames = data.len() / channels;
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut sum = 0.0f32;
        for c in 0..channels {
            sum += data[f * channels + c];
        }
        out.push(sum / channels as f32);
    }
    out
}

fn downmix_i16(data: &[i16], channels: usize) -> Vec<i16> {
    if channels <= 1 {
        return data.to_vec();
    }
    let frames = data.len() / channels;
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut sum = 0i32;
        for c in 0..channels {
            sum += data[f * channels + c] as i32;
        }
        out.push((sum / channels as i32) as i16);
    }
    out
}

fn downmix_u16(data: &[u16], channels: usize) -> Vec<u16> {
    if channels <= 1 {
        return data.to_vec();
    }
    let frames = data.len() / channels;
    let mut out = Vec::with_capacity(frames);
    for f in 0..frames {
        let mut sum = 0u32;
        for c in 0..channels {
            sum += data[f * channels + c] as u32;
        }
        out.push((sum / channels as u32) as u16);
    }
    out
}

/// Naive linear resampler. Tracks a fractional read position across calls so
/// successive callbacks don't drop or duplicate samples at boundaries.
struct ResampleState {
    src_sr: u32,
    dst_sr: u32,
    pos: f32,
    last_input_sample: f32,
}

impl ResampleState {
    fn new(src_sr: u32, dst_sr: u32) -> Self {
        Self {
            src_sr,
            dst_sr,
            pos: 0.0,
            last_input_sample: 0.0,
        }
    }

    fn resample_f32(&mut self, input: &[f32]) -> Vec<i16> {
        if input.is_empty() {
            return Vec::new();
        }
        if self.src_sr == self.dst_sr {
            return input.iter().map(f32_to_i16).collect();
        }
        let ratio = self.src_sr as f32 / self.dst_sr as f32;
        let mut out: Vec<i16> =
            Vec::with_capacity(input.len() * self.dst_sr as usize / self.src_sr as usize + 4);
        // pos is the fractional index into a stream that conceptually starts
        // with `last_input_sample` then `input[0..]`.
        loop {
            let p = self.pos;
            let lo = p.floor() as i64;
            let hi = lo + 1;
            // Stop when hi is past the last input sample.
            if hi >= input.len() as i64 {
                break;
            }
            let a = if lo < 0 {
                self.last_input_sample
            } else {
                input[lo as usize]
            };
            let b = input[hi as usize];
            let frac = p - lo as f32;
            let s = a + (b - a) * frac;
            out.push(f32_to_i16(&s));
            self.pos += ratio;
        }
        // Slide pos so that pos=0 corresponds to "right after the last sample
        // we already consumed". Caller will pass the next input chunk; we
        // remember the last sample as the boundary anchor.
        if !input.is_empty() {
            self.last_input_sample = *input.last().unwrap();
        }
        let consumed = self.pos.floor() as i64;
        let max_consume = input.len() as i64;
        let consumed = consumed.min(max_consume);
        self.pos -= consumed as f32;
        // Reset pos to keep it bounded; if input was fully consumed, position
        // restarts at the same fractional offset relative to the next chunk.
        out
    }
}

fn f32_to_i16(s: &f32) -> i16 {
    let clamped = s.clamp(-1.0, 1.0);
    (clamped * (i16::MAX as f32)) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downmix_stereo_to_mono() {
        let stereo = vec![1.0_f32, -1.0, 0.5, -0.5, 0.0, 0.0];
        let m = downmix_f32(&stereo, 2);
        assert_eq!(m, vec![0.0, 0.0, 0.0]);
    }

    #[test]
    fn resample_44100_to_16000() {
        let mut rs = ResampleState::new(44100, 16000);
        // 1s of 440Hz sine
        let n = 44100;
        let input: Vec<f32> = (0..n)
            .map(|i| (i as f32 / 44100.0 * 440.0 * std::f32::consts::TAU).sin() * 0.5)
            .collect();
        let out = rs.resample_f32(&input);
        // ~16000 ± a couple of samples for boundary effects.
        assert!(
            (out.len() as i64 - 16000).abs() < 50,
            "got {} samples",
            out.len()
        );
    }
}
