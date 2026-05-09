//! End-to-end demo of the KB research pipeline.
//!
//! Copies the user's live DB to a tempdir (read-only on the original),
//! runs Wave 1 migrations on the copy, optionally seeds a synthetic Chinese
//! rulebook if the chosen game has no pages, then exercises the full pipeline:
//!  - extractors (components/faq/setup)
//!  - explicit research with 2 representative questions
//! Prints a structured before/after diff to stdout.
//!
//! Usage:
//!   cargo run --example kb_e2e -- --game-id <uuid> [--seed-if-empty]
//!   cargo run --example kb_e2e --                                # picks first game

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context};
use bcgagent_lib::commands::chunker;
use bcgagent_lib::embed;
use bcgagent_lib::extractors;
use bcgagent_lib::paths;
use bcgagent_lib::research::connectors::url_fetch::UrlFetchConnector;
use bcgagent_lib::research::connectors::{
    bgg_forum::BggForumConnector, web_search::WebSearchConnector, GameCtx, ResearchConnector,
};
use bcgagent_lib::research::orchestrator::{
    self, EmbedFn, OrchestratorDeps, ResearchPlan, TranslateFn,
};
use bcgagent_lib::store::{
    chunks as store_chunks, games as store_games, illustrations as store_ill,
    pages as store_pages, Db,
};

const CATAN_BGG_ID: u32 = 13;

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,bcgagent_lib=info".into()),
        )
        .init();

    if let Err(e) = run().await {
        eprintln!("FATAL: {e:#}");
        std::process::exit(1);
    }
}

#[derive(Debug)]
struct Args {
    game_id: Option<String>,
    seed_if_empty: bool,
}

fn parse_args() -> Args {
    let mut game_id = None;
    let mut seed_if_empty = false;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--game-id" => game_id = it.next(),
            "--seed-if-empty" => seed_if_empty = true,
            "--help" | "-h" => {
                println!(
                    "Usage: cargo run --example kb_e2e -- [--game-id <uuid>] [--seed-if-empty]"
                );
                std::process::exit(0);
            }
            other => eprintln!("warn: ignoring unknown arg {other}"),
        }
    }
    Args {
        game_id,
        seed_if_empty,
    }
}

async fn run() -> anyhow::Result<()> {
    let args = parse_args();

    // 1. Copy user DB to a tempdir so we never write back.
    let tmp = tempfile::tempdir().context("tempdir")?;
    let dst_db = tmp.path().join("db.sqlite");
    let src_db = paths::db_path();
    if !src_db.exists() {
        return Err(anyhow!(
            "user DB not found at {}; nothing to copy",
            src_db.display()
        ));
    }
    std::fs::copy(&src_db, &dst_db).with_context(|| format!("copy {}", src_db.display()))?;
    eprintln!(
        "[kb_e2e] copied {} -> {}",
        src_db.display(),
        dst_db.display()
    );

    // 2. Open at the new path (Wave 1 migrations + defensive ALTERs run here).
    let db = Db::open_at_path(&dst_db).context("open copy")?;

    // 3. Pick a game.
    let game = pick_game(&db, args.game_id.as_deref())?;
    eprintln!(
        "[kb_e2e] using game {} ({}) page_count={}",
        game.id, game.name_zh, game.page_count
    );

    // 4. Seed if empty + flag.
    if game.page_count == 0 && args.seed_if_empty {
        seed_synthetic_catan(&db, &game.id).await?;
        eprintln!("[kb_e2e] seeded synthetic rulebook + illustrations");
    } else if game.page_count == 0 {
        eprintln!(
            "[kb_e2e] WARNING: game has 0 pages and --seed-if-empty not given; \
             extractors/research will likely produce nothing"
        );
    }

    // 5. Make sure we have a usable bgg_id for BGG forum search. Catan = 13.
    let bgg_id = match store_games::get_bgg_id(&db, &game.id)? {
        Some(v) => Some(v),
        None => {
            // Best-effort: only fill in if the name strongly suggests Catan.
            if game.name_zh.contains("卡坦")
                || game
                    .name_en
                    .as_deref()
                    .map(|s| s.to_ascii_lowercase().contains("catan"))
                    .unwrap_or(false)
            {
                store_games::set_bgg_id(&db, &game.id, CATAN_BGG_ID).ok();
                Some(CATAN_BGG_ID)
            } else {
                None
            }
        }
    };
    eprintln!("[kb_e2e] bgg_id resolved to {bgg_id:?}");

    // 6. BEFORE snapshot — captured AFTER seed but BEFORE any extractor/research.
    let before = snapshot(&db, &game.id)?;

    // 7. Run extractors. Each wrapped in a 60s timeout. Errors logged but
    //    don't abort the run — the diff should reflect partial progress.
    eprintln!("[kb_e2e] running extractors (real MiniMax, ~$0.01)...");
    run_with_timeout(
        "components",
        Duration::from_secs(60),
        extractors::extract_components(&db, &game.id),
    )
    .await;
    run_with_timeout(
        "faq",
        Duration::from_secs(60),
        extractors::extract_faqs(&db, &game.id),
    )
    .await;
    run_with_timeout(
        "setup",
        Duration::from_secs(60),
        extractors::extract_setup(&db, &game.id),
    )
    .await;

    // 8. Run two explicit research passes. Each with a 20s deadline.
    let deps = build_orchestrator_deps(db.clone())?;
    let queries = [
        "卡坦岛 强盗 规则".to_string(),
        "卡坦岛 建造 城市".to_string(),
    ];
    for q in &queries {
        eprintln!("[kb_e2e] research: {q}");
        let ctx = GameCtx {
            game_id: &game.id,
            bgg_id,
            name_zh: &game.name_zh,
            name_en: game.name_en.as_deref(),
            publisher_url: None,
        };
        let plan = ResearchPlan::explicit(q.clone());
        let deadline = Instant::now() + Duration::from_secs(20);
        match orchestrator::run_research(&db, &ctx, plan, deadline, &deps).await {
            Ok(out) => eprintln!(
                "[kb_e2e]   -> chunks_added={} urls_fetched={} timed_out={}",
                out.chunks_added,
                out.urls_fetched.len(),
                out.timed_out
            ),
            Err(e) => eprintln!("[kb_e2e]   -> FAILED: {e}"),
        }
    }

    // 9. AFTER snapshot.
    let after = snapshot(&db, &game.id)?;

    // 10. Print markdown diff.
    print_markdown_diff(&db, &game, &before, &after)?;

    // 11. Sample retrieve.
    print_sample_retrieve(&db, &game.id, "强盗如何使用?")?;

    Ok(())
}

#[derive(Debug, Clone)]
struct GameRow {
    id: String,
    name_zh: String,
    name_en: Option<String>,
    page_count: i64,
}

fn pick_game(db: &Db, requested: Option<&str>) -> anyhow::Result<GameRow> {
    let games = store_games::list_games(db)?;
    if games.is_empty() {
        return Err(anyhow!("no games in DB"));
    }
    let g = match requested {
        Some(id) => games
            .iter()
            .find(|g| g.id == id)
            .cloned()
            .ok_or_else(|| anyhow!("game id {id} not found"))?,
        None => games[0].clone(),
    };
    Ok(GameRow {
        id: g.id,
        name_zh: g.name_zh,
        name_en: g.name_en,
        page_count: g.page_count,
    })
}

async fn seed_synthetic_catan(db: &Db, game_id: &str) -> anyhow::Result<()> {
    // Two pages of synthetic Chinese rulebook content. Kept under ~500 chars
    // each so the harness output is small and the embed/extract calls are
    // cheap.
    let p1_md = r#"# 设置

1. 把六边形地形板拼成大六边形。
2. 在每块地形板上随机放置一个数字标记（2-12，跳过 7）。
3. 沙漠格子放置强盗。
4. 每位玩家选择一种颜色，拿取 5 个定居点、4 个城市与 15 条道路。
5. 在自己回合按顺序在交叉点放置两个起始定居点与两条相邻的道路。"#;

    let p2_md = r#"# 资源与强盗

每回合开始时投掷两颗骰子。所有相邻数字与点数相等的地形产出资源。掷出 7 时强盗出动：所有手牌超过 7 张的玩家弃掉一半，并允许当前玩家移动强盗到任意非沙漠格，再从该格相邻一名玩家手中随机抽走一张资源卡。

资源用于建造：道路花费 1 木 + 1 砖；定居点花费 1 木 + 1 砖 + 1 麦 + 1 羊；城市花费 2 麦 + 3 矿，城市替换已有定居点，每回合产出双倍资源。"#;

    let pages = [(1i64, p1_md), (2i64, p2_md)];

    let mut page_ids: Vec<String> = Vec::new();
    for (n, md) in pages.iter() {
        let pid = store_pages::insert_page(db, game_id, *n, &format!("/seed/p{n}.png"), None)?;
        store_pages::set_ocr_result(db, &pid, "done", Some(md), None)?;
        page_ids.push(pid);
    }

    // Chunk + embed each page. One real BGE-M3 batch.
    for (idx, (_n, md)) in pages.iter().enumerate() {
        let chunked = chunker::chunk_markdown(md);
        if chunked.is_empty() {
            continue;
        }
        let texts: Vec<String> = chunked.iter().map(|c| c.content.clone()).collect();
        let embeds = embed::embed_batch(&texts)?;
        if embeds.len() != texts.len() {
            return Err(anyhow!("embed_batch returned wrong count"));
        }
        for (chunk, vec) in chunked.iter().zip(embeds.iter()) {
            let prov = store_chunks::ChunkProvenance {
                source_kind: "photo_ocr",
                source_url: None,
                trust_tier: "publisher",
                official: true,
                confidence: 1.0,
                fetched_at: None,
                content_lang: "zh",
                content_orig: None,
            };
            store_chunks::insert_chunk_with_embedding_and_provenance(
                db,
                &page_ids[idx],
                game_id,
                chunk.heading_path.as_deref(),
                &chunk.content,
                chunk.token_count as i64,
                vec,
                &prov,
            )?;
        }
    }

    // 3 fake illustrations on page 2 to mirror real shape.
    let labels = ["建造卡：道路", "建造卡：定居点", "建造卡：城市"];
    for (i, label) in labels.iter().enumerate() {
        let id = uuid::Uuid::new_v4().to_string();
        let token = format!("ill:{i}");
        store_ill::insert(
            db,
            &id,
            &page_ids[1],
            game_id,
            i as i64,
            &format!("/seed/ill{i}.png"),
            (10 + i as u32 * 100, 10, 100 + i as u32 * 100, 100),
            Some(label),
            Some(&token),
        )?;
    }

    // Bump page_count to match.
    store_games::increment_page_count(db, game_id)?;
    store_games::increment_page_count(db, game_id)?;

    Ok(())
}

#[derive(Debug, Clone, Default)]
struct Snapshot {
    chunks_by_source_kind: BTreeMap<String, u64>,
    components: u64,
    faq_pairs: u64,
    setup_steps: u64,
    research_events: u64,
}

fn snapshot(db: &Db, game_id: &str) -> anyhow::Result<Snapshot> {
    let counts = store_chunks::count_chunks_by_source_kind(db, game_id)?;
    let mut s = Snapshot::default();
    for (k, n) in counts {
        s.chunks_by_source_kind.insert(k, n);
    }
    let (c, f, st, re) = db.with_conn(|conn| {
        let c = conn
            .query_row(
                "SELECT COUNT(*) FROM components WHERE game_id = ?",
                rusqlite::params![game_id],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            .max(0) as u64;
        let f = conn
            .query_row(
                "SELECT COUNT(*) FROM faq_pairs WHERE game_id = ?",
                rusqlite::params![game_id],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            .max(0) as u64;
        let st = conn
            .query_row(
                "SELECT COUNT(*) FROM setup_steps WHERE game_id = ?",
                rusqlite::params![game_id],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            .max(0) as u64;
        let re = conn
            .query_row(
                "SELECT COUNT(*) FROM research_events WHERE game_id = ?",
                rusqlite::params![game_id],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0)
            .max(0) as u64;
        (c, f, st, re)
    });
    s.components = c;
    s.faq_pairs = f;
    s.setup_steps = st;
    s.research_events = re;
    Ok(s)
}

fn build_orchestrator_deps(db: Db) -> anyhow::Result<OrchestratorDeps> {
    // We bypass `OrchestratorDeps::production` because that would re-load
    // secrets via app paths (fine here since we use the user's real secrets
    // dir on disk), but we want to be explicit so the harness logs whether
    // the Brave key is present.
    let bgg = Arc::new(BggForumConnector::new()) as Arc<dyn ResearchConnector>;
    let brave = Arc::new(WebSearchConnector::from_secrets()?) as Arc<dyn ResearchConnector>;
    let url_fetch = Arc::new(UrlFetchConnector::new(db.clone()));
    let embed_fn: EmbedFn = Arc::new(|texts: &[String]| embed::embed_batch(texts));
    let translate_fn: TranslateFn = Arc::new(|text: String| {
        Box::pin(async move {
            bcgagent_lib::llm::translate::translate_to_chinese(
                bcgagent_lib::llm::translate::TranslateRequest {
                    text: &text,
                    source_lang_hint: None,
                    domain: Some("桌游规则"),
                },
            )
            .await
        })
    });
    Ok(OrchestratorDeps {
        connectors: vec![bgg, brave],
        url_fetch,
        embed_fn,
        translate_fn,
    })
}

async fn run_with_timeout<F, T>(label: &str, dur: Duration, fut: F)
where
    F: std::future::Future<Output = bcgagent_lib::error::AppResult<T>>,
    T: std::fmt::Debug,
{
    match tokio::time::timeout(dur, fut).await {
        Ok(Ok(v)) => eprintln!("[kb_e2e] extractor {label}: ok ({v:?})"),
        Ok(Err(e)) => eprintln!("[kb_e2e] extractor {label}: ERROR {e}"),
        Err(_) => eprintln!("[kb_e2e] extractor {label}: TIMEOUT after {dur:?}"),
    }
}

fn print_markdown_diff(
    db: &Db,
    game: &GameRow,
    before: &Snapshot,
    after: &Snapshot,
) -> anyhow::Result<()> {
    println!();
    println!("## KB Diff: {} ({})", game.name_zh, game.id);
    println!();

    // Chunks by source kind — union of before+after keys.
    println!("### Chunks by source_kind");
    println!();
    println!("| source_kind | before | after | delta |");
    println!("|---|---:|---:|---:|");
    let mut all_keys: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for k in before.chunks_by_source_kind.keys() {
        all_keys.insert(k.clone());
    }
    for k in after.chunks_by_source_kind.keys() {
        all_keys.insert(k.clone());
    }
    for k in &all_keys {
        let b = before.chunks_by_source_kind.get(k).copied().unwrap_or(0);
        let a = after.chunks_by_source_kind.get(k).copied().unwrap_or(0);
        let d = a as i64 - b as i64;
        let dstr = if d > 0 { format!("+{d}") } else { d.to_string() };
        println!("| {k} | {b} | {a} | {dstr} |");
    }

    println!();
    println!("### Structured tables");
    println!();
    println!("| table | before | after | delta |");
    println!("|---|---:|---:|---:|");
    for (label, b, a) in [
        ("components", before.components, after.components),
        ("faq_pairs", before.faq_pairs, after.faq_pairs),
        ("setup_steps", before.setup_steps, after.setup_steps),
        (
            "research_events",
            before.research_events,
            after.research_events,
        ),
    ] {
        let d = a as i64 - b as i64;
        let dstr = if d > 0 { format!("+{d}") } else { d.to_string() };
        println!("| {label} | {b} | {a} | {dstr} |");
    }

    println!();
    println!("### Sample of new chunks");
    println!();
    let rows: Vec<(String, String, f64, Option<String>, String)> = db.with_conn(|conn| {
        let mut stmt = conn.prepare(
            "SELECT source_kind, trust_tier, confidence, source_url, content \
             FROM chunks \
             WHERE game_id = ? AND source_kind != 'photo_ocr' \
             ORDER BY id DESC LIMIT 8",
        )?;
        let rows: Vec<(String, String, f64, Option<String>, String)> = stmt
            .query_map(rusqlite::params![&game.id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })?
            .collect::<Result<Vec<_>, rusqlite::Error>>()?;
        Ok::<_, rusqlite::Error>(rows)
    })?;
    if rows.is_empty() {
        println!("(no non-rulebook chunks to show)");
    } else {
        for (kind, tier, conf, url, content) in rows {
            let snippet: String = content.chars().take(80).collect();
            let url_part = url
                .as_deref()
                .map(|u| format!("({u}) "))
                .unwrap_or_default();
            println!("- [{kind} | {tier} | conf={conf:.2}] {url_part}{snippet}");
        }
    }
    println!();
    Ok(())
}

fn print_sample_retrieve(db: &Db, game_id: &str, query: &str) -> anyhow::Result<()> {
    println!("### Sample retrieve on \"{query}\"");
    println!();
    let qv = match embed::embed_query(query) {
        Ok(v) => v,
        Err(e) => {
            println!("(embed failed: {e})");
            return Ok(());
        }
    };
    let vec_hits = store_chunks::vec_search(db, &qv, Some(game_id), 3)?;
    if vec_hits.is_empty() {
        println!("(no hits)");
        return Ok(());
    }
    println!("Top 3 hits (cosine | tier | source_kind):");
    for (rank, (cid, dist)) in vec_hits.iter().enumerate() {
        let cos = (1.0_f32 - dist / 2.0).clamp(0.0, 1.0);
        let row: Option<(String, String, String)> = db.with_conn(|conn| {
            conn.query_row(
                "SELECT trust_tier, source_kind, content FROM chunks WHERE id = ?",
                rusqlite::params![cid],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                },
            )
            .ok()
        });
        if let Some((tier, kind, content)) = row {
            let snippet: String = content.chars().take(60).collect();
            println!(
                "  {}. {:.2} | {} | {} | {}",
                rank + 1,
                cos,
                tier,
                kind,
                snippet
            );
        }
    }
    println!();
    Ok(())
}

// Suppress unused-warning for `PathBuf` if rust-analyzer flags it.
#[allow(dead_code)]
fn _unused() -> PathBuf {
    PathBuf::new()
}
