use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::events::{emit as sink_emit, EventSink};
use crate::llm::{build_messages, rrf, stream_chat, RetrievedChunk};
use crate::store::{chunks as store_chunks, games as store_games, pages as store_pages, qa};

use super::AppState;

const FETCH_K: usize = 20;
const RRF_K: usize = 60;
const TOP_N: usize = 8;

/// Cite-friendly view of a retrieved chunk for the UI.
#[derive(Debug, Clone, Serialize)]
struct CitationChunk {
    chunk_id: i64,
    game_id: String,
    game_name: String,
    page_id: String,
    page_number: i64,
    heading_path: Option<String>,
    content: String,
    fused_score: f32,
}

#[derive(Debug, Clone, Serialize)]
struct AskDoneEvent {
    qa_id: String,
}

/// Transport-agnostic ask orchestration.
pub async fn run_ask(
    state: &AppState,
    sink: EventSink,
    question: String,
    game_id: Option<String>,
) -> AppResult<String> {
    let q_for_embed = question.clone();
    let qv = tokio::task::spawn_blocking(move || crate::embed::embed_query(&q_for_embed))
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;

    let db = state.db.clone();
    let gid_v = game_id.clone();
    let vec_hits = tokio::task::spawn_blocking(move || {
        store_chunks::vec_search(&db, &qv, gid_v.as_deref(), FETCH_K)
    })
    .await
    .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;

    let db = state.db.clone();
    let gid_f = game_id.clone();
    let q_for_fts = question.clone();
    let fts_hits = tokio::task::spawn_blocking(move || {
        store_chunks::fts_search(&db, &q_for_fts, gid_f.as_deref(), FETCH_K)
    })
    .await
    .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;

    let vec_ids: Vec<i64> = vec_hits.iter().map(|(id, _)| *id).collect();
    let fts_ids: Vec<i64> = fts_hits.iter().map(|(id, _)| *id).collect();
    let fused = rrf(&vec_ids, &fts_ids, RRF_K, TOP_N);

    let db = state.db.clone();
    let fused_clone = fused.clone();
    let (retrieved, citations): (Vec<RetrievedChunk>, Vec<CitationChunk>) =
        tokio::task::spawn_blocking(move || -> AppResult<_> {
            let mut retrieved = Vec::with_capacity(fused_clone.len());
            let mut citations = Vec::with_capacity(fused_clone.len());
            for (id, score) in &fused_clone {
                let chunk = match store_chunks::get_chunk(&db, *id)? {
                    Some(c) => c,
                    None => continue,
                };
                let page = match store_pages::get_page(&db, &chunk.page_id)? {
                    Some(p) => p,
                    None => continue,
                };
                let game = match store_games::get_game(&db, &chunk.game_id)? {
                    Some(g) => g,
                    None => continue,
                };
                retrieved.push(RetrievedChunk {
                    chunk_id: chunk.id,
                    game_name: game.name_zh.clone(),
                    page_number: page.page_number,
                    heading_path: chunk.heading_path.clone(),
                    content: chunk.content.clone(),
                    fused_score: *score,
                });
                citations.push(CitationChunk {
                    chunk_id: chunk.id,
                    game_id: chunk.game_id.clone(),
                    game_name: game.name_zh,
                    page_id: page.id,
                    page_number: page.page_number,
                    heading_path: chunk.heading_path,
                    content: chunk.content,
                    fused_score: *score,
                });
            }
            Ok((retrieved, citations))
        })
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;

    sink_emit(&sink, "ask:citations", &citations);

    let messages = build_messages(&question, &retrieved);
    let sink_for_tokens = sink.clone();
    let answer = stream_chat(messages, move |tok| {
        sink_emit(&sink_for_tokens, "ask:token", &tok.to_string());
    })
    .await?;

    let chunk_ids: Vec<i64> = retrieved.iter().map(|c| c.chunk_id).collect();
    let chunk_ids_json = serde_json::to_string(&chunk_ids).unwrap_or_else(|_| "[]".into());
    let db = state.db.clone();
    let q_for_save = question.clone();
    let answer_for_save = answer.clone();
    let game_id_for_save = game_id.clone();
    let qa_id = tokio::task::spawn_blocking(move || {
        qa::insert_qa(
            &db,
            game_id_for_save.as_deref(),
            &q_for_save,
            Some(&answer_for_save),
            None,
            Some(&chunk_ids_json),
        )
    })
    .await
    .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;

    sink_emit(
        &sink,
        "ask:done",
        &AskDoneEvent {
            qa_id: qa_id.clone(),
        },
    );

    Ok(qa_id)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn ask(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    question: String,
    game_id: Option<String>,
) -> AppResult<String> {
    let app = app_handle.clone();
    let sink: EventSink = Arc::new(move |event: &str, payload: serde_json::Value| {
        if let Err(e) = app.emit(event, payload) {
            tracing::warn!("emit {event} failed: {e}");
        }
    });
    run_ask(&state, sink, question, game_id).await
}
