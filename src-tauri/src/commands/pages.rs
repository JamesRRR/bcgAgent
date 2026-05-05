use tauri::State;

use crate::error::{AppError, AppResult};
use crate::store::{pages, qa, Page, QAHistory};

use super::AppState;

#[tauri::command]
pub async fn pages_list_by_game(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<Vec<Page>> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || pages::list_pages_by_game(&db, &game_id))
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))?
}

#[tauri::command]
pub async fn page_get(state: State<'_, AppState>, id: String) -> AppResult<Option<Page>> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || pages::get_page(&db, &id))
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))?
}

#[tauri::command]
pub async fn qa_list(
    state: State<'_, AppState>,
    game_id: Option<String>,
    limit: i64,
) -> AppResult<Vec<QAHistory>> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || {
        qa::list_qa(&db, game_id.as_deref(), limit.max(0) as usize)
    })
    .await
    .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))?
}
