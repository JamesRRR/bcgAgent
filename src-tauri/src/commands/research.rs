//! User-triggered "rebuild knowledge" command for an existing game.
//!
//! The same pipeline runs automatically at import time. This command lets
//! the user re-pull BGG + re-caption illustrations after the fact, e.g.
//! after we've improved the prompt or BGG has new forum threads.

use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::extractors;
use crate::research::connectors::GameCtx;
use crate::research::orchestrator::{
    self, OrchestratorDeps, ResearchOutcome, ResearchPlan, DEFAULT_MAX_HITS_TO_FETCH,
};
use crate::research::pipeline::{self, ResearchSummary};
use crate::store::{chunks as store_chunks, games as store_games};

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

/// Wave 2 user-explicit research command. Bypasses the confidence check
/// (Wave 4 owns the auto-trigger path) and runs the orchestrator with the
/// `user_explicit` trigger and a 15-second deadline.
///
/// Frontend gets the `ResearchOutcome` (event id, chunks added, urls fetched)
/// so it can confirm to the user that something happened.
#[tauri::command(rename_all = "snake_case")]
pub async fn cmd_explicit_research(
    state: State<'_, AppState>,
    game_id: String,
    query: String,
) -> AppResult<ResearchOutcome> {
    let db = state.db.clone();

    // Load game context (name_zh / name_en / bgg_id). The publisher URL
    // isn't tracked in `games` today; pass `None` until we surface it.
    let game = {
        let db = db.clone();
        let gid = game_id.clone();
        tokio::task::spawn_blocking(move || store_games::get_game(&db, &gid))
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??
    }
    .ok_or_else(|| AppError::Other(anyhow::anyhow!("game not found: {game_id}")))?;
    let bgg_id = {
        let db = db.clone();
        let gid = game_id.clone();
        tokio::task::spawn_blocking(move || store_games::get_bgg_id(&db, &gid))
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??
    };

    let ctx = GameCtx {
        game_id: &game.id,
        bgg_id,
        name_zh: &game.name_zh,
        name_en: game.name_en.as_deref(),
        publisher_url: None,
    };
    let plan = ResearchPlan {
        trigger: "user_explicit",
        query,
        max_hits_to_fetch: DEFAULT_MAX_HITS_TO_FETCH,
    };
    let deadline = Instant::now() + Duration::from_secs(15);
    let deps = OrchestratorDeps::production(db.clone())?;
    orchestrator::run_research(&db, &ctx, plan, deadline, &deps).await
}

#[derive(Debug, Clone, Serialize)]
pub struct ExtractorRunSummary {
    pub components: extractors::ExtractSummary,
    pub faqs: extractors::ExtractSummary,
    pub setup: extractors::ExtractSummary,
}

/// Wave 3 explicit re-extraction. Re-runs all three structured extractors
/// for `game_id` and returns the per-extractor summary. Idempotent — each
/// extractor wipes its rows before reinserting.
#[tauri::command(rename_all = "snake_case")]
pub async fn cmd_run_extractors(
    state: State<'_, AppState>,
    game_id: String,
) -> AppResult<ExtractorRunSummary> {
    let db = state.db.clone();
    let (c, f, s) = tokio::join!(
        extractors::extract_components(&db, &game_id),
        extractors::extract_faqs(&db, &game_id),
        extractors::extract_setup(&db, &game_id),
    );
    Ok(ExtractorRunSummary {
        components: c?,
        faqs: f?,
        setup: s?,
    })
}

/// Wave 4 user feedback hook. Sets the `endorsed` column on a chunk to
/// thumbs-up (`true`) or thumbs-down (`false`). The retrieval scorer reads
/// this column at score time — there's no re-embedding to do.
#[tauri::command(rename_all = "snake_case")]
pub async fn cmd_endorse_chunk(
    state: State<'_, AppState>,
    chunk_id: i64,
    up: bool,
) -> AppResult<()> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || store_chunks::update_chunk_endorsed(&db, chunk_id, Some(up)))
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))?
}

#[derive(Debug, Clone, Serialize)]
pub struct KbSourceKindCount {
    pub source_kind: String,
    pub count: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct KbSnapshot {
    pub game_id: String,
    pub chunks_by_source_kind: Vec<KbSourceKindCount>,
    pub components: u64,
    pub faq_pairs: u64,
    pub setup_steps: u64,
    pub research_events: u64,
}

/// Wave 4/5 KB-diff harness command. Aggregates per-source-kind chunk counts
/// and the structured-table sizes for `game_id`. Pure read; cheap.
#[tauri::command(rename_all = "snake_case")]
pub async fn cmd_kb_diff(state: State<'_, AppState>, game_id: String) -> AppResult<KbSnapshot> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || -> AppResult<KbSnapshot> {
        let counts = store_chunks::count_chunks_by_source_kind(&db, &game_id)?;
        let chunks_by_source_kind: Vec<KbSourceKindCount> = counts
            .into_iter()
            .map(|(source_kind, count)| KbSourceKindCount { source_kind, count })
            .collect();

        let conn = db.lock();
        let components: u64 = conn
            .query_row(
                "SELECT COUNT(*) FROM components WHERE game_id = ?",
                rusqlite::params![&game_id],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            .max(0) as u64;
        let faq_pairs: u64 = conn
            .query_row(
                "SELECT COUNT(*) FROM faq_pairs WHERE game_id = ?",
                rusqlite::params![&game_id],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            .max(0) as u64;
        let setup_steps: u64 = conn
            .query_row(
                "SELECT COUNT(*) FROM setup_steps WHERE game_id = ?",
                rusqlite::params![&game_id],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            .max(0) as u64;
        let research_events: u64 = conn
            .query_row(
                "SELECT COUNT(*) FROM research_events WHERE game_id = ?",
                rusqlite::params![&game_id],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            .max(0) as u64;
        drop(conn);

        Ok(KbSnapshot {
            game_id,
            chunks_by_source_kind,
            components,
            faq_pairs,
            setup_steps,
            research_events,
        })
    })
    .await
    .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))?
}
