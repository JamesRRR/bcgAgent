//! Conversational walkthrough commands.
//!
//! `walkthrough_session_start` creates a session and generates the very first
//! agent turn (greeting + first instruction). `walkthrough_session_continue`
//! appends a user turn (confirm or question) and streams the next agent turn.
//! `walkthrough_session_get` loads the latest session for a game (resume).
//! `walkthrough_session_reset` deletes the session.

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::events::{emit as sink_emit, EventSink};
use crate::llm::minimax::Message;
use crate::llm::prompts::WALKTHROUGH_TURN_PROMPT_ZH;
use crate::llm::stream_chat;
use crate::store::{
    chunks as store_chunks, external_refs as store_refs, games as store_games,
    illustrations as store_ill, walkthrough_sessions as store_sessions,
};

use super::AppState;

const MAX_CHUNKS: usize = 200;

#[derive(Debug, Clone, Serialize)]
struct SessionTokenEvent {
    session_id: String,
    token: String,
}

#[derive(Debug, Clone, Serialize)]
struct SessionDoneEvent {
    session_id: String,
    turn_no: i64,
    phase: String,
    full_content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionView {
    pub session: store_sessions::Session,
    pub turns: Vec<store_sessions::Turn>,
}

/// Build the rulebook context block reused across every turn.
fn rulebook_context(state: &AppState, game_id: &str) -> AppResult<String> {
    let db = state.db.clone();
    let gid = game_id.to_string();
    let game = store_games::get_game(&db, &gid)?
        .ok_or_else(|| AppError::Other(anyhow::anyhow!("game not found: {gid}")))?;
    let chunks = store_chunks::list_chunks_for_game(&db, &gid)?;
    if chunks.is_empty() {
        return Err(AppError::Other(anyhow::anyhow!(
            "this game has no indexed pages yet — import some rulebook pages first"
        )));
    }

    let mut user = format!(
        "规则书：《{}》\n\n以下是规则书的全部内容，请基于此带玩家走查：\n\n",
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

    // Append per-illustration captions so the coach can answer "X 长什么样".
    let illustrations = store_ill::list_for_game(&db, &gid).unwrap_or_default();
    let illustrations_with_data: Vec<_> = illustrations
        .iter()
        .filter(|i| {
            i.label
                .as_deref()
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false)
                || i.description
                    .as_deref()
                    .map(|s| !s.trim().is_empty())
                    .unwrap_or(false)
        })
        .collect();
    if !illustrations_with_data.is_empty() {
        user.push_str("\n# 插图说明（来自规则书页面，每条对应一张图）\n");
        for ill in illustrations_with_data.iter().take(80) {
            let label = ill.label.as_deref().unwrap_or("").trim();
            let desc = ill.description.as_deref().unwrap_or("").trim();
            let token = ill.token.as_deref().unwrap_or("").trim();
            user.push_str("- ");
            if !token.is_empty() {
                user.push_str(&format!("[{}] ", token));
            }
            if !label.is_empty() {
                user.push_str(label);
                if !desc.is_empty() {
                    user.push_str(" — ");
                }
            }
            if !desc.is_empty() {
                user.push_str(desc);
            }
            user.push('\n');
        }
        user.push('\n');
    }

    // Append external knowledge (BGG description, top forum threads, gallery
    // captions) — pre-fetched at import time, capped to keep the prompt small.
    let refs = store_refs::list_for_game(&db, &gid).unwrap_or_default();
    if !refs.is_empty() {
        user.push_str("# 外部资料（已为你预先研究过，可作为补充依据）\n");
        for r in refs.iter().take(8) {
            let kind_label = match r.kind.as_str() {
                "description" => "BGG 简介",
                "forum" => "BGG 论坛讨论",
                "gallery" => "BGG 图库说明",
                other => other,
            };
            let title = r.title.as_deref().unwrap_or("");
            user.push_str(&format!("\n## {} — {}\n", kind_label, title));
            // Cap each ref body so the total prompt stays bounded.
            let body: String = r.content.chars().take(2000).collect();
            user.push_str(&body);
            user.push_str("\n");
        }
    }

    Ok(user)
}

/// Parse `<<PHASE:foo>>` from a streamed agent message; default "setup".
fn extract_phase(msg: &str) -> String {
    if let Some(start) = msg.find("<<PHASE:") {
        let rest = &msg[start + "<<PHASE:".len()..];
        if let Some(end) = rest.find(">>") {
            let raw = rest[..end].trim();
            if !raw.is_empty() {
                return raw.to_string();
            }
        }
    }
    "setup".into()
}

async fn run_agent_turn(
    state: &AppState,
    sink: EventSink,
    session_id: String,
    rulebook_user: String,
    history: Vec<store_sessions::Turn>,
) -> AppResult<(i64, String, String)> {
    // Build the conversation: system prompt + rulebook context as the
    // *first* user message, then alternate agent/user turns mapped to
    // assistant/user roles.
    let mut messages: Vec<Message> = Vec::with_capacity(history.len() + 2);
    messages.push(Message {
        role: "system".into(),
        content: WALKTHROUGH_TURN_PROMPT_ZH.into(),
    });
    messages.push(Message {
        role: "user".into(),
        content: rulebook_user,
    });
    for t in &history {
        let role = if t.role == "agent" {
            "assistant".to_string()
        } else {
            "user".to_string()
        };
        messages.push(Message {
            role,
            content: t.content.clone(),
        });
    }

    // Stream tokens to the frontend. Accumulate locally so we can persist
    // the full text and parse the phase at the end.
    let sid_for_tokens = session_id.clone();
    let sink_for_tokens = sink.clone();
    let answer = stream_chat(messages, move |tok| {
        sink_emit(
            &sink_for_tokens,
            "walkthrough_session:token",
            &SessionTokenEvent {
                session_id: sid_for_tokens.clone(),
                token: tok.to_string(),
            },
        );
    })
    .await?;

    let phase = extract_phase(&answer);

    // Persist the agent turn.
    let db = state.db.clone();
    let sid = session_id.clone();
    let body = answer.clone();
    let phase_for_persist = phase.clone();
    let turn_no = tokio::task::spawn_blocking(move || -> AppResult<i64> {
        let n = store_sessions::next_turn_no(&db, &sid)?;
        store_sessions::append_turn(&db, &sid, n, "agent", "instruction", &body)?;
        store_sessions::set_phase(&db, &sid, &phase_for_persist)?;
        Ok(n)
    })
    .await
    .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;

    sink_emit(
        &sink,
        "walkthrough_session:done",
        &SessionDoneEvent {
            session_id: session_id.clone(),
            turn_no,
            phase: phase.clone(),
            full_content: answer.clone(),
        },
    );

    Ok((turn_no, phase, answer))
}

fn make_sink(app_handle: &AppHandle) -> EventSink {
    let app = app_handle.clone();
    Arc::new(move |event: &str, payload: serde_json::Value| {
        if let Err(e) = app.emit(event, payload) {
            tracing::warn!("emit {event} failed: {e}");
        }
    })
}

#[tauri::command(rename_all = "snake_case")]
pub async fn walkthrough_session_start(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    game_id: String,
) -> AppResult<SessionView> {
    // If an active session exists, return it instead of starting a new one —
    // resume is the default. The frontend can call `_reset` first if it
    // wants to start fresh.
    let db = state.db.clone();
    let gid = game_id.clone();
    let existing = tokio::task::spawn_blocking(move || store_sessions::latest_for_game(&db, &gid))
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;

    let session = match existing {
        Some(s) => s,
        None => {
            let db = state.db.clone();
            let gid = game_id.clone();
            tokio::task::spawn_blocking(move || store_sessions::create(&db, &gid))
                .await
                .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??
        }
    };

    // If the session has no turns yet, fire the first agent turn now.
    let db = state.db.clone();
    let sid = session.session_id.clone();
    let turns = tokio::task::spawn_blocking(move || store_sessions::turns(&db, &sid))
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;

    if turns.is_empty() {
        let rulebook = rulebook_context(&state, &game_id)?;
        let sink = make_sink(&app_handle);
        let _ = run_agent_turn(&state, sink, session.session_id.clone(), rulebook, vec![]).await?;
    }

    let db = state.db.clone();
    let sid = session.session_id.clone();
    let turns = tokio::task::spawn_blocking(move || store_sessions::turns(&db, &sid))
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;

    // Re-read the session to pick up any phase update from the new turn.
    let db = state.db.clone();
    let gid = game_id.clone();
    let session = tokio::task::spawn_blocking(move || store_sessions::latest_for_game(&db, &gid))
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??
        .ok_or_else(|| AppError::Other(anyhow::anyhow!("session vanished after create")))?;

    Ok(SessionView { session, turns })
}

#[tauri::command(rename_all = "snake_case")]
pub async fn walkthrough_session_continue(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    session_id: String,
    user_kind: String, // "confirm" | "question"
    user_text: String, // the player's reply (for confirm: "好了" or similar)
) -> AppResult<()> {
    // Look up the game id so we can fetch chunks.
    let db = state.db.clone();
    let sid = session_id.clone();
    let game_id = tokio::task::spawn_blocking(move || -> AppResult<String> {
        let conn = db.lock();
        let gid: String = conn
            .query_row(
                "SELECT game_id FROM walkthrough_sessions WHERE session_id = ?",
                rusqlite::params![sid],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Other(anyhow::anyhow!("session lookup: {e}")))?;
        Ok(gid)
    })
    .await
    .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;

    // Persist the user turn first so it appears in history immediately.
    let db = state.db.clone();
    let sid = session_id.clone();
    let kind = user_kind.clone();
    let text = user_text.clone();
    tokio::task::spawn_blocking(move || -> AppResult<()> {
        let n = store_sessions::next_turn_no(&db, &sid)?;
        store_sessions::append_turn(&db, &sid, n, "user", &kind, &text)?;
        Ok(())
    })
    .await
    .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;

    let rulebook = rulebook_context(&state, &game_id)?;

    // Pull full history (now including the just-appended user turn).
    let db = state.db.clone();
    let sid = session_id.clone();
    let history = tokio::task::spawn_blocking(move || store_sessions::turns(&db, &sid))
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;

    let sink = make_sink(&app_handle);
    let _ = run_agent_turn(&state, sink, session_id, rulebook, history).await?;
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn walkthrough_session_get(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<Option<SessionView>> {
    let db = state.db.clone();
    let gid = game_id.clone();
    let session = tokio::task::spawn_blocking(move || store_sessions::latest_for_game(&db, &gid))
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;
    let Some(session) = session else {
        return Ok(None);
    };
    let db = state.db.clone();
    let sid = session.session_id.clone();
    let turns = tokio::task::spawn_blocking(move || store_sessions::turns(&db, &sid))
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;
    Ok(Some(SessionView { session, turns }))
}

#[tauri::command(rename_all = "snake_case")]
pub async fn walkthrough_session_reset(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<()> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || store_sessions::delete_for_game(&db, &game_id))
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_phase_from_well_formed_message() {
        let msg = "<<PHASE:first_round>>\n<<INSTRUCTION>>\n请抽 5 张牌。\n<<END>>";
        assert_eq!(extract_phase(msg), "first_round");
    }

    #[test]
    fn extract_phase_defaults_when_missing() {
        let msg = "free-form text without markers";
        assert_eq!(extract_phase(msg), "setup");
    }
}
