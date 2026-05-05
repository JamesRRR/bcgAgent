use std::io::Write;

use tauri::State;
use uuid::Uuid;

use crate::audio as audio_mod;
use crate::error::{AppError, AppResult};

use super::AppState;

#[tauri::command]
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
    let (_file, path) = tmp.keep().map_err(|e| AppError::Audio(format!("temp keep: {e}")))?;
    let result = audio_mod::transcribe(&path, &lang_hint).await;
    // Best-effort cleanup.
    let _ = std::fs::remove_file(&path);
    result
}

#[tauri::command]
pub fn speak(state: State<'_, AppState>, text: String, lang: String) -> AppResult<String> {
    let handle = audio_mod::speak(&text, &lang)?;
    let id = Uuid::new_v4().to_string();
    state.tts.lock().insert(id.clone(), handle);
    Ok(id)
}

#[tauri::command]
pub fn speak_cancel(state: State<'_, AppState>, handle_id: String) -> AppResult<()> {
    let popped = state.tts.lock().remove(&handle_id);
    if let Some(h) = popped {
        h.cancel();
    }
    Ok(())
}
