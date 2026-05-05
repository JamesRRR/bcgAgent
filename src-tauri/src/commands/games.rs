use tauri::State;

use crate::error::{AppError, AppResult};
use crate::store::{games, Game};

use super::AppState;

#[tauri::command(rename_all = "snake_case")]
pub async fn games_list(state: State<'_, AppState>) -> AppResult<Vec<Game>> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || games::list_games(&db))
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn game_create(
    state: State<'_, AppState>,
    name_zh: String,
    name_en: Option<String>,
    publisher: Option<String>,
) -> AppResult<String> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        games::insert_game(&db, &name_zh, name_en.as_deref(), publisher.as_deref())
    })
    .await
    .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn game_get(state: State<'_, AppState>, id: String) -> AppResult<Option<Game>> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || games::get_game(&db, &id))
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn game_set_cover(
    state: State<'_, AppState>,
    id: String,
    cover_path: String,
) -> AppResult<()> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || games::set_cover(&db, &id, &cover_path))
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn game_rename(
    state: State<'_, AppState>,
    id: String,
    name_zh: String,
    name_en: Option<String>,
) -> AppResult<()> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        games::update_name(&db, &id, &name_zh, name_en.as_deref())
    })
    .await
    .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))?
}
