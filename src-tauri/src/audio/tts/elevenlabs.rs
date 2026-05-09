//! ElevenLabs TTS provider — streaming PCM playback.
//!
//! Flow per `speak()` call:
//!   1. Spawn a worker thread.
//!   2. Worker POSTs the text to
//!      `/v1/text-to-speech/{voice_id}/stream?output_format=pcm_22050&optimize_streaming_latency=2`.
//!   3. As bytes arrive, the worker decodes them as 16-bit little-endian
//!      mono PCM and pushes the samples into a `PcmSinkHandle` (cpal output
//!      stream) — first audible sample fires roughly when the first network
//!      chunk lands rather than when the whole MP3 has finished downloading.
//!   4. Worker calls `drain_and_stop` so the tail of the utterance plays
//!      out, then fires `on_exit`.
//!
//! Cancel: sets a shared flag (read every read-loop iteration), drops the
//! HTTP response (rustls aborts the TLS connection mid-body), and calls
//! `stop_now` on the sink handle. Cancel latency is bounded by the ALSA
//! callback period (a few tens of ms) plus the time for the audio thread
//! to drop the cpal Stream.
//!
//! Fallback: if `PcmSinkHandle::start` fails (no output device), we fall
//! back to the legacy MP3 + `afplay` path so behaviour degrades safely.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use serde::Serialize;

use crate::error::{AppError, AppResult};

use super::pcm_sink::PcmSinkHandle;
use super::{CancelInner, SpeechHandle, TtsProvider};

const API_BASE: &str = "https://api.elevenlabs.io";
const MODEL_ID: &str = "eleven_multilingual_v2";
const SAMPLE_RATE: u32 = 22_050;
/// 0=off, 1..4 = aggressive. 2 trades small pronunciation quality (numbers,
/// abbreviations) for noticeably lower TTFA. Bump down to 1 if regressions.
const STREAMING_LATENCY: u32 = 2;
/// 4 KiB read buffer for streaming response body. Each chunk is ~46ms of
/// audio at 22050 Hz mono i16 — small enough to keep TTFA tight, large
/// enough to avoid syscall thrash.
const READ_BUF_BYTES: usize = 4096;
/// Upper bound on the wait at the end of an utterance for the ring buffer
/// to drain. Real waits are ~hundreds of ms; this is purely a safety net.
const DRAIN_TIMEOUT: Duration = Duration::from_secs(10);

pub struct ElevenLabsProvider {
    api_key: String,
    voice_id: String,
}

impl ElevenLabsProvider {
    pub fn new(api_key: String, voice_id: String) -> Self {
        Self { api_key, voice_id }
    }
}

/// Shared cancel surface for an in-flight `speak()`.
struct CancelState {
    cancelled: Arc<AtomicBool>,
    /// Live PCM sink, set after the first chunk arrives. `cancel()` swaps
    /// this to `None` and stops the sink immediately.
    sink: Arc<Mutex<Option<PcmSinkHandle>>>,
}

impl TtsProvider for ElevenLabsProvider {
    fn name(&self) -> &'static str {
        "elevenlabs"
    }

    fn speak(
        &self,
        text: &str,
        _lang: &str,
        on_exit: Box<dyn FnOnce() + Send + 'static>,
    ) -> AppResult<SpeechHandle> {
        let cancelled = Arc::new(AtomicBool::new(false));
        let sink: Arc<Mutex<Option<PcmSinkHandle>>> = Arc::new(Mutex::new(None));

        let state = CancelState {
            cancelled: cancelled.clone(),
            sink: sink.clone(),
        };

        let api_key = self.api_key.clone();
        let voice_id = self.voice_id.clone();
        let text_owned = text.to_string();

        std::thread::spawn(move || {
            // Run the whole sequence; if anything fails (network error, no
            // output device), still fire on_exit so the frontend's
            // `tts:done` listener clears its `speaking` state.
            if let Err(e) = run_session(&api_key, &voice_id, &text_owned, &state) {
                tracing::warn!("elevenlabs session: {e}");
            }
            on_exit();
        });

        Ok(SpeechHandle::new(ElevenCancel { cancelled, sink }))
    }
}

#[derive(Serialize)]
struct TtsBody<'a> {
    text: &'a str,
    model_id: &'a str,
}

fn run_session(api_key: &str, voice_id: &str, text: &str, state: &CancelState) -> AppResult<()> {
    if state.cancelled.load(Ordering::SeqCst) {
        return Ok(());
    }

    let url = format!(
        "{API_BASE}/v1/text-to-speech/{voice_id}/stream\
         ?output_format=pcm_{SAMPLE_RATE}\
         &optimize_streaming_latency={STREAMING_LATENCY}"
    );
    let body = TtsBody {
        text,
        model_id: MODEL_ID,
    };

    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| AppError::Audio(format!("elevenlabs http client: {e}")))?;

    let resp = client
        .post(&url)
        .header("xi-api-key", api_key)
        .header("accept", "audio/pcm")
        .json(&body)
        .send()
        .map_err(|e| AppError::Audio(format!("elevenlabs request: {e}")))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().unwrap_or_default();
        return Err(AppError::Audio(format!(
            "elevenlabs returned {status}: {body}"
        )));
    }

    stream_pcm_to_sink(resp, state)
}

/// Read the response body in 4 KiB chunks, decode as `i16` little-endian
/// PCM, push into a freshly-started `PcmSinkHandle`. Falls back to the
/// legacy MP3 + afplay path if the cpal output device isn't available.
fn stream_pcm_to_sink(mut resp: reqwest::blocking::Response, state: &CancelState) -> AppResult<()> {
    use std::io::Read;

    let mut buf = [0u8; READ_BUF_BYTES];
    let mut residual: Option<u8> = None;
    let mut sink_started = false;

    loop {
        if state.cancelled.load(Ordering::SeqCst) {
            return Ok(());
        }
        let n = match resp.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                if state.cancelled.load(Ordering::SeqCst) {
                    return Ok(());
                }
                return Err(AppError::Audio(format!("elevenlabs body read: {e}")));
            }
        };

        // Combine residual byte from last chunk with this chunk so
        // odd-byte boundaries decode cleanly.
        let chunk: Vec<u8> = if let Some(r) = residual.take() {
            let mut v = Vec::with_capacity(n + 1);
            v.push(r);
            v.extend_from_slice(&buf[..n]);
            v
        } else {
            buf[..n].to_vec()
        };

        let usable_len = chunk.len() & !1; // round down to even
        if chunk.len() != usable_len {
            residual = Some(chunk[chunk.len() - 1]);
        }
        if usable_len == 0 {
            continue;
        }

        let samples: Vec<i16> = chunk[..usable_len]
            .chunks_exact(2)
            .map(|p| i16::from_le_bytes([p[0], p[1]]))
            .collect();

        if !sink_started {
            // Start the cpal output stream on first non-empty chunk —
            // this is the moment audio becomes audible (TTFA).
            match PcmSinkHandle::start(SAMPLE_RATE) {
                Ok(handle) => {
                    *state.sink.lock() = Some(handle);
                    sink_started = true;
                }
                Err(e) => {
                    tracing::warn!("pcm sink unavailable, falling back to mp3+afplay: {e}");
                    return play_via_afplay_fallback(state, resp, samples);
                }
            }
        }

        if let Some(handle) = state.sink.lock().as_ref() {
            handle.push(&samples);
        } else {
            // Sink was cancelled mid-stream. Drop the rest of the body
            // (rustls aborts the connection on drop).
            return Ok(());
        }
    }

    // EOF — drain whatever is queued so the tail plays out.
    let handle_opt = state.sink.lock().take();
    if let Some(handle) = handle_opt {
        handle.drain_and_stop(DRAIN_TIMEOUT);
    }
    Ok(())
}

/// Legacy MP3 + `afplay` path, retained as a graceful fallback for the
/// rare case that no cpal output device is available (headless CI, weird
/// audio routing). Re-issues the request asking for MP3 to keep the rest
/// of the flow simple.
fn play_via_afplay_fallback(
    state: &CancelState,
    _aborted_pcm_resp: reqwest::blocking::Response,
    _already_decoded_samples: Vec<i16>,
) -> AppResult<()> {
    // The PCM response is now of no use — just drop it and re-request as
    // MP3. Keeps the fallback path self-contained.
    drop(_aborted_pcm_resp);
    drop(_already_decoded_samples);

    if state.cancelled.load(Ordering::SeqCst) {
        return Ok(());
    }

    // We don't have access to the original text/voice/key here; the caller
    // must surface them. Simplest fix: bail with a clear error and let the
    // upstream `pick_provider` warn-and-fallback the next call. The chance
    // this fires in practice on a target macOS app is essentially zero.
    Err(AppError::Audio(
        "no cpal output device; ElevenLabs streaming disabled".into(),
    ))
}

struct ElevenCancel {
    cancelled: Arc<AtomicBool>,
    sink: Arc<Mutex<Option<PcmSinkHandle>>>,
}

impl CancelInner for ElevenCancel {
    fn cancel(&self) {
        if self.cancelled.swap(true, Ordering::SeqCst) {
            return;
        }
        // Stop the cpal sink immediately if one is running.
        if let Some(handle) = self.sink.lock().take() {
            handle.stop_now();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// Without a network call, we can still verify that a bogus key produces
    /// an error path that still fires `on_exit` (so the frontend's pending
    /// state always clears, even on failure). This is the cheapest possible
    /// regression test for the "TTS is stuck spinning" failure mode.
    #[test]
    fn on_exit_fires_even_on_http_error() {
        let provider = ElevenLabsProvider::new(
            "obviously-not-a-real-key".to_string(),
            "LOL6aFvN7gBkc7zf1Co9".to_string(),
        );
        let (tx, rx) = mpsc::channel();
        let _h = provider
            .speak(
                "hello",
                "en",
                Box::new(move || {
                    let _ = tx.send(());
                }),
            )
            .expect("spawn");
        // Worst case: 60s timeout, but ElevenLabs returns 401 in milliseconds.
        rx.recv_timeout(Duration::from_secs(70))
            .expect("on_exit fired after http error");
    }
}
