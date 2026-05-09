use tauri::State;

use crate::cover;
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

/// Run the auto-cover pipeline (BGG → first-page thumbnail). Idempotent:
/// no-op if the game already has a `cover_path`. Errors are logged, not surfaced.
#[tauri::command(rename_all = "snake_case")]
pub async fn game_auto_set_cover(state: State<'_, AppState>, game_id: String) -> AppResult<()> {
    let db = state.db.clone();
    cover::auto::auto_set_cover(&db, &game_id).await
}

/// Replace the cover with a user-chosen image file. Returns the new on-disk path.
#[tauri::command(rename_all = "snake_case")]
pub async fn game_set_cover_from_file(
    state: State<'_, AppState>,
    game_id: String,
    src_path: String,
) -> AppResult<String> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || cover::auto::set_cover_from_file(&db, &game_id, &src_path))
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
    tokio::task::spawn_blocking(move || games::update_name(&db, &id, &name_zh, name_en.as_deref()))
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))?
}

/// Permanently delete a game: DB rows (cascade) + on-disk
/// `games/<id>/` directory containing pages, thumbs, illustrations, cover.
/// Idempotent: deleting a non-existent game is a no-op.
#[tauri::command(rename_all = "snake_case")]
pub async fn game_delete(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let db = state.db.clone();
    let id_for_db = id.clone();
    tokio::task::spawn_blocking(move || games::delete_game(&db, &id_for_db))
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;
    // Best-effort wipe of on-disk game folder. Failure here is logged but
    // non-fatal — DB has already been cleaned.
    let dir = crate::paths::games_dir().join(&id);
    if dir.exists() {
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            tracing::warn!("game_delete: failed to remove {}: {e}", dir.display());
        }
    }
    Ok(())
}
