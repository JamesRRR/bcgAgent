//! Run the import-time research pass for a game.
//!
//! Steps (in order, all idempotent):
//! 1. Resolve `bgg_id` (search by `name_zh` then `name_en`) and persist on `games`.
//! 2. Fetch BGG `<description>` → write to `game_external_refs` + chunk + embed.
//! 3. Fetch the "Rules" / "Reviews" forums, pick the top threads by article
//!    count, fetch each, write to `game_external_refs` + chunk + embed.
//! 4. Fetch one page of the BGG image gallery; write captions to
//!    `game_external_refs` + chunk + embed.
//!
//! Throttle: 1 request/sec (BGG ToS).
//! Quotas: at most 5 forum threads, at most 50 gallery captions.

use std::time::{Duration, Instant};

use crate::cover::bgg;
use crate::error::AppResult;
use crate::events::{self, EventSink};
use crate::extractors;
use crate::research::bgg_extra;
use crate::research::connectors::GameCtx;
use crate::research::orchestrator::{
    self, OrchestratorDeps, ResearchPlan, DEFAULT_MAX_HITS_TO_FETCH,
};
use crate::store::{
    chunks as store_chunks, external_refs, games as store_games, illustrations as store_ill,
    pages as store_pages, Db,
};

const REQUEST_GAP: Duration = Duration::from_millis(1100);
const MAX_FORUM_THREADS: usize = 5;
const MAX_GALLERY_PAGES: u32 = 1; // 50 images
const FORUM_TITLE_HINTS: &[&str] = &["rules", "reviews", "strategy", "general"];

#[derive(Debug, Default, Clone, serde::Serialize)]
pub struct ResearchSummary {
    pub bgg_id: Option<u32>,
    pub description_added: bool,
    pub forum_threads_added: usize,
    pub gallery_captions_added: usize,
    pub illustrations_captioned: usize,
    pub chunks_added: usize,
}

async fn sleep_throttle() {
    tokio::time::sleep(REQUEST_GAP).await;
}

/// Resolve a BGG id for the game by name. Returns the existing one if
/// `games.bgg_id` is already set; otherwise searches BGG and persists.
async fn ensure_bgg_id(db: &Db, game_id: &str) -> AppResult<Option<u32>> {
    let cached = {
        let db = db.clone();
        let gid = game_id.to_string();
        tokio::task::spawn_blocking(move || store_games::get_bgg_id(&db, &gid))
            .await
            .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!("join: {e}")))??
    };
    if let Some(id) = cached {
        return Ok(Some(id));
    }
    let game = {
        let db = db.clone();
        let gid = game_id.to_string();
        tokio::task::spawn_blocking(move || store_games::get_game(&db, &gid))
            .await
            .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!("join: {e}")))??
    };
    let game = match game {
        Some(g) => g,
        None => return Ok(None),
    };
    let mut hit: Option<bgg::BggMatch> = bgg::search(&game.name_zh).await?;
    if hit.is_none() {
        if let Some(name_en) = game.name_en.as_deref() {
            sleep_throttle().await;
            hit = bgg::search(name_en).await?;
        }
    }
    if let Some(m) = hit.as_ref() {
        let db = db.clone();
        let gid = game_id.to_string();
        let bid = m.id;
        tokio::task::spawn_blocking(move || store_games::set_bgg_id(&db, &gid, bid))
            .await
            .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!("join: {e}")))??;
    }
    Ok(hit.map(|m| m.id))
}

/// Embed `text` and persist it as a chunk attached to the game's first
/// existing page (so existing FK indexes work). Returns 1 on success, 0 on
/// no-op. The page_id is found via `pages.list_pages_by_game`. If the game
/// has no pages yet (caller mis-sequenced), we silently skip embedding —
/// the row is still in `game_external_refs` for direct context injection.
fn embed_external_chunk(db: &Db, game_id: &str, heading: &str, text: &str) -> AppResult<usize> {
    if text.trim().is_empty() {
        return Ok(0);
    }
    let pages = store_pages::list_pages_by_game(db, game_id)?;
    let anchor = match pages.first() {
        Some(p) => p.id.clone(),
        None => return Ok(0),
    };
    let vec = crate::embed::embed_batch(&[text.to_string()])?
        .into_iter()
        .next()
        .ok_or_else(|| {
            crate::error::AppError::Other(anyhow::anyhow!("embed_batch returned empty"))
        })?;
    let token_count = text.chars().count() as i64;
    store_chunks::insert_chunk_with_embedding(
        db,
        &anchor,
        game_id,
        Some(heading),
        text,
        token_count,
        &vec,
    )?;
    Ok(1)
}

/// Pull BGG description (long-form) and persist + embed it.
async fn pull_description(db: &Db, game_id: &str, bgg_id: u32) -> AppResult<(bool, usize)> {
    let thing = match bgg::fetch_thing_full(bgg_id).await? {
        Some(t) => t,
        None => return Ok((false, 0)),
    };
    let desc = thing.description.trim();
    if desc.is_empty() {
        return Ok((false, 0));
    }
    let url = format!("https://boardgamegeek.com/boardgame/{bgg_id}");
    let title = format!("BGG 描述 — {}", thing.primary_name);
    let content = desc.to_string();
    {
        let db = db.clone();
        let gid = game_id.to_string();
        let owned_title = title.clone();
        let owned_content = content.clone();
        let owned_url = url.clone();
        let owned_extid = bgg_id.to_string();
        tokio::task::spawn_blocking(move || -> AppResult<()> {
            let r = external_refs::NewExternalRef {
                source: "bgg",
                kind: "description",
                ext_id: Some(owned_extid.as_str()),
                title: Some(owned_title.as_str()),
                content: owned_content.as_str(),
                url: Some(owned_url.as_str()),
            };
            external_refs::upsert(&db, &gid, &r)?;
            Ok(())
        })
        .await
        .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!("join: {e}")))??;
    }

    // Chunk + embed each paragraph-bounded slice into RAG.
    let mut chunks_added = 0usize;
    for para in split_paragraphs(&content, 1500) {
        let db = db.clone();
        let gid = game_id.to_string();
        let heading = format!("BGG 描述 / {}", thing.primary_name);
        let n =
            tokio::task::spawn_blocking(move || embed_external_chunk(&db, &gid, &heading, &para))
                .await
                .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!("join: {e}")))??;
        chunks_added += n;
    }
    Ok((true, chunks_added))
}

async fn pull_forum(db: &Db, game_id: &str, bgg_id: u32) -> AppResult<(usize, usize)> {
    let forums = bgg_extra::list_forums(bgg_id).await?;
    sleep_throttle().await;

    // Order forums by usefulness: titles matching our hints first, then by
    // num_threads desc.
    let mut ranked: Vec<bgg_extra::ForumSummary> =
        forums.into_iter().filter(|f| f.num_threads > 0).collect();
    ranked.sort_by_key(|f| {
        let title = f.title.to_lowercase();
        let priority = FORUM_TITLE_HINTS
            .iter()
            .position(|h| title.contains(*h))
            .unwrap_or(usize::MAX);
        (priority, std::cmp::Reverse(f.num_threads))
    });

    let mut threads_added = 0usize;
    let mut chunks_added = 0usize;

    for forum in ranked.iter().take(2) {
        let threads = bgg_extra::list_threads(forum.id).await?;
        sleep_throttle().await;

        let mut top: Vec<_> = threads
            .into_iter()
            .filter(|t| t.num_articles >= 2)
            .collect();
        top.sort_by_key(|t| std::cmp::Reverse(t.num_articles));
        top.truncate(MAX_FORUM_THREADS - threads_added);

        for thread in top {
            if threads_added >= MAX_FORUM_THREADS {
                break;
            }
            let articles = bgg_extra::fetch_thread(thread.id).await?;
            sleep_throttle().await;

            if articles.is_empty() {
                continue;
            }

            let mut body = String::new();
            body.push_str(&format!("# {}\n\n", thread.subject));
            for a in articles.iter().take(8) {
                body.push_str(&format!("## {} 说\n", a.username));
                body.push_str(a.body.trim());
                body.push_str("\n\n");
            }

            let url = format!("https://boardgamegeek.com/thread/{}", thread.id);
            let owned_subject = thread.subject.clone();
            let owned_body = body.clone();
            let owned_url = url.clone();
            let owned_extid = thread.id.to_string();
            {
                let db = db.clone();
                let gid = game_id.to_string();
                tokio::task::spawn_blocking(move || -> AppResult<()> {
                    let r = external_refs::NewExternalRef {
                        source: "bgg",
                        kind: "forum",
                        ext_id: Some(owned_extid.as_str()),
                        title: Some(owned_subject.as_str()),
                        content: owned_body.as_str(),
                        url: Some(owned_url.as_str()),
                    };
                    external_refs::upsert(&db, &gid, &r)?;
                    Ok(())
                })
                .await
                .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!("join: {e}")))??;
            }

            for para in split_paragraphs(&body, 1500) {
                let db = db.clone();
                let gid = game_id.to_string();
                let heading = format!("BGG 论坛 / {}", thread.subject);
                let n = tokio::task::spawn_blocking(move || {
                    embed_external_chunk(&db, &gid, &heading, &para)
                })
                .await
                .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!("join: {e}")))??;
                chunks_added += n;
            }
            threads_added += 1;
        }

        if threads_added >= MAX_FORUM_THREADS {
            break;
        }
    }
    Ok((threads_added, chunks_added))
}

async fn pull_gallery(db: &Db, game_id: &str, bgg_id: u32) -> AppResult<(usize, usize)> {
    let mut captions_added = 0usize;
    let mut chunks_added = 0usize;

    for page in 1..=MAX_GALLERY_PAGES {
        let imgs = bgg_extra::fetch_gallery_page(bgg_id, page).await?;
        sleep_throttle().await;
        if imgs.is_empty() {
            break;
        }

        let mut combined = String::new();
        combined.push_str("# BGG 图库（玩家上传的实物照片与组件特写）\n\n");
        for img in imgs.iter() {
            let cap = img.caption.trim();
            if cap.is_empty() {
                continue;
            }
            combined.push_str(&format!("- {} ({})\n", cap, img.image_url));
            captions_added += 1;
        }

        if captions_added == 0 {
            continue;
        }

        let owned_combined = combined.clone();
        let owned_extid = format!("gallery-p{page}");
        {
            let db = db.clone();
            let gid = game_id.to_string();
            tokio::task::spawn_blocking(move || -> AppResult<()> {
                let r = external_refs::NewExternalRef {
                    source: "bgg",
                    kind: "gallery",
                    ext_id: Some(owned_extid.as_str()),
                    title: Some("BGG 图库（实物照片）"),
                    content: owned_combined.as_str(),
                    url: None,
                };
                external_refs::upsert(&db, &gid, &r)?;
                Ok(())
            })
            .await
            .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!("join: {e}")))??;
        }

        let db = db.clone();
        let gid = game_id.to_string();
        let n = tokio::task::spawn_blocking(move || {
            embed_external_chunk(&db, &gid, "BGG 图库", &combined)
        })
        .await
        .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!("join: {e}")))??;
        chunks_added += n;
    }

    Ok((captions_added, chunks_added))
}

/// Caption every illustration on the game that doesn't already have a
/// description set. Each caption ends up on `page_illustrations.description`
/// AND as a chunk so RAG/coach can retrieve "什么是 X 卡, 长什么样".
async fn caption_illustrations(db: &Db, game_id: &str) -> AppResult<(usize, usize)> {
    let illustrations = {
        let db = db.clone();
        let gid = game_id.to_string();
        tokio::task::spawn_blocking(move || store_ill::list_for_game(&db, &gid))
            .await
            .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!("join: {e}")))??
    };

    if illustrations.is_empty() {
        return Ok((0, 0));
    }

    // Map page_id → image_path so we don't re-query for every illustration.
    let pages = {
        let db = db.clone();
        let gid = game_id.to_string();
        tokio::task::spawn_blocking(move || store_pages::list_pages_by_game(&db, &gid))
            .await
            .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!("join: {e}")))??
    };
    let mut page_image: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    for p in &pages {
        if !p.image_path.is_empty() {
            page_image.insert(p.id.clone(), p.image_path.clone());
        }
    }

    let mut captioned = 0usize;
    let mut chunks_added = 0usize;
    for ill in illustrations {
        if ill
            .description
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
        {
            continue;
        }
        let img_path = match page_image.get(&ill.page_id) {
            Some(p) if !p.is_empty() => std::path::PathBuf::from(p),
            _ => continue, // external-imported page: no source image to crop
        };
        let bbox = (
            ill.bbox_x1 as u32,
            ill.bbox_y1 as u32,
            ill.bbox_x2 as u32,
            ill.bbox_y2 as u32,
        );
        let caption = match crate::ocr::qwen::caption_crop(&img_path, bbox).await {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("caption_crop failed for {}: {e}", ill.id);
                continue;
            }
        };
        let label = ill.label.clone().unwrap_or_default();
        let token = ill.token.clone().unwrap_or_default();
        let combined_text = if label.is_empty() {
            format!("{token} {caption}").trim().to_string()
        } else {
            format!("{token} {label} — {caption}").trim().to_string()
        };

        // Persist on page_illustrations.description.
        {
            let db = db.clone();
            let id = ill.id.clone();
            let cap = caption.clone();
            tokio::task::spawn_blocking(move || store_ill::set_description(&db, &id, &cap))
                .await
                .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!("join: {e}")))??;
        }
        captioned += 1;

        // Embed it as its own chunk so RAG/Q&A can hit it directly.
        let db = db.clone();
        let gid = game_id.to_string();
        let heading = format!("插图 / {}", label);
        let n = tokio::task::spawn_blocking(move || {
            embed_external_chunk(&db, &gid, &heading, &combined_text)
        })
        .await
        .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!("join: {e}")))??;
        chunks_added += n;
    }

    Ok((captioned, chunks_added))
}

/// Run the full research pass. Failures inside any step are logged and the
/// pipeline continues — partial knowledge is better than none.
///
/// `sink`, when supplied, is forwarded into `run_seed_crawl` so the UI banner
/// can react to `seed_crawl:done` (Wave 4).
pub async fn run_for_game(db: &Db, game_id: &str) -> AppResult<ResearchSummary> {
    run_for_game_with_sink(db, game_id, None).await
}

/// Same as `run_for_game` but with an event sink so the seed-crawl emit can
/// reach the frontend. Existing callers that don't care about the seed banner
/// keep using the no-sink form above.
pub async fn run_for_game_with_sink(
    db: &Db,
    game_id: &str,
    sink: Option<EventSink>,
) -> AppResult<ResearchSummary> {
    let mut s = ResearchSummary::default();

    let bgg_id = match ensure_bgg_id(db, game_id).await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("research: ensure_bgg_id failed: {e}");
            None
        }
    };
    s.bgg_id = bgg_id;

    // Caption illustrations FIRST — independent of BGG, runs even when the
    // BGG XML API is blocked or rate-limited.
    match caption_illustrations(db, game_id).await {
        Ok((c, n)) => {
            s.illustrations_captioned = c;
            s.chunks_added += n;
        }
        Err(e) => tracing::warn!("research: caption_illustrations failed: {e}"),
    }

    let bgg_id = match bgg_id {
        Some(id) => id,
        None => {
            tracing::info!("research: no BGG id resolved, skipping external pull");
            return Ok(s);
        }
    };
    sleep_throttle().await;

    match pull_description(db, game_id, bgg_id).await {
        Ok((added, n)) => {
            s.description_added = added;
            s.chunks_added += n;
        }
        Err(e) => tracing::warn!("research: pull_description failed: {e}"),
    }
    sleep_throttle().await;

    match pull_forum(db, game_id, bgg_id).await {
        Ok((threads, n)) => {
            s.forum_threads_added = threads;
            s.chunks_added += n;
        }
        Err(e) => tracing::warn!("research: pull_forum failed: {e}"),
    }
    sleep_throttle().await;

    match pull_gallery(db, game_id, bgg_id).await {
        Ok((caps, n)) => {
            s.gallery_captions_added = caps;
            s.chunks_added += n;
        }
        Err(e) => tracing::warn!("research: pull_gallery failed: {e}"),
    }

    // Wave 3 — final additive step: lazy seed crawl + structured extractors.
    // Failures here NEVER fail the import. The user has already seen
    // `ingest:done`; this work is bonus context.
    if let Err(e) = run_seed_crawl(db, game_id, sink.clone()).await {
        tracing::warn!("research: seed crawl failed: {e}");
    }

    Ok(s)
}

/// Wave 3 seed crawl + extractor sweep. Runs two short research events
/// (`{name} setup` + `{name} rules clarifications`), then fans out the three
/// structured extractors in parallel. All errors are logged and swallowed.
///
/// If `sink` is `Some`, emits `seed_crawl:done` with `{ game_id, chunks_added }`
/// when finished — used by the UI's seed-crawl banner (Wave 4).
pub async fn run_seed_crawl(
    db: &Db,
    game_id: &str,
    sink: Option<EventSink>,
) -> AppResult<()> {
    // Resolve game context once.
    let game = {
        let db = db.clone();
        let gid = game_id.to_string();
        tokio::task::spawn_blocking(move || store_games::get_game(&db, &gid))
            .await
            .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!("join: {e}")))??
    };
    let game = match game {
        Some(g) => g,
        None => {
            tracing::warn!("seed_crawl: game {game_id} not found");
            return Ok(());
        }
    };
    let bgg_id = {
        let db = db.clone();
        let gid = game_id.to_string();
        tokio::task::spawn_blocking(move || store_games::get_bgg_id(&db, &gid))
            .await
            .map_err(|e| crate::error::AppError::Other(anyhow::anyhow!("join: {e}")))??
    };

    // Pick the most identifiable name (English preferred; falls back to
    // Chinese) for the search query.
    let name_for_query: String = game
        .name_en
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| game.name_zh.clone());

    let queries = [
        format!("{} setup", name_for_query),
        format!("{} rules clarifications", name_for_query),
    ];

    // Build orchestrator deps once. If production wiring fails (missing key,
    // etc.) skip the crawl but still try the extractors.
    let crawl_chunks_added: u32 = match OrchestratorDeps::production(db.clone()) {
        Ok(deps) => {
            let ctx = GameCtx {
                game_id: &game.id,
                bgg_id,
                name_zh: &game.name_zh,
                name_en: game.name_en.as_deref(),
                publisher_url: None,
            };
            let mut total = 0u32;
            for q in &queries {
                let plan = ResearchPlan {
                    trigger: "seed_import",
                    query: q.clone(),
                    max_hits_to_fetch: DEFAULT_MAX_HITS_TO_FETCH,
                };
                let deadline = Instant::now() + Duration::from_secs(10);
                match orchestrator::run_research(db, &ctx, plan, deadline, &deps).await {
                    Ok(o) => total += o.chunks_added,
                    Err(e) => tracing::warn!("seed_crawl: research '{q}' failed: {e}"),
                }
            }
            total
        }
        Err(e) => {
            tracing::warn!("seed_crawl: orchestrator deps unavailable: {e}");
            0
        }
    };

    // Run all three extractors in parallel. tokio::join! returns a tuple of
    // results; we log + swallow any failures.
    let (c, f, s) = tokio::join!(
        extractors::extract_components(db, game_id),
        extractors::extract_faqs(db, game_id),
        extractors::extract_setup(db, game_id),
    );
    let comp = c.unwrap_or_else(|e| {
        tracing::warn!("seed_crawl: components extractor failed: {e}");
        Default::default()
    });
    let faqs = f.unwrap_or_else(|e| {
        tracing::warn!("seed_crawl: faq extractor failed: {e}");
        Default::default()
    });
    let setup = s.unwrap_or_else(|e| {
        tracing::warn!("seed_crawl: setup extractor failed: {e}");
        Default::default()
    });

    let total_chunks =
        crawl_chunks_added + comp.chunks_added + faqs.chunks_added + setup.chunks_added;
    tracing::info!(
        "seed_crawl summary: components={}, faqs={}, setup_steps={}, chunks_added={}",
        comp.created,
        faqs.created,
        setup.created,
        total_chunks
    );
    if let Some(s) = sink.as_ref() {
        events::emit(
            s,
            "seed_crawl:done",
            &serde_json::json!({
                "game_id": game_id,
                "chunks_added": total_chunks,
            }),
        );
    }
    Ok(())
}

/// Greedy paragraph-bounded splitter. Same shape as
/// `import_external::split_into_pages` but kept private so research can tune
/// its own size budget.
fn split_paragraphs(text: &str, target: usize) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::with_capacity(target);
    for para in text.split("\n\n") {
        let p = para.trim();
        if p.is_empty() {
            continue;
        }
        if cur.len() + p.len() + 2 > target && !cur.is_empty() {
            out.push(std::mem::take(&mut cur).trim().to_string());
        }
        if !cur.is_empty() {
            cur.push_str("\n\n");
        }
        cur.push_str(p);
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    if out.is_empty() {
        out.push(text.trim().to_string());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_paragraphs_keeps_boundaries() {
        let s = "para one is short\n\npara two is also short\n\npara three";
        let p = split_paragraphs(s, 25);
        assert!(p.len() >= 2);
        for chunk in &p {
            assert!(!chunk.is_empty());
        }
    }
}
