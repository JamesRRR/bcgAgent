use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::events::{emit as sink_emit, EventSink};
use crate::llm::{
    build_messages, compute_confidence, endorsement_boost, rrf, stream_chat, RetrievedChunk,
};
use crate::store::{
    chunks as store_chunks, games as store_games, pages as store_pages, qa,
    settings as store_settings, Db,
};

use super::AppState;

const FETCH_K: usize = 20;
const RRF_K: usize = 60;
const TOP_N: usize = 8;

/// Default confidence threshold below which we kick off a research pass.
/// User-tunable via `settings.kb.confidence_threshold` (Wave 4 setting).
pub const CONF_THRESHOLD: f32 = 0.45;

/// Hard cap on auto-research wall-clock so the ask command always returns
/// something in bounded time.
const AUTO_RESEARCH_BUDGET: Duration = Duration::from_secs(8);

/// Cite-friendly view of a retrieved chunk for the UI. Wave 4 extends this
/// with provenance fields so the frontend can render trust badges.
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
    // Wave 4 provenance fields. These mirror the columns added in Wave 1.
    source_kind: String,
    source_url: Option<String>,
    trust_tier: String,
    official: bool,
    endorsed: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
struct AskDoneEvent {
    qa_id: String,
}

/// Output of one retrieve+score pass. Returned by `retrieve_with_scores` so
/// the caller can compute confidence + render citations from the same data.
struct RetrievalPass {
    retrieved: Vec<RetrievedChunk>,
    citations: Vec<CitationChunk>,
    /// Best (largest) cosine similarity across the top-K hits. Cosine is
    /// derived from the `chunks_vec` L2 distance: cos = 1 - dist/2 for
    /// unit-normalized vectors. We clamp to [0,1].
    top_cosine: f32,
    /// Best FTS rank, normalized to `[0,1]` where `1` = best. We do this by
    /// taking `1.0 / (1 + best_index)` over the raw FTS hit list — the very
    /// first FTS hit gives `1.0`, the second `0.5`, etc.
    fts_rank_norm: f32,
    /// Trust tier of the highest-scored hit, used for the tier weight in
    /// the confidence formula. Empty string when there are no hits.
    top_trust_tier: String,
}

/// Single retrieval pass. Pulled out of `run_ask` so the auto-research path
/// can call it twice (before & after research). Takes `&Db` directly so it
/// can run inside `spawn_blocking` without holding a Tauri `State`.
fn retrieve_with_scores(
    db: &Db,
    qv: &[f32],
    question: &str,
    game_id: Option<&str>,
    include_unofficial: bool,
) -> AppResult<RetrievalPass> {
    let qv_owned = qv.to_vec();
    let gid_owned = game_id.map(|s| s.to_string());
    let q_owned = question.to_string();

    // Vector + FTS searches. Both run on the blocking pool because rusqlite
    // is sync. We do them sequentially here (was already serialized via
    // spawn_blocking before) — keeping behavior identical to the original
    // `cmd_ask`.
    let vec_hits = store_chunks::vec_search(db, &qv_owned, gid_owned.as_deref(), FETCH_K)?;
    let fts_hits = store_chunks::fts_search(db, &q_owned, gid_owned.as_deref(), FETCH_K)?;

    // Wave 4: optionally drop non-official chunks before fusion.
    let (vec_hits, fts_hits): (Vec<(i64, f32)>, Vec<(i64, f32)>) = if include_unofficial {
        (vec_hits, fts_hits)
    } else {
        let mut v = Vec::with_capacity(vec_hits.len());
        for (id, s) in vec_hits {
            if store_chunks::is_chunk_official(db, id)? {
                v.push((id, s));
            }
        }
        let mut f = Vec::with_capacity(fts_hits.len());
        for (id, s) in fts_hits {
            if store_chunks::is_chunk_official(db, id)? {
                f.push((id, s));
            }
        }
        (v, f)
    };

    // Best raw cosine from the top vector hit. `vec_search` returns L2
    // distance for unit-normalized embeddings, where dist ∈ [0, 2]. So
    // cos = 1 - dist/2. We use the FIRST entry (lowest distance).
    let top_cosine = vec_hits
        .first()
        .map(|(_, dist)| (1.0_f32 - (*dist) / 2.0).clamp(0.0, 1.0))
        .unwrap_or(0.0);

    // Best FTS-rank → 1/(1+i). i=0 is the top hit.
    let fts_rank_norm = if fts_hits.is_empty() {
        0.0
    } else {
        1.0_f32 / (1.0 + 0.0)
    };

    let vec_ids: Vec<i64> = vec_hits.iter().map(|(id, _)| *id).collect();
    let fts_ids: Vec<i64> = fts_hits.iter().map(|(id, _)| *id).collect();

    // Apply the endorsement bias before fusing — we re-rank by adjusting
    // each list's per-id score with `endorsement_boost`, then resort.
    // This keeps RRF input as ordered ids (its expected shape) while still
    // honoring user thumbs.
    let endorse_lookup = |id: i64| -> AppResult<Option<bool>> {
        let prov = store_chunks::get_chunk_provenance(db, id)?;
        Ok(prov.and_then(|p| p.endorsed))
    };
    let mut vec_with_boost: Vec<(i64, f32)> = Vec::with_capacity(vec_hits.len());
    for (id, dist) in &vec_hits {
        // Higher score = better. Use 1/(1+dist) then add endorsement_boost.
        let base = 1.0_f32 / (1.0 + dist.max(0.0));
        let boost = endorsement_boost(endorse_lookup(*id)?);
        vec_with_boost.push((*id, base + boost));
    }
    vec_with_boost.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let vec_ids_boosted: Vec<i64> = vec_with_boost.iter().map(|(id, _)| *id).collect();

    let mut fts_with_boost: Vec<(i64, f32)> = Vec::with_capacity(fts_hits.len());
    for (id, bm) in &fts_hits {
        // bm25 is "smaller is better" → use -bm25, then add boost.
        let base = -*bm;
        let boost = endorsement_boost(endorse_lookup(*id)?);
        fts_with_boost.push((*id, base + boost));
    }
    fts_with_boost.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let fts_ids_boosted: Vec<i64> = fts_with_boost.iter().map(|(id, _)| *id).collect();

    // Fall back to the original (un-boosted) order when nothing was endorsed
    // — the ids list is identical, just maybe re-permuted.
    let _ = (vec_ids, fts_ids);
    let fused = rrf(&vec_ids_boosted, &fts_ids_boosted, RRF_K, TOP_N);

    // Hydrate fused ids into rich rows.
    let mut retrieved = Vec::with_capacity(fused.len());
    let mut citations = Vec::with_capacity(fused.len());
    let mut top_trust_tier = String::new();
    for (id, score) in &fused {
        let chunk = match store_chunks::get_chunk(db, *id)? {
            Some(c) => c,
            None => continue,
        };
        let page = match store_pages::get_page(db, &chunk.page_id)? {
            Some(p) => p,
            None => continue,
        };
        let game = match store_games::get_game(db, &chunk.game_id)? {
            Some(g) => g,
            None => continue,
        };
        let prov = store_chunks::get_chunk_provenance(db, *id)?;
        let (source_kind, source_url, trust_tier, official, endorsed) = match prov {
            Some(p) => (p.source_kind, p.source_url, p.trust_tier, p.official, p.endorsed),
            None => ("photo_ocr".to_string(), None, "publisher".to_string(), true, None),
        };
        if top_trust_tier.is_empty() {
            top_trust_tier = trust_tier.clone();
        }
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
            source_kind,
            source_url,
            trust_tier,
            official,
            endorsed,
        });
    }

    Ok(RetrievalPass {
        retrieved,
        citations,
        top_cosine,
        fts_rank_norm,
        top_trust_tier,
    })
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

    // Read user-controllable Wave 4 settings up front.
    let db = state.db.clone();
    let settings_snapshot = tokio::task::spawn_blocking(move || -> AppResult<_> {
        let auto = store_settings::get_bool(&db, store_settings::KB_AUTO_RESEARCH_ENABLED, true);
        let include_unofficial =
            store_settings::get_bool(&db, store_settings::KB_INCLUDE_UNOFFICIAL, true);
        let threshold =
            store_settings::get_f32(&db, store_settings::KB_CONFIDENCE_THRESHOLD, CONF_THRESHOLD);
        Ok((auto, include_unofficial, threshold))
    })
    .await
    .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;
    let (auto_research_enabled, include_unofficial, conf_threshold) = settings_snapshot;

    // First retrieval pass.
    let db = state.db.clone();
    let qv_clone = qv.clone();
    let gid_clone = game_id.clone();
    let q_clone = question.clone();
    let mut pass = tokio::task::spawn_blocking(move || {
        retrieve_with_scores(
            &db,
            &qv_clone,
            &q_clone,
            gid_clone.as_deref(),
            include_unofficial,
        )
    })
    .await
    .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;

    // Confidence + auto-research branch. Skipped when game_id is None: the
    // research connectors all need a concrete game ctx.
    let mut research_appendix: Option<&'static str> = None;
    let conf = compute_confidence(pass.top_cosine, pass.fts_rank_norm, &pass.top_trust_tier);
    if auto_research_enabled
        && conf < conf_threshold
        && game_id.is_some()
        && !pass.retrieved.is_empty()
    {
        sink_emit(
            &sink,
            "ask:research_started",
            &serde_json::json!({"trigger": "low_confidence", "confidence": conf}),
        );
        let chunks_added = match maybe_auto_research(state, game_id.as_deref().unwrap(), &question)
            .await
        {
            Ok(n) => n,
            Err(e) => {
                tracing::warn!("auto-research failed: {e}");
                0
            }
        };
        sink_emit(
            &sink,
            "ask:research_done",
            &serde_json::json!({"chunks_added": chunks_added}),
        );
        if chunks_added > 0 {
            // Re-run retrieval with the freshly-inserted chunks.
            let db = state.db.clone();
            let qv_clone = qv.clone();
            let gid_clone = game_id.clone();
            let q_clone = question.clone();
            pass = tokio::task::spawn_blocking(move || {
                retrieve_with_scores(
                    &db,
                    &qv_clone,
                    &q_clone,
                    gid_clone.as_deref(),
                    include_unofficial,
                )
            })
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;
        } else {
            research_appendix = Some("(没有找到外部资料，仅使用本地手册作答)");
        }
    } else if auto_research_enabled && conf < conf_threshold && pass.retrieved.is_empty() {
        // No local chunks at all and user wants auto-research: same hint.
        research_appendix = Some("(没有找到外部资料，仅使用本地手册作答)");
    }

    sink_emit(&sink, "ask:citations", &pass.citations);

    let mut messages = build_messages(&question, &pass.retrieved);
    if let Some(line) = research_appendix {
        if let Some(last) = messages.last_mut() {
            last.content.push_str("\n\n");
            last.content.push_str(line);
        }
    }
    let sink_for_tokens = sink.clone();
    let answer = stream_chat(messages, move |tok| {
        sink_emit(&sink_for_tokens, "ask:token", &tok.to_string());
    })
    .await?;

    let chunk_ids: Vec<i64> = pass.retrieved.iter().map(|c| c.chunk_id).collect();
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

/// Auto-research helper. Returns the number of chunks added (0 on timeout
/// or any failure). Bounded to `AUTO_RESEARCH_BUDGET`.
async fn maybe_auto_research(state: &AppState, game_id: &str, question: &str) -> AppResult<u32> {
    use crate::research::connectors::GameCtx;
    use crate::research::orchestrator::{
        self, OrchestratorDeps, ResearchPlan, DEFAULT_MAX_HITS_TO_FETCH,
    };

    let db = state.db.clone();
    let game = {
        let db = db.clone();
        let gid = game_id.to_string();
        tokio::task::spawn_blocking(move || store_games::get_game(&db, &gid))
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??
    };
    let game = match game {
        Some(g) => g,
        None => return Ok(0),
    };
    let bgg_id = {
        let db = db.clone();
        let gid = game_id.to_string();
        tokio::task::spawn_blocking(move || store_games::get_bgg_id(&db, &gid))
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??
    };

    let deps = OrchestratorDeps::production(db.clone())?;
    let plan = ResearchPlan {
        trigger: "low_confidence",
        query: question.to_string(),
        max_hits_to_fetch: DEFAULT_MAX_HITS_TO_FETCH,
    };
    let deadline = Instant::now() + AUTO_RESEARCH_BUDGET;
    let outcome_fut = async {
        let ctx = GameCtx {
            game_id: &game.id,
            bgg_id,
            name_zh: &game.name_zh,
            name_en: game.name_en.as_deref(),
            publisher_url: None,
        };
        orchestrator::run_research(&db, &ctx, plan, deadline, &deps).await
    };
    match tokio::time::timeout(AUTO_RESEARCH_BUDGET, outcome_fut).await {
        Ok(Ok(o)) => Ok(o.chunks_added),
        Ok(Err(e)) => {
            tracing::warn!("auto-research orchestrator error: {e}");
            Ok(0)
        }
        Err(_) => {
            tracing::warn!("auto-research budget exceeded ({}s)", AUTO_RESEARCH_BUDGET.as_secs());
            Ok(0)
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventSink;
    use crate::store::{games as store_games, pages as store_pages, Db};
    use std::sync::Mutex;

    fn synthetic_embedding(value: f32) -> Vec<f32> {
        vec![value; 1024]
    }

    /// Capture-into-vec event sink for assertions.
    fn capture_sink() -> (EventSink, Arc<Mutex<Vec<(String, serde_json::Value)>>>) {
        let store: Arc<Mutex<Vec<(String, serde_json::Value)>>> =
            Arc::new(Mutex::new(Vec::new()));
        let s = store.clone();
        let sink: EventSink = Arc::new(move |event: &str, payload: serde_json::Value| {
            s.lock().unwrap().push((event.to_string(), payload));
        });
        (sink, store)
    }

    /// Confidence math at the boundary: weak hits → below threshold.
    #[test]
    fn confidence_below_threshold_for_weak_unverified_hit() {
        let conf = compute_confidence(0.3, 0.0, "unverified");
        // 0.6*0.3 + 0.3*0 + 0.1*0.5 = 0.18 + 0 + 0.05 = 0.23
        assert!(conf < CONF_THRESHOLD);
    }

    /// retrieve_with_scores should respect include_unofficial=false by
    /// dropping `official=0` chunks from both vec + fts hits.
    #[test]
    fn retrieve_with_scores_filters_unofficial_when_requested() {
        let db = Db::open_in_memory().unwrap();
        let g = store_games::insert_game(&db, "G", None, None).unwrap();
        let p = store_pages::insert_page(&db, &g, 1, "/tmp/p.png", None).unwrap();

        // One official chunk (default photo_ocr provenance), one community.
        let _id_off = store_chunks::insert_chunk_with_embedding(
            &db,
            &p,
            &g,
            None,
            "official rule chunk",
            3,
            &synthetic_embedding(0.5),
        )
        .unwrap();
        let _id_unoff = store_chunks::insert_chunk_with_embedding_and_provenance(
            &db,
            &p,
            &g,
            None,
            "community rule chunk",
            3,
            &synthetic_embedding(0.5),
            &store_chunks::ChunkProvenance {
                source_kind: "bgg_forum",
                source_url: Some("https://example.com/t"),
                trust_tier: "community",
                official: false,
                confidence: 0.7,
                fetched_at: None,
                content_lang: "zh",
                content_orig: None,
            },
        )
        .unwrap();

        let qv = synthetic_embedding(0.5);
        let pass = retrieve_with_scores(&db, &qv, "rule chunk", Some(&g), false).unwrap();
        assert!(!pass.retrieved.is_empty(), "should still find official chunk");
        for c in &pass.citations {
            assert!(c.official, "non-official chunk leaked into filtered pass");
        }

        // With include_unofficial=true, both should be retrievable.
        let pass2 = retrieve_with_scores(&db, &qv, "rule chunk", Some(&g), true).unwrap();
        let kinds: Vec<&str> = pass2
            .citations
            .iter()
            .map(|c| c.source_kind.as_str())
            .collect();
        assert!(kinds.contains(&"photo_ocr"));
        assert!(kinds.contains(&"bgg_forum"));
    }

    #[test]
    fn citation_chunk_carries_provenance_fields() {
        let db = Db::open_in_memory().unwrap();
        let g = store_games::insert_game(&db, "P", None, None).unwrap();
        let p = store_pages::insert_page(&db, &g, 1, "/tmp/p.png", None).unwrap();
        store_chunks::insert_chunk_with_embedding_and_provenance(
            &db,
            &p,
            &g,
            None,
            "community snippet",
            2,
            &synthetic_embedding(0.5),
            &store_chunks::ChunkProvenance {
                source_kind: "bgg_forum",
                source_url: Some("https://bgg/x"),
                trust_tier: "community",
                official: false,
                confidence: 0.7,
                fetched_at: None,
                content_lang: "zh",
                content_orig: None,
            },
        )
        .unwrap();
        let pass =
            retrieve_with_scores(&db, &synthetic_embedding(0.5), "snippet", Some(&g), true)
                .unwrap();
        let c = pass.citations.first().expect("at least one citation");
        assert_eq!(c.source_kind, "bgg_forum");
        assert_eq!(c.source_url.as_deref(), Some("https://bgg/x"));
        assert_eq!(c.trust_tier, "community");
        assert!(!c.official);
    }

    /// Endorsed chunks should win RRF fusion over otherwise-equal peers.
    #[test]
    fn endorsement_promotes_chunks_in_retrieval() {
        let db = Db::open_in_memory().unwrap();
        let g = store_games::insert_game(&db, "E", None, None).unwrap();
        let p = store_pages::insert_page(&db, &g, 1, "/tmp/p.png", None).unwrap();
        let c1 = store_chunks::insert_chunk_with_embedding(
            &db,
            &p,
            &g,
            None,
            "neutral content one",
            3,
            &synthetic_embedding(0.5),
        )
        .unwrap();
        let c2 = store_chunks::insert_chunk_with_embedding(
            &db,
            &p,
            &g,
            None,
            "neutral content two",
            3,
            &synthetic_embedding(0.5),
        )
        .unwrap();
        store_chunks::update_chunk_endorsed(&db, c2, Some(true)).unwrap();
        let pass =
            retrieve_with_scores(&db, &synthetic_embedding(0.5), "neutral content", Some(&g), true)
                .unwrap();
        assert!(!pass.retrieved.is_empty());
        let top = pass.retrieved[0].chunk_id;
        assert_eq!(top, c2, "endorsed chunk should rank above the unendorsed peer");
        let _ = c1;
    }

    /// Simulates the auto-research re-retrieval path: a first pass returns
    /// nothing useful (no game data), then a fresh chunk is inserted (as the
    /// orchestrator would have done), and the SECOND retrieval pass picks it
    /// up. This exercises the same code path `run_ask` uses without spinning
    /// up the full LLM stack.
    #[test]
    fn second_retrieval_pass_picks_up_freshly_inserted_chunk() {
        let db = Db::open_in_memory().unwrap();
        let g = store_games::insert_game(&db, "Sim", None, None).unwrap();
        let p = store_pages::insert_page(&db, &g, 1, "/tmp/p.png", None).unwrap();

        let qv = synthetic_embedding(0.5);
        let pass1 = retrieve_with_scores(&db, &qv, "rule about kobold", Some(&g), true).unwrap();
        assert!(pass1.retrieved.is_empty());

        // Auto-research "inserts" a chunk:
        store_chunks::insert_chunk_with_embedding_and_provenance(
            &db,
            &p,
            &g,
            None,
            "kobold rule clarified by community",
            5,
            &qv,
            &store_chunks::ChunkProvenance {
                source_kind: "bgg_forum",
                source_url: Some("https://bgg/k"),
                trust_tier: "community",
                official: false,
                confidence: 0.7,
                fetched_at: None,
                content_lang: "zh",
                content_orig: None,
            },
        )
        .unwrap();

        let pass2 = retrieve_with_scores(&db, &qv, "rule about kobold", Some(&g), true).unwrap();
        assert!(!pass2.retrieved.is_empty());
        assert!(pass2.citations[0].source_kind == "bgg_forum");
    }

    /// Suppress unused warning on `capture_sink` until the auto-research
    /// integration test lands in Wave 5 E2E.
    #[test]
    fn capture_sink_smoke() {
        let (sink, store) = capture_sink();
        sink_emit(&sink, "x", &"hi");
        assert_eq!(store.lock().unwrap().len(), 1);
    }
}
