use std::io::Write;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use uuid::Uuid;

use crate::audio as audio_mod;
use crate::error::{AppError, AppResult};

use super::AppState;

#[tauri::command(rename_all = "snake_case")]
pub async fn transcribe(wav_bytes: Vec<u8>, lang_hint: String) -> AppResult<String> {
    if wav_bytes.is_empty() {
        return Err(AppError::Audio("empty wav payload".into()));
    }
    // Persist to a temp file the whisper-cli can open.
    let mut tmp = tempfile::Builder::new()
        .prefix("bcg-stt-")
        .suffix(".wav")
        .tempfile()?;
    tmp.write_all(&wav_bytes)?;
    tmp.flush()?;
    // Keep the path alive for the duration of the call.
    let (_file, path) = tmp
        .keep()
        .map_err(|e| AppError::Audio(format!("temp keep: {e}")))?;
    let result = audio_mod::transcribe(&path, &lang_hint).await;
    // Best-effort cleanup.
    let _ = std::fs::remove_file(&path);
    result
}

#[derive(Debug, Clone, Serialize)]
struct TtsDoneEvent {
    handle_id: String,
}

#[tauri::command(rename_all = "snake_case")]
pub fn speak(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    text: String,
    lang: String,
) -> AppResult<String> {
    let id = Uuid::new_v4().to_string();
    let id_for_cb = id.clone();

    // Capture clones the watcher thread will need to clean up state when
    // `say` exits (naturally or via cancel).
    let app = app_handle.clone();
    // We cannot move `State` into the callback (it's lifetime-bound); but
    // `AppState` lives in Tauri's managed map, accessible via `app.state()`.
    let on_exit = move || {
        if let Some(state) = app.try_state::<AppState>() {
            // Idempotent: removing twice is fine.
            let _ = state.tts.lock().remove(&id_for_cb);
        }
        if let Err(e) = app.emit(
            "tts:done",
            TtsDoneEvent {
                handle_id: id_for_cb.clone(),
            },
        ) {
            tracing::warn!("emit tts:done failed: {e}");
        }
    };

    let provider = audio_mod::tts::pick_provider(&state.db);
    let handle = provider.speak(&text, &lang, Box::new(on_exit))?;
    state.tts.lock().insert(id.clone(), handle);
    Ok(id)
}

#[tauri::command(rename_all = "snake_case")]
pub fn speak_cancel(state: State<'_, AppState>, handle_id: String) -> AppResult<()> {
    // Take it out of the map so future cancels for this id are no-ops.
    let popped = state.tts.lock().remove(&handle_id);
    if let Some(h) = popped {
        h.cancel();
    }
    Ok(())
}

// ---- Streaming push-to-talk transcription ----------------------------------
//
// Lifecycle:
//   transcribe_stream_start   → register session
//   transcribe_chunk × N      → append samples; emit `transcribe:partial`
//   transcribe_finalize       → final pass, return text, drop session
//   transcribe_stream_cancel  → drop without finalizing

#[derive(Debug, Clone, Serialize)]
struct PartialEvent {
    session_id: String,
    text: String,
    duration_ms: u64,
}

#[tauri::command(rename_all = "snake_case")]
pub fn transcribe_stream_start(
    state: State<'_, AppState>,
    session_id: String,
    lang_hint: String,
) -> AppResult<()> {
    state.stream.start(&session_id, &lang_hint);
    Ok(())
}

/// Append a 16kHz mono WAV chunk to the streaming session and run a partial
/// whisper pass on the cumulative buffer. Emits `transcribe:partial`.
/// Skips re-transcription if the buffer is shorter than 1 second to avoid
/// noise; whisper-cli wants at least ~1s for stable output.
#[tauri::command(rename_all = "snake_case")]
pub async fn transcribe_chunk(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    session_id: String,
    wav_bytes: Vec<u8>,
) -> AppResult<()> {
    if wav_bytes.is_empty() {
        return Ok(());
    }
    let samples = audio_mod::streaming::decode_wav_to_i16(&wav_bytes)?;
    let total = state.stream.append(&session_id, &samples)?;

    // Need at least ~1s of audio before bothering whisper.
    if total < 16_000 {
        return Ok(());
    }

    let snapshot = match state.stream.snapshot(&session_id) {
        Some(s) => s,
        None => return Ok(()),
    };
    let (samples, lang) = snapshot;

    let sid = session_id.clone();
    let app = app_handle.clone();
    tokio::task::spawn_blocking(move || -> AppResult<()> {
        let path = audio_mod::streaming::write_temp_wav(&samples)?;
        let started = std::time::Instant::now();
        let result = audio_mod::whisper::transcribe_blocking(&path, &lang);
        let _ = std::fs::remove_file(&path);
        match result {
            Ok(text) => {
                let evt = PartialEvent {
                    session_id: sid,
                    text,
                    duration_ms: started.elapsed().as_millis() as u64,
                };
                if let Err(e) = app.emit("transcribe:partial", evt) {
                    tracing::warn!("emit transcribe:partial failed: {e}");
                }
            }
            Err(e) => {
                tracing::debug!("partial transcribe skipped: {e}");
            }
        }
        Ok(())
    })
    .await
    .map_err(|e| AppError::Audio(format!("join error: {e}")))??;

    Ok(())
}

/// Final pass on accumulated samples. Returns the final transcript and
/// drops the session.
#[tauri::command(rename_all = "snake_case")]
pub async fn transcribe_finalize(
    state: State<'_, AppState>,
    session_id: String,
) -> AppResult<String> {
    let (samples, lang) = state
        .stream
        .finish(&session_id)
        .ok_or_else(|| AppError::Audio(format!("no streaming session {session_id}")))?;

    if samples.len() < 800 {
        // <50ms of audio — treat as cancelled.
        return Ok(String::new());
    }

    tokio::task::spawn_blocking(move || -> AppResult<String> {
        let path = audio_mod::streaming::write_temp_wav(&samples)?;
        let text = audio_mod::whisper::transcribe_blocking(&path, &lang);
        let _ = std::fs::remove_file(&path);
        text
    })
    .await
    .map_err(|e| AppError::Audio(format!("join error: {e}")))?
}

#[tauri::command(rename_all = "snake_case")]
pub fn transcribe_stream_cancel(state: State<'_, AppState>, session_id: String) -> AppResult<()> {
    let _ = state.stream.finish(&session_id);
    Ok(())
}

// ---- Native push-to-talk (cpal) -------------------------------------------
//
// Replaces the browser-side getUserMedia path because WKWebView on macOS
// silently denies media permissions. Recording is owned in Rust; the frontend
// just sends start/stop and listens to `transcribe:partial`.

#[tauri::command(rename_all = "snake_case")]
pub async fn mic_capture_start(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    session_id: String,
    lang_hint: String,
) -> AppResult<()> {
    // Make sure the whisper model is on disk before we start streaming —
    // otherwise the first partial pass (and the final pass) would error with
    // "whisper model not yet downloaded". Idempotent: no-op if already cached.
    audio_mod::whisper::ensure_model().await?;
    state
        .mic
        .start(session_id, lang_hint, state.stream.clone(), app_handle)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn mic_capture_stop(state: State<'_, AppState>, session_id: String) -> AppResult<String> {
    tracing::info!("mic_capture_stop sid={session_id}");
    // Halt the cpal thread (drops the Stream; pumps no more samples).
    let _ = state.mic.stop(&session_id);
    // Give the mic thread a moment to drop its cpal Stream and stop appending.
    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
    // Final whisper pass on the full accumulated buffer.
    let (samples, lang) = state
        .stream
        .finish(&session_id)
        .ok_or_else(|| AppError::Audio(format!("no streaming session {session_id}")))?;
    tracing::info!(
        "mic_capture_stop sid={session_id} samples={} lang={}",
        samples.len(),
        lang
    );
    if samples.len() < 800 {
        return Ok(String::new());
    }
    let result = tokio::task::spawn_blocking(move || -> AppResult<String> {
        let path = audio_mod::streaming::write_temp_wav(&samples)?;
        tracing::info!("mic_capture_stop wav written: {}", path.display());
        let text = audio_mod::whisper::transcribe_blocking(&path, &lang);
        let _ = std::fs::remove_file(&path);
        text
    })
    .await
    .map_err(|e| AppError::Audio(format!("join error: {e}")))?;
    match &result {
        Ok(t) => tracing::info!("mic_capture_stop transcript len={}", t.len()),
        Err(e) => tracing::error!("mic_capture_stop whisper failed: {e}"),
    }
    result
}

#[tauri::command(rename_all = "snake_case")]
pub fn mic_capture_cancel(state: State<'_, AppState>, session_id: String) -> AppResult<()> {
    let _ = state.mic.stop(&session_id);
    let _ = state.stream.finish(&session_id);
    Ok(())
}
