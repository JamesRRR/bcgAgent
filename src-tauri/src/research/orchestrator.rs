//! Wave 2 ask-time research orchestrator.
//!
//! The orchestrator owns the per-event lifecycle:
//!
//! 1. Bump the daily `research_budget`. Cap exceeded → record an event and
//!    return a zero-chunk outcome (no error).
//! 2. Fan out across registered connectors in parallel, deadline-bounded.
//! 3. Dedupe hits by URL, rank (`bgg_forum` > publisher web > other web),
//!    keep top-`max_hits_to_fetch`.
//! 4. Hydrate each chosen hit through `UrlFetchConnector` (parallel,
//!    deadline-bounded).
//! 5. Chunk + embed + insert each fetched page with provenance.
//! 6. Record a `research_events` row with the serialized hits + count.
//!
//! Wave 3: fetched English content is translated to Chinese before chunking
//! (canonical retrieval language is Chinese). Original English is retained
//! in `content_orig` for audit / re-translation.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use time::OffsetDateTime;
use tokio::time::timeout;

use super::connectors::url_fetch::UrlFetchConnector;
use super::connectors::{GameCtx, ResearchConnector, ResearchHit};
use crate::commands::chunker;
use crate::error::{AppError, AppResult};
use crate::llm::translate::{self, TranslateRequest};
use crate::store::{
    chunks as store_chunks, pages as store_pages, research as store_research, Db,
};

/// Default upper bound on URL fetches per event.
pub const DEFAULT_MAX_HITS_TO_FETCH: usize = 3;

#[derive(Debug, Clone)]
pub struct ResearchPlan {
    pub trigger: &'static str,
    pub query: String,
    pub max_hits_to_fetch: usize,
}

impl ResearchPlan {
    pub fn explicit(query: impl Into<String>) -> Self {
        Self {
            trigger: "user_explicit",
            query: query.into(),
            max_hits_to_fetch: DEFAULT_MAX_HITS_TO_FETCH,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ResearchOutcome {
    pub event_id: i64,
    pub chunks_added: u32,
    pub urls_fetched: Vec<String>,
    pub timed_out: bool,
}

/// Type alias for the embed function so tests can substitute a deterministic
/// stand-in instead of loading BGE-M3.
pub type EmbedFn = Arc<dyn Fn(&[String]) -> AppResult<Vec<Vec<f32>>> + Send + Sync>;

/// Async translation hook (Wave 3). Tests inject a canned passthrough so
/// no MiniMax call is required. Production wires it to
/// `translate::translate_to_chinese`.
pub type TranslateFn = Arc<
    dyn Fn(String) -> std::pin::Pin<Box<dyn std::future::Future<Output = AppResult<String>> + Send>>
        + Send
        + Sync,
>;

/// Internal config knob: connectors + embed fn + url fetcher (so tests can
/// stub the network layer without dragging in real HTTP).
pub struct OrchestratorDeps {
    pub connectors: Vec<Arc<dyn ResearchConnector>>,
    pub url_fetch: Arc<UrlFetchConnector>,
    pub embed_fn: EmbedFn,
    pub translate_fn: TranslateFn,
}

impl OrchestratorDeps {
    /// Production wiring: bgg_forum + web_search + url_fetch with the real
    /// BGE-M3 embedder. Reads the Brave key from `secrets/`.
    pub fn production(db: Db) -> AppResult<Self> {
        let bgg = Arc::new(super::connectors::bgg_forum::BggForumConnector::new())
            as Arc<dyn ResearchConnector>;
        let brave = Arc::new(super::connectors::web_search::WebSearchConnector::from_secrets()?)
            as Arc<dyn ResearchConnector>;
        let url_fetch = Arc::new(UrlFetchConnector::new(db));
        let embed_fn: EmbedFn = Arc::new(|texts: &[String]| crate::embed::embed_batch(texts));
        let translate_fn: TranslateFn = Arc::new(|text: String| {
            Box::pin(async move {
                translate::translate_to_chinese(TranslateRequest {
                    text: &text,
                    source_lang_hint: None,
                    domain: Some("桌游规则"),
                })
                .await
            })
        });
        Ok(Self {
            connectors: vec![bgg, brave],
            url_fetch,
            embed_fn,
            translate_fn,
        })
    }
}

/// Run one research pass synchronously.
pub async fn run_research(
    db: &Db,
    ctx: &GameCtx<'_>,
    plan: ResearchPlan,
    deadline: Instant,
    deps: &OrchestratorDeps,
) -> AppResult<ResearchOutcome> {
    let now = OffsetDateTime::now_utc().unix_timestamp();
    let query_normalized = normalize_query(&plan.query);

    // 1. Budget check. Errors out only when truly over the cap.
    let bump = {
        let db = db.clone();
        let game_id = ctx.game_id.to_string();
        tokio::task::spawn_blocking(move || store_research::increment_budget(&db, &game_id))
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))?
    };
    if bump.is_err() {
        let event_id = record_event(
            db,
            ctx.game_id,
            "budget_exceeded",
            &plan.query,
            &query_normalized,
            "[]",
            0,
            Some(0.0),
        )
        .await?;
        return Ok(ResearchOutcome {
            event_id,
            chunks_added: 0,
            urls_fetched: Vec::new(),
            timed_out: false,
        });
    }

    let mut timed_out = false;

    // 2. Fan-out connector search. Each connector runs concurrently with its
    // own slice of the deadline.
    let connector_search_futs = deps.connectors.iter().map(|c| {
        let conn = Arc::clone(c);
        let q = plan.query.clone();
        let game_id = ctx.game_id.to_string();
        let bgg_id = ctx.bgg_id;
        let name_zh = ctx.name_zh.to_string();
        let name_en = ctx.name_en.map(|s| s.to_string());
        let publisher_url = ctx.publisher_url.map(|s| s.to_string());
        async move {
            let local_ctx = GameCtx {
                game_id: &game_id,
                bgg_id,
                name_zh: &name_zh,
                name_en: name_en.as_deref(),
                publisher_url: publisher_url.as_deref(),
            };
            let until_deadline = deadline.saturating_duration_since(Instant::now());
            if until_deadline.is_zero() {
                return (conn.id(), Vec::new(), true);
            }
            match timeout(until_deadline, conn.search(&local_ctx, &q)).await {
                Ok(Ok(hits)) => (conn.id(), hits, false),
                Ok(Err(e)) => {
                    tracing::warn!("research connector {} failed: {e}", conn.id());
                    (conn.id(), Vec::new(), false)
                }
                Err(_) => {
                    tracing::warn!("research connector {} timed out", conn.id());
                    (conn.id(), Vec::new(), true)
                }
            }
        }
    });
    let results = futures::future::join_all(connector_search_futs).await;

    let mut all_hits: Vec<ResearchHit> = Vec::new();
    for (_id, hits, t) in results {
        if t {
            timed_out = true;
        }
        all_hits.extend(hits);
    }

    // 3. Dedupe + rank.
    let chosen = pick_top_hits(all_hits, ctx.publisher_url, plan.max_hits_to_fetch);

    // 4. Hydrate.
    let mut hits_for_event: Vec<ResearchHit> = Vec::with_capacity(chosen.len());
    let mut urls_fetched: Vec<String> = Vec::new();
    let mut pages_to_ingest: Vec<(ResearchHit, String)> = Vec::new();

    let fetch_futs = chosen.into_iter().map(|hit| {
        let url_fetch = Arc::clone(&deps.url_fetch);
        async move {
            let until_deadline = deadline.saturating_duration_since(Instant::now());
            if until_deadline.is_zero() {
                return (hit, Err(AppError::Other(anyhow::anyhow!("deadline"))), true);
            }
            // If the connector pre-loaded `full_text` (e.g. url_fetch itself),
            // skip the HTTP round-trip.
            if let Some(full) = hit.full_text.clone() {
                return (hit, Ok(full), false);
            }
            match timeout(until_deadline, url_fetch.fetch(&hit.url)).await {
                Ok(Ok(page)) => (hit, Ok(page.content_md), false),
                Ok(Err(e)) => {
                    tracing::warn!("research url_fetch failed: {e}");
                    (hit, Err(e), false)
                }
                Err(_) => (
                    hit,
                    Err(AppError::Other(anyhow::anyhow!("url_fetch timeout"))),
                    true,
                ),
            }
        }
    });
    let fetch_results = futures::future::join_all(fetch_futs).await;
    for (hit, res, t) in fetch_results {
        if t {
            timed_out = true;
        }
        match res {
            Ok(md) if !md.trim().is_empty() => {
                urls_fetched.push(hit.url.clone());
                hits_for_event.push(hit.clone());
                pages_to_ingest.push((hit, md));
            }
            _ => {
                hits_for_event.push(hit);
            }
        }
    }

    // 5. Chunk + embed + insert with provenance.
    let mut chunks_added: u32 = 0;
    if !pages_to_ingest.is_empty() {
        // Anchor every chunk to the game's first page (same convention as
        // the import-time pipeline). If the game has no pages yet, skip
        // chunk insertion — we still record the event.
        let pages = {
            let db = db.clone();
            let gid = ctx.game_id.to_string();
            tokio::task::spawn_blocking(move || store_pages::list_pages_by_game(&db, &gid))
                .await
                .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??
        };
        if let Some(anchor) = pages.first().map(|p| p.id.clone()) {
            for (hit, md) in pages_to_ingest {
                // Wave 3: translate the fetched English markdown to Chinese
                // BEFORE chunking. Failures fall back to the original text so
                // a flaky LLM never costs us a chunk; retrieval will just be
                // weaker on that hit until the next pass.
                let translated = match (deps.translate_fn)(md.clone()).await {
                    Ok(zh) if !zh.trim().is_empty() => zh,
                    Ok(_) => md.clone(),
                    Err(e) => {
                        tracing::warn!("research translate failed: {e}; using original");
                        md.clone()
                    }
                };
                let chunks_zh = chunker::chunk_markdown(&translated);
                let chunks_en = chunker::chunk_markdown(&md);
                if chunks_zh.is_empty() {
                    continue;
                }
                let texts: Vec<String> =
                    chunks_zh.iter().map(|c| c.content.clone()).collect();
                let embeds = (deps.embed_fn)(&texts)?;
                if embeds.len() != texts.len() {
                    return Err(AppError::Other(anyhow::anyhow!(
                        "embed_fn returned {} vectors for {} texts",
                        embeds.len(),
                        texts.len()
                    )));
                }
                for (i, (chunk, vec)) in chunks_zh.iter().zip(embeds.iter()).enumerate() {
                    // Best-effort pairing: same index in the English chunk
                    // list. If lengths differ (translation collapsed paragraphs)
                    // we pass `None` rather than mis-aligned text.
                    let orig = chunks_en.get(i).map(|c| c.content.as_str());
                    let prov = store_chunks::ChunkProvenance {
                        source_kind: hit.source_kind.as_str(),
                        source_url: Some(hit.url.as_str()),
                        trust_tier: hit.trust_tier.as_str(),
                        official: hit.trust_tier.is_official(),
                        confidence: 0.7,
                        fetched_at: Some(now),
                        content_lang: "zh",
                        content_orig: orig,
                    };
                    let db = db.clone();
                    let gid = ctx.game_id.to_string();
                    let anchor_owned = anchor.clone();
                    let heading = chunk.heading_path.clone();
                    let content_owned = chunk.content.clone();
                    let token_count = chunk.token_count as i64;
                    let vec_owned = vec.clone();
                    let prov_source_kind = prov.source_kind.to_string();
                    let prov_source_url = prov.source_url.map(|s| s.to_string());
                    let prov_trust_tier = prov.trust_tier.to_string();
                    let prov_official = prov.official;
                    let prov_confidence = prov.confidence;
                    let prov_fetched_at = prov.fetched_at;
                    let prov_content_lang = prov.content_lang.to_string();
                    let prov_content_orig = prov.content_orig.map(|s| s.to_string());
                    tokio::task::spawn_blocking(move || -> AppResult<()> {
                        let p = store_chunks::ChunkProvenance {
                            source_kind: &prov_source_kind,
                            source_url: prov_source_url.as_deref(),
                            trust_tier: &prov_trust_tier,
                            official: prov_official,
                            confidence: prov_confidence,
                            fetched_at: prov_fetched_at,
                            content_lang: &prov_content_lang,
                            content_orig: prov_content_orig.as_deref(),
                        };
                        store_chunks::insert_chunk_with_embedding_and_provenance(
                            &db,
                            &anchor_owned,
                            &gid,
                            heading.as_deref(),
                            &content_owned,
                            token_count,
                            &vec_owned,
                            &p,
                        )?;
                        Ok(())
                    })
                    .await
                    .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;
                    chunks_added += 1;
                }
            }
        } else {
            tracing::info!(
                "research: game {} has no pages yet; skipping chunk insertion",
                ctx.game_id
            );
        }
    }

    // 6. Record the event.
    let hits_json = serde_json::to_string(&hits_for_event)
        .map_err(|e| AppError::Other(anyhow::anyhow!("hits_json: {e}")))?;
    let event_id = record_event(
        db,
        ctx.game_id,
        plan.trigger,
        &plan.query,
        &query_normalized,
        &hits_json,
        chunks_added as i64,
        Some(0.0),
    )
    .await?;

    Ok(ResearchOutcome {
        event_id,
        chunks_added,
        urls_fetched,
        timed_out,
    })
}

async fn record_event(
    db: &Db,
    game_id: &str,
    trigger: &str,
    query: &str,
    query_normalized: &str,
    hits_json: &str,
    chunks_added: i64,
    cost_estimate: Option<f64>,
) -> AppResult<i64> {
    let db = db.clone();
    let gid = game_id.to_string();
    let trigger = trigger.to_string();
    let query = query.to_string();
    let qn = query_normalized.to_string();
    let hj = hits_json.to_string();
    tokio::task::spawn_blocking(move || -> AppResult<i64> {
        store_research::record_research_event(
            &db,
            &gid,
            &store_research::NewResearchEvent {
                trigger: &trigger,
                query: &query,
                query_normalized: &qn,
                hits_json: &hj,
                chunks_added,
                cost_estimate,
            },
        )
    })
    .await
    .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))?
}

/// Squash whitespace + lower-case so duplicate research events on the same
/// question dedupe via `(game_id, query_normalized)`.
fn normalize_query(q: &str) -> String {
    q.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// Dedupe by URL, rank `bgg_forum` first, then publisher-host web hits, then
/// other web hits. Stable within tier — preserves insertion order so the
/// per-connector top-K ranking shines through.
fn pick_top_hits(hits: Vec<ResearchHit>, publisher_url: Option<&str>, max: usize) -> Vec<ResearchHit> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut deduped: Vec<ResearchHit> = Vec::with_capacity(hits.len());
    for h in hits {
        if seen.insert(h.url.clone()) {
            deduped.push(h);
        }
    }
    let rank = |h: &ResearchHit| -> u8 {
        if h.source_kind == "bgg_forum" {
            0
        } else if h.source_kind == "web"
            && super::connectors::url_fetch::same_host(&h.url, publisher_url)
        {
            1
        } else {
            2
        }
    };
    // Stable sort preserves connector-internal ranking inside each tier.
    deduped.sort_by_key(|h| rank(h));
    deduped.truncate(max);
    deduped
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::research::connectors::{ResearchHit, TrustTier};
    use async_trait::async_trait;
    use std::sync::Mutex;
    use std::time::Duration;

    fn synthetic_embed(texts: &[String]) -> AppResult<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|_| vec![0.0f32; 1024]).collect())
    }

    fn embed_fn() -> EmbedFn {
        Arc::new(synthetic_embed)
    }

    /// Test translation hook: prepend "ZH:" so we can assert the orchestrator
    /// actually used translated text downstream.
    fn fake_translate_fn() -> TranslateFn {
        Arc::new(|text: String| {
            Box::pin(async move { Ok(format!("ZH:{}", text)) })
        })
    }

    /// Identity translation hook (no-op) for tests that only care about
    /// chunk counts / event recording, not translation behaviour.
    fn passthrough_translate_fn() -> TranslateFn {
        Arc::new(|text: String| Box::pin(async move { Ok(text) }))
    }

    /// A test connector that returns a pre-baked list and records every call.
    struct StubConnector {
        id: &'static str,
        tier: TrustTier,
        hits: Vec<ResearchHit>,
        calls: Mutex<u32>,
    }

    #[async_trait]
    impl ResearchConnector for StubConnector {
        fn id(&self) -> &'static str {
            self.id
        }
        fn default_tier(&self) -> TrustTier {
            self.tier
        }
        async fn search(&self, _ctx: &GameCtx<'_>, _query: &str) -> AppResult<Vec<ResearchHit>> {
            *self.calls.lock().unwrap() += 1;
            Ok(self.hits.clone())
        }
    }

    fn pre_seeded_url_fetch(db: &Db, urls_with_md: &[(&str, &str)]) -> Arc<UrlFetchConnector> {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        for (u, md) in urls_with_md {
            store_research::put_web_cache(
                db,
                u,
                Some(200),
                Some(md),
                None,
                None,
                now + 7 * 86_400,
            )
            .unwrap();
        }
        // Point the fetcher at a dead loopback — every URL must come from
        // cache, otherwise the test fails loudly.
        Arc::new(UrlFetchConnector::with_http_base(
            db.clone(),
            "http://127.0.0.1:1",
        ))
    }

    fn make_game_with_page(db: &Db) -> String {
        let g = crate::store::games::insert_game(db, "Stub", None, None).unwrap();
        crate::store::pages::insert_page(db, &g, 1, "/tmp/p.png", None).unwrap();
        g
    }

    #[tokio::test]
    async fn happy_path_inserts_chunks_and_records_event() {
        let db = Db::open_in_memory().unwrap();
        let g = make_game_with_page(&db);

        let hit_a = ResearchHit {
            url: "https://example.com/a".into(),
            title: "A".into(),
            snippet: "snip".into(),
            source_kind: "web".into(),
            trust_tier: TrustTier::Unverified,
            full_text: None,
        };
        let hit_b = ResearchHit {
            url: "https://boardgamegeek.com/thread/1".into(),
            title: "Thread".into(),
            snippet: "snip2".into(),
            source_kind: "bgg_forum".into(),
            trust_tier: TrustTier::Community,
            full_text: None,
        };

        let stub = Arc::new(StubConnector {
            id: "stub",
            tier: TrustTier::Community,
            hits: vec![hit_a.clone(), hit_b.clone()],
            calls: Mutex::new(0),
        }) as Arc<dyn ResearchConnector>;

        let url_fetch = pre_seeded_url_fetch(
            &db,
            &[
                (
                    &hit_a.url,
                    "# A title\n\nfirst paragraph of A\n\nsecond paragraph of A",
                ),
                (
                    &hit_b.url,
                    "# B title\n\nbody of forum thread",
                ),
            ],
        );

        let deps = OrchestratorDeps {
            connectors: vec![stub],
            url_fetch,
            embed_fn: embed_fn(),
            translate_fn: fake_translate_fn(),
        };
        let ctx = GameCtx {
            game_id: &g,
            bgg_id: Some(123),
            name_zh: "测试",
            name_en: None,
            publisher_url: None,
        };
        let plan = ResearchPlan::explicit("setup");
        let outcome = run_research(
            &db,
            &ctx,
            plan,
            Instant::now() + Duration::from_secs(5),
            &deps,
        )
        .await
        .unwrap();

        assert!(outcome.chunks_added >= 2, "expected chunks; got {outcome:?}");
        assert_eq!(outcome.urls_fetched.len(), 2);
        // bgg_forum should be ranked above web → first fetched URL is the bgg one.
        assert_eq!(outcome.urls_fetched[0], hit_b.url);

        // Budget incremented.
        assert_eq!(store_research::current_budget(&db, &g).unwrap(), 1);

        // research_events row exists with non-empty hits_json.
        let conn = db.lock();
        let (trigger, hits_json, chunks_added): (String, String, i64) = conn
            .query_row(
                "SELECT trigger, hits_json, chunks_added FROM research_events WHERE id = ?",
                rusqlite::params![outcome.event_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(trigger, "user_explicit");
        assert!(hits_json.contains("boardgamegeek.com"));
        assert_eq!(chunks_added as u32, outcome.chunks_added);
    }

    #[tokio::test]
    async fn dedupe_by_url() {
        let db = Db::open_in_memory().unwrap();
        let g = make_game_with_page(&db);
        let dup_url = "https://example.com/dup";
        let h = ResearchHit {
            url: dup_url.into(),
            title: "T".into(),
            snippet: "s".into(),
            source_kind: "web".into(),
            trust_tier: TrustTier::Unverified,
            full_text: None,
        };
        let stub_a = Arc::new(StubConnector {
            id: "a",
            tier: TrustTier::Community,
            hits: vec![h.clone()],
            calls: Mutex::new(0),
        }) as Arc<dyn ResearchConnector>;
        let stub_b = Arc::new(StubConnector {
            id: "b",
            tier: TrustTier::Community,
            hits: vec![h.clone()],
            calls: Mutex::new(0),
        }) as Arc<dyn ResearchConnector>;

        let url_fetch = pre_seeded_url_fetch(
            &db,
            &[(dup_url, "# X\n\nbody of X")],
        );
        let deps = OrchestratorDeps {
            connectors: vec![stub_a, stub_b],
            url_fetch,
            embed_fn: embed_fn(),
            translate_fn: passthrough_translate_fn(),
        };
        let ctx = GameCtx {
            game_id: &g,
            bgg_id: None,
            name_zh: "g",
            name_en: None,
            publisher_url: None,
        };
        let outcome = run_research(
            &db,
            &ctx,
            ResearchPlan::explicit("q"),
            Instant::now() + Duration::from_secs(5),
            &deps,
        )
        .await
        .unwrap();
        // Only ONE URL fetched even though two connectors returned the same hit.
        assert_eq!(outcome.urls_fetched.len(), 1);
    }

    #[tokio::test]
    async fn budget_exceeded_short_circuits_without_chunks() {
        let db = Db::open_in_memory().unwrap();
        let g = make_game_with_page(&db);
        // Drain the budget.
        for _ in 0..store_research::RESEARCH_DAILY_CAP {
            store_research::increment_budget(&db, &g).unwrap();
        }
        let h = ResearchHit {
            url: "https://example.com/x".into(),
            title: "X".into(),
            snippet: "s".into(),
            source_kind: "web".into(),
            trust_tier: TrustTier::Unverified,
            full_text: None,
        };
        let stub = Arc::new(StubConnector {
            id: "s",
            tier: TrustTier::Community,
            hits: vec![h],
            calls: Mutex::new(0),
        }) as Arc<dyn ResearchConnector>;
        let url_fetch = Arc::new(UrlFetchConnector::with_http_base(
            db.clone(),
            "http://127.0.0.1:1",
        ));
        let deps = OrchestratorDeps {
            connectors: vec![Arc::clone(&stub)],
            url_fetch,
            embed_fn: embed_fn(),
            translate_fn: passthrough_translate_fn(),
        };
        let ctx = GameCtx {
            game_id: &g,
            bgg_id: None,
            name_zh: "g",
            name_en: None,
            publisher_url: None,
        };
        let outcome = run_research(
            &db,
            &ctx,
            ResearchPlan::explicit("q"),
            Instant::now() + Duration::from_secs(2),
            &deps,
        )
        .await
        .unwrap();
        assert_eq!(outcome.chunks_added, 0);
        assert!(outcome.urls_fetched.is_empty());
        // Connector NOT called.
        // (We cast back to access calls — Arc<dyn> doesn't expose it; instead
        // assert via behaviour: zero chunks, zero urls.)

        // Event row recorded with budget_exceeded trigger.
        let conn = db.lock();
        let trigger: String = conn
            .query_row(
                "SELECT trigger FROM research_events WHERE id = ?",
                rusqlite::params![outcome.event_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(trigger, "budget_exceeded");
    }

    #[tokio::test]
    async fn orchestrator_writes_translated_text_to_content() {
        let db = Db::open_in_memory().unwrap();
        let g = make_game_with_page(&db);
        let hit = ResearchHit {
            url: "https://example.com/translate-me".into(),
            title: "T".into(),
            snippet: "s".into(),
            source_kind: "web".into(),
            trust_tier: TrustTier::Unverified,
            full_text: None,
        };
        let stub = Arc::new(StubConnector {
            id: "t",
            tier: TrustTier::Community,
            hits: vec![hit.clone()],
            calls: Mutex::new(0),
        }) as Arc<dyn ResearchConnector>;
        let url_fetch = pre_seeded_url_fetch(
            &db,
            &[(&hit.url, "# Heading\n\nThis is some English content for translation.")],
        );
        let deps = OrchestratorDeps {
            connectors: vec![stub],
            url_fetch,
            embed_fn: embed_fn(),
            translate_fn: fake_translate_fn(),
        };
        let ctx = GameCtx {
            game_id: &g,
            bgg_id: None,
            name_zh: "g",
            name_en: None,
            publisher_url: None,
        };
        let outcome = run_research(
            &db,
            &ctx,
            ResearchPlan::explicit("q"),
            Instant::now() + Duration::from_secs(5),
            &deps,
        )
        .await
        .unwrap();
        assert!(outcome.chunks_added >= 1);

        // Inspect the inserted chunks: content must start with our fake
        // translation marker `ZH:`, content_lang must be `zh`, content_orig
        // must hold the English text.
        let conn = db.lock();
        let (content, content_lang, content_orig): (String, String, Option<String>) = conn
            .query_row(
                "SELECT content, content_lang, content_orig FROM chunks \
                 WHERE game_id = ? AND source_url = ? LIMIT 1",
                rusqlite::params![&g, &hit.url],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert!(content.contains("ZH:"), "expected translated content; got: {}", content);
        assert_eq!(content_lang, "zh");
        assert!(content_orig.unwrap_or_default().contains("English content"));
    }

    #[test]
    fn pick_top_hits_orders_bgg_first_then_publisher_then_other() {
        let pub_h = ResearchHit {
            url: "https://pub.example.com/x".into(),
            title: "P".into(),
            snippet: "".into(),
            source_kind: "web".into(),
            trust_tier: TrustTier::Unverified,
            full_text: None,
        };
        let other_h = ResearchHit {
            url: "https://other.example.com/x".into(),
            title: "O".into(),
            snippet: "".into(),
            source_kind: "web".into(),
            trust_tier: TrustTier::Unverified,
            full_text: None,
        };
        let bgg_h = ResearchHit {
            url: "https://boardgamegeek.com/thread/1".into(),
            title: "B".into(),
            snippet: "".into(),
            source_kind: "bgg_forum".into(),
            trust_tier: TrustTier::Community,
            full_text: None,
        };
        let chosen = pick_top_hits(
            vec![pub_h.clone(), other_h.clone(), bgg_h.clone()],
            Some("https://pub.example.com"),
            10,
        );
        let order: Vec<String> = chosen.iter().map(|h| h.url.clone()).collect();
        assert_eq!(
            order,
            vec![
                "https://boardgamegeek.com/thread/1".to_string(),
                "https://pub.example.com/x".to_string(),
                "https://other.example.com/x".to_string(),
            ]
        );
    }
}
