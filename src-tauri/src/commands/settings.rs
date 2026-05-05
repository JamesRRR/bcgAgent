use tauri::State;

use crate::error::{AppError, AppResult};
use crate::secrets;
use crate::store::settings as store_settings;

use super::AppState;

#[tauri::command(rename_all = "snake_case")]
pub fn settings_get_secret(name: String) -> AppResult<Option<String>> {
    secrets::get_secret(&name)
}

#[tauri::command(rename_all = "snake_case")]
pub fn settings_set_secret(name: String, value: String) -> AppResult<()> {
    secrets::set_secret(&name, &value)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn settings_get(state: State<'_, AppState>, key: String) -> AppResult<Option<String>> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || store_settings::get(&db, &key))
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn settings_set(
    state: State<'_, AppState>,
    key: String,
    value: String,
) -> AppResult<()> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || store_settings::set(&db, &key, &value))
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))?
}
