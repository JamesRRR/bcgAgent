use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::events::{emit as sink_emit, EventSink};
use crate::llm::minimax::Message;
use crate::llm::prompts::WALKTHROUGH_PROMPT_ZH;
use crate::llm::stream_chat;
use crate::store::{
    chunks as store_chunks, games as store_games, walkthroughs as store_walkthroughs,
};

use super::AppState;

/// Hard cap on how many chunks we send. A typical rulebook has 20-100
/// chunks; if the user imports something massive we want a fallback rather
/// than blowing past MiniMax's context window.
const MAX_CHUNKS: usize = 200;

#[derive(Debug, Clone, Serialize)]
struct WalkthroughDoneEvent {
    game_id: String,
}

pub async fn run_walkthrough(
    state: &AppState,
    sink: EventSink,
    game_id: String,
) -> AppResult<String> {
    let db = state.db.clone();
    let gid = game_id.clone();
    let game = tokio::task::spawn_blocking(move || store_games::get_game(&db, &gid))
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??
        .ok_or_else(|| AppError::Other(anyhow::anyhow!("game not found: {game_id}")))?;

    let db = state.db.clone();
    let gid = game_id.clone();
    let chunks = tokio::task::spawn_blocking(move || store_chunks::list_chunks_for_game(&db, &gid))
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;

    tracing::debug!(
        "walkthrough: game={:?} chunks={}",
        game.name_zh,
        chunks.len()
    );
    if chunks.is_empty() {
        return Err(AppError::Other(anyhow::anyhow!(
            "this game has no indexed pages yet — import some rulebook pages first"
        )));
    }

    let mut user = format!(
        "规则书：《{}》\n\n以下是规则书的全部内容：\n\n",
        game.name_zh
    );
    for (_, page_no, heading, content) in chunks.iter().take(MAX_CHUNKS) {
        user.push_str(&format!("[p.{}]", page_no));
        if let Some(h) = heading {
            if !h.is_empty() {
                user.push_str("  · ");
                user.push_str(h);
            }
        }
        user.push('\n');
        user.push_str(content);
        user.push_str("\n\n");
    }
    user.push_str("\n请基于以上内容，按系统提示规定的章节结构，写一份新手走查。");

    let messages = vec![
        Message {
            role: "system".into(),
            content: WALKTHROUGH_PROMPT_ZH.into(),
        },
        Message {
            role: "user".into(),
            content: user,
        },
    ];

    let sink_for_tokens = sink.clone();
    let answer = stream_chat(messages, move |tok| {
        sink_emit(&sink_for_tokens, "walkthrough:token", &tok.to_string());
    })
    .await?;

    // Persist the result so future page visits don't re-spend tokens.
    if !answer.trim().is_empty() {
        let db = state.db.clone();
        let gid = game_id.clone();
        let body = answer.clone();
        if let Err(e) =
            tokio::task::spawn_blocking(move || store_walkthroughs::upsert(&db, &gid, &body))
                .await
                .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))?
        {
            tracing::warn!("walkthrough cache upsert failed: {e}");
        }
    }

    sink_emit(
        &sink,
        "walkthrough:done",
        &WalkthroughDoneEvent {
            game_id: game_id.clone(),
        },
    );

    Ok(answer)
}

/// Read the cached walkthrough for a game without making any LLM call.
/// Returns `None` if not yet generated.
#[tauri::command(rename_all = "snake_case")]
pub async fn walkthrough_get_cached(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<Option<String>> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || store_walkthroughs::get(&db, &game_id))
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn walkthrough_run(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    game_id: String,
) -> AppResult<String> {
    tracing::debug!("walkthrough_run invoked for game_id={}", game_id);
    let app = app_handle.clone();
    let sink: EventSink = Arc::new(move |event: &str, payload: serde_json::Value| {
        if let Err(e) = app.emit(event, payload) {
            tracing::warn!("emit {event} failed: {e}");
        }
    });
    let result = run_walkthrough(&state, sink, game_id).await;
    if let Err(e) = &result {
        tracing::warn!("walkthrough_run failed: {e}");
    }
    result
}
