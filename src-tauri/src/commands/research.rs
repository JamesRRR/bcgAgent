//! User-triggered "rebuild knowledge" command for an existing game.
//!
//! The same pipeline runs automatically at import time. This command lets
//! the user re-pull BGG + re-caption illustrations after the fact, e.g.
//! after we've improved the prompt or BGG has new forum threads.

use tauri::{AppHandle, Emitter, State};

use crate::error::AppResult;
use crate::research::pipeline::{self, ResearchSummary};

use super::AppState;

#[tauri::command(rename_all = "snake_case")]
pub async fn research_run(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    game_id: String,
) -> AppResult<ResearchSummary> {
    let _ = app_handle.emit("research:started", serde_json::json!({"game_id": game_id}));
    let db = state.db.clone();
    let summary = pipeline::run_for_game(&db, &game_id).await?;
    let _ = app_handle.emit(
        "research:done",
        serde_json::json!({"game_id": game_id, "summary": &summary}),
    );
    Ok(summary)
}
