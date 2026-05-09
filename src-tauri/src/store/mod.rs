//! SQLite storage layer (Wave 1.2).
//!
//! Connection management, schema migrations, and CRUD/search helpers for
//! games, pages, chunks (with `sqlite-vec` 1024-d embeddings + jieba-tokenized
//! FTS5), and Q&A history.

pub mod db;
pub mod jieba;
pub mod models;

pub mod chunks;
pub mod components;
pub mod external_refs;
pub mod faq_pairs;
pub mod games;
pub mod illustrations;
pub mod migrations_wave1;
pub mod pages;
pub mod qa;
pub mod research;
pub mod settings;
pub mod setup_steps;
pub mod walkthrough_sessions;
pub mod walkthroughs;

pub use db::Db;
pub use models::{Chunk, Game, Page, QAHistory};

#[cfg(test)]
mod tests {
    use super::*;

    fn synthetic_embedding(value: f32) -> Vec<f32> {
        vec![value; 1024]
    }

    #[test]
    fn migrations_run_on_in_memory() {
        let db = Db::open_in_memory().expect("open in-memory db");
        // sqlite-vec sanity check
        let conn = db.lock();
        let v: String = conn
            .query_row("SELECT vec_version()", [], |row| row.get(0))
            .expect("vec_version available");
        assert!(v.starts_with('v'), "got vec version: {}", v);
    }

    #[test]
    fn vec_search_returns_nearest() {
        let db = Db::open_in_memory().unwrap();
        let game_id = games::insert_game(&db, "测试游戏", None, None).unwrap();
        let p1 = pages::insert_page(&db, &game_id, 1, "/tmp/p1.png", None).unwrap();
        let p2 = pages::insert_page(&db, &game_id, 2, "/tmp/p2.png", None).unwrap();

        let low = synthetic_embedding(0.1);
        let high = synthetic_embedding(0.9);

        let c_low = chunks::insert_chunk_with_embedding(
            &db,
            &p1,
            &game_id,
            Some("rules"),
            "low embedding chunk",
            4,
            &low,
        )
        .unwrap();
        let c_high = chunks::insert_chunk_with_embedding(
            &db,
            &p2,
            &game_id,
            Some("rules"),
            "high embedding chunk",
            4,
            &high,
        )
        .unwrap();
        let _c_mid = chunks::insert_chunk_with_embedding(
            &db,
            &p1,
            &game_id,
            None,
            "mid embedding chunk",
            4,
            &synthetic_embedding(0.5),
        )
        .unwrap();

        let q = synthetic_embedding(0.11);
        let hits = chunks::vec_search(&db, &q, None, 1).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, c_low, "expected low-embedding chunk to win");

        let q2 = synthetic_embedding(0.91);
        let hits2 = chunks::vec_search(&db, &q2, Some(&game_id), 1).unwrap();
        assert_eq!(hits2.len(), 1);
        assert_eq!(hits2[0].0, c_high);
    }

    #[test]
    fn fts_search_finds_chinese_token() {
        let db = Db::open_in_memory().unwrap();
        let game_id = games::insert_game(&db, "战棋测试", None, None).unwrap();
        let page_id = pages::insert_page(&db, &game_id, 1, "/tmp/p.png", None).unwrap();

        let knight_id = chunks::insert_chunk_with_embedding(
            &db,
            &page_id,
            &game_id,
            Some("Units"),
            "骑士的攻击力是2点",
            8,
            &synthetic_embedding(0.2),
        )
        .unwrap();
        let _archer_id = chunks::insert_chunk_with_embedding(
            &db,
            &page_id,
            &game_id,
            Some("Units"),
            "弓箭手的射程是3格",
            8,
            &synthetic_embedding(0.3),
        )
        .unwrap();

        let hits = chunks::fts_search(&db, "骑士", None, 5).unwrap();
        assert!(!hits.is_empty(), "expected FTS hit for 骑士");
        assert_eq!(hits[0].0, knight_id);
    }

    #[test]
    fn qa_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let game_id = games::insert_game(&db, "RoundTrip", None, None).unwrap();
        let qa_id = qa::insert_qa(
            &db,
            Some(&game_id),
            "How does combat work?",
            Some("Roll a d6."),
            None,
            Some("[1,2,3]"),
        )
        .unwrap();

        let listed = qa::list_qa(&db, Some(&game_id), 10).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, qa_id);
        assert_eq!(listed[0].question, "How does combat work?");
        assert_eq!(listed[0].retrieved_chunk_ids.as_deref(), Some("[1,2,3]"));
    }

    #[test]
    fn page_count_increment_and_cover() {
        let db = Db::open_in_memory().unwrap();
        let g = games::insert_game(&db, "G", None, None).unwrap();
        games::increment_page_count(&db, &g).unwrap();
        games::increment_page_count(&db, &g).unwrap();
        games::set_cover(&db, &g, "/tmp/cover.png").unwrap();

        let got = games::get_game(&db, &g).unwrap().unwrap();
        assert_eq!(got.page_count, 2);
        assert_eq!(got.cover_path.as_deref(), Some("/tmp/cover.png"));
    }

    // -- Wave 1: provenance + KB tables ---------------------------------

    /// Synthetic embed_fn for backfill tests — bypasses the heavy BGE-M3
    /// model. Returns one zero-vector per input.
    fn synthetic_embed_fn(texts: &[String]) -> crate::error::AppResult<Vec<Vec<f32>>> {
        Ok(texts.iter().map(|_| vec![0.0f32; 1024]).collect())
    }

    #[test]
    fn wave1_migration_runs_on_empty_db() {
        let db = Db::open_in_memory().expect("open in-memory db");
        // ensure_schema already ran via Db::init. Verify the new tables and
        // chunks columns exist.
        let conn = db.lock();
        let names: Vec<String> = conn
            .prepare("PRAGMA table_info(chunks)")
            .unwrap()
            .query_map([], |r| r.get::<_, String>(1))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        for col in [
            "source_kind",
            "source_url",
            "trust_tier",
            "official",
            "confidence",
            "fetched_at",
            "endorsed",
            "content_lang",
            "content_orig",
        ] {
            assert!(names.iter().any(|n| n == col), "missing column {col}");
        }
        for tbl in [
            "components",
            "faq_pairs",
            "setup_steps",
            "research_events",
            "web_cache",
            "research_budget",
            "kv_meta",
        ] {
            let n: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name = ?",
                    rusqlite::params![tbl],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(n, 1, "missing table {tbl}");
        }
    }

    #[test]
    fn wave1_migration_is_idempotent() {
        let db = Db::open_in_memory().unwrap();
        // Re-running ensure_schema must not error and must not duplicate
        // columns or tables. Run it 3 more times for good measure.
        {
            let conn = db.lock();
            for _ in 0..3 {
                migrations_wave1::ensure_schema(&conn).unwrap();
            }
        }

        // Insert a chunk via the legacy path; should still work and pick up
        // default provenance.
        let g = games::insert_game(&db, "Idem", None, None).unwrap();
        let p = pages::insert_page(&db, &g, 1, "/tmp/p.png", None).unwrap();
        let cid = chunks::insert_chunk_with_embedding(
            &db,
            &p,
            &g,
            None,
            "hello",
            1,
            &synthetic_embedding(0.1),
        )
        .unwrap();
        let conn = db.lock();
        let (sk, tier, official): (String, String, i64) = conn
            .query_row(
                "SELECT source_kind, trust_tier, official FROM chunks WHERE id = ?",
                rusqlite::params![cid],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .unwrap();
        assert_eq!(sk, "photo_ocr");
        assert_eq!(tier, "publisher");
        assert_eq!(official, 1);
    }

    #[test]
    fn wave1_backfill_no_op_when_external_refs_empty() {
        let db = Db::open_in_memory().unwrap();
        let n = migrations_wave1::run_external_refs_backfill(&db, synthetic_embed_fn).unwrap();
        assert_eq!(n, 0);
        // Second call must also no-op (already marked done).
        let n2 = migrations_wave1::run_external_refs_backfill(&db, synthetic_embed_fn).unwrap();
        assert_eq!(n2, 0);
    }

    #[test]
    fn wave1_backfill_copies_external_refs_with_provenance() {
        let db = Db::open_in_memory().unwrap();
        let g = games::insert_game(&db, "Backfilled", None, None).unwrap();

        // Two refs: one description (publisher), one forum thread (community).
        external_refs::upsert(
            &db,
            &g,
            &external_refs::NewExternalRef {
                source: "bgg",
                kind: "description",
                ext_id: Some("desc-1"),
                title: Some("Game description"),
                content: "# Overview\n\n这是游戏的官方简介。\n\n包含两段落。",
                url: Some("https://boardgamegeek.com/boardgame/123"),
            },
        )
        .unwrap();
        external_refs::upsert(
            &db,
            &g,
            &external_refs::NewExternalRef {
                source: "bgg",
                kind: "forum_thread",
                ext_id: Some("thread-9"),
                title: Some("Rules question"),
                content: "# 玩家提问\n\n规则不清楚怎么办？",
                url: Some("https://boardgamegeek.com/thread/9"),
            },
        )
        .unwrap();

        let inserted =
            migrations_wave1::run_external_refs_backfill(&db, synthetic_embed_fn).unwrap();
        assert!(inserted >= 2, "expected at least 2 chunks, got {inserted}");

        // Inspect the provenance distribution.
        let counts = chunks::count_chunks_by_source_kind(&db, &g).unwrap();
        let map: std::collections::HashMap<String, u64> = counts.into_iter().collect();
        assert!(
            map.get("bgg_description").copied().unwrap_or(0) >= 1,
            "missing bgg_description chunks: {:?}",
            map
        );
        assert!(
            map.get("bgg_forum").copied().unwrap_or(0) >= 1,
            "missing bgg_forum chunks: {:?}",
            map
        );

        // Verify trust_tier + official derived from kind.
        let conn = db.lock();
        let (tier_desc, off_desc): (String, i64) = conn
            .query_row(
                "SELECT trust_tier, official FROM chunks \
                 WHERE game_id = ? AND source_kind = 'bgg_description' LIMIT 1",
                rusqlite::params![&g],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(tier_desc, "publisher");
        assert_eq!(off_desc, 1);
        let (tier_fm, off_fm): (String, i64) = conn
            .query_row(
                "SELECT trust_tier, official FROM chunks \
                 WHERE game_id = ? AND source_kind = 'bgg_forum' LIMIT 1",
                rusqlite::params![&g],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(tier_fm, "community");
        assert_eq!(off_fm, 0);
        drop(conn);

        // Idempotency: running backfill twice doesn't double the rows.
        let again =
            migrations_wave1::run_external_refs_backfill(&db, synthetic_embed_fn).unwrap();
        assert_eq!(again, 0, "second run should be no-op");
        let counts2 = chunks::count_chunks_by_source_kind(&db, &g).unwrap();
        let total2: u64 = counts2.iter().map(|(_, n)| n).sum();
        assert_eq!(total2 as usize, inserted);
    }

    #[test]
    fn count_chunks_by_source_kind_distribution() {
        let db = Db::open_in_memory().unwrap();
        let g = games::insert_game(&db, "Counts", None, None).unwrap();
        let p = pages::insert_page(&db, &g, 1, "/tmp/p.png", None).unwrap();

        // photo_ocr (default) x2
        chunks::insert_chunk_with_embedding(
            &db,
            &p,
            &g,
            None,
            "a",
            1,
            &synthetic_embedding(0.1),
        )
        .unwrap();
        chunks::insert_chunk_with_embedding(
            &db,
            &p,
            &g,
            None,
            "b",
            1,
            &synthetic_embedding(0.1),
        )
        .unwrap();

        // bgg_forum x1
        chunks::insert_chunk_with_embedding_and_provenance(
            &db,
            &p,
            &g,
            None,
            "c",
            1,
            &synthetic_embedding(0.1),
            &chunks::ChunkProvenance {
                source_kind: "bgg_forum",
                source_url: Some("https://example.com/t"),
                trust_tier: "community",
                official: false,
                confidence: 0.9,
                fetched_at: Some(1700000000),
                content_lang: "zh",
                content_orig: None,
            },
        )
        .unwrap();

        let counts = chunks::count_chunks_by_source_kind(&db, &g).unwrap();
        let map: std::collections::HashMap<String, u64> = counts.into_iter().collect();
        assert_eq!(map.get("photo_ocr").copied(), Some(2));
        assert_eq!(map.get("bgg_forum").copied(), Some(1));
    }

    #[test]
    fn update_chunk_endorsed_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let g = games::insert_game(&db, "E", None, None).unwrap();
        let p = pages::insert_page(&db, &g, 1, "/tmp/p.png", None).unwrap();
        let cid = chunks::insert_chunk_with_embedding(
            &db,
            &p,
            &g,
            None,
            "x",
            1,
            &synthetic_embedding(0.1),
        )
        .unwrap();

        chunks::update_chunk_endorsed(&db, cid, Some(true)).unwrap();
        let conn = db.lock();
        let v: Option<i64> = conn
            .query_row(
                "SELECT endorsed FROM chunks WHERE id = ?",
                rusqlite::params![cid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v, Some(1));
        drop(conn);

        chunks::update_chunk_endorsed(&db, cid, Some(false)).unwrap();
        let conn = db.lock();
        let v: Option<i64> = conn
            .query_row(
                "SELECT endorsed FROM chunks WHERE id = ?",
                rusqlite::params![cid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v, Some(0));
        drop(conn);

        chunks::update_chunk_endorsed(&db, cid, None).unwrap();
        let conn = db.lock();
        let v: Option<i64> = conn
            .query_row(
                "SELECT endorsed FROM chunks WHERE id = ?",
                rusqlite::params![cid],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(v, None);
    }

    #[test]
    fn components_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let g = games::insert_game(&db, "Cmp", None, None).unwrap();
        let id = components::insert_component(
            &db,
            &g,
            &components::NewComponent {
                name_zh: "骑士",
                category: Some("单位"),
                effect_zh: Some("攻击力2"),
                source_kind: "extracted_component",
                source_url: None,
                page_id: None,
                bbox_json: None,
                illustration_id: None,
                trust_tier: "publisher",
                confidence: 0.95,
            },
        )
        .unwrap();
        let listed = components::list_components_for_game(&db, &g).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, id);
        assert_eq!(listed[0].name_zh, "骑士");
        components::clear_components_for_game(&db, &g).unwrap();
        assert!(components::list_components_for_game(&db, &g).unwrap().is_empty());
    }

    #[test]
    fn faq_pairs_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let g = games::insert_game(&db, "Faq", None, None).unwrap();
        faq_pairs::insert_faq_pair(
            &db,
            &g,
            &faq_pairs::NewFaqPair {
                question_zh: "骑士能否远程攻击?",
                answer_zh: "不能。",
                source_kind: "bgg_forum",
                source_url: Some("https://bgg/t"),
                trust_tier: "community",
                official: false,
                confidence: 0.8,
                fetched_at: Some(1700000000),
            },
        )
        .unwrap();
        let listed = faq_pairs::list_faqs_for_game(&db, &g).unwrap();
        assert_eq!(listed.len(), 1);
        assert!(!listed[0].official);
        assert_eq!(listed[0].fetched_at, 1700000000);
        faq_pairs::clear_faqs_for_game(&db, &g).unwrap();
        assert!(faq_pairs::list_faqs_for_game(&db, &g).unwrap().is_empty());
    }

    #[test]
    fn setup_steps_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let g = games::insert_game(&db, "Setup", None, None).unwrap();
        setup_steps::insert_setup_step(
            &db,
            &g,
            &setup_steps::NewSetupStep {
                step_no: 2,
                player_count: Some("3-4"),
                text_zh: "拿出地图",
                component_ids: Some("[1,2]"),
                source_kind: "extracted_setup",
                source_url: None,
                page_id: None,
                trust_tier: "publisher",
                confidence: 0.9,
            },
        )
        .unwrap();
        setup_steps::insert_setup_step(
            &db,
            &g,
            &setup_steps::NewSetupStep {
                step_no: 1,
                player_count: None,
                text_zh: "洗牌",
                component_ids: None,
                source_kind: "extracted_setup",
                source_url: None,
                page_id: None,
                trust_tier: "publisher",
                confidence: 0.9,
            },
        )
        .unwrap();
        let listed = setup_steps::list_setup_steps_for_game(&db, &g).unwrap();
        assert_eq!(listed.len(), 2);
        // Ordered by step_no
        assert_eq!(listed[0].step_no, 1);
        assert_eq!(listed[1].step_no, 2);
        setup_steps::clear_setup_steps_for_game(&db, &g).unwrap();
        assert!(setup_steps::list_setup_steps_for_game(&db, &g)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn research_event_and_web_cache_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let g = games::insert_game(&db, "Res", None, None).unwrap();
        research::record_research_event(
            &db,
            &g,
            &research::NewResearchEvent {
                trigger: "explicit",
                query: "骑士射程",
                query_normalized: "骑士射程",
                hits_json: "[]",
                chunks_added: 0,
                cost_estimate: Some(0.01),
            },
        )
        .unwrap();

        // web_cache upsert
        research::put_web_cache(
            &db,
            "https://example.com/a",
            Some(200),
            Some("md"),
            Some("zh"),
            None,
            1700000000 + 7 * 86400,
        )
        .unwrap();
        let entry = research::get_web_cache(&db, "https://example.com/a")
            .unwrap()
            .expect("cache entry should exist");
        assert_eq!(entry.status, Some(200));
        assert_eq!(entry.content_zh.as_deref(), Some("zh"));

        // Update existing cache
        research::put_web_cache(
            &db,
            "https://example.com/a",
            Some(200),
            Some("md2"),
            Some("zh2"),
            Some("etag"),
            1700000000 + 7 * 86400,
        )
        .unwrap();
        let entry2 = research::get_web_cache(&db, "https://example.com/a")
            .unwrap()
            .unwrap();
        assert_eq!(entry2.content_md.as_deref(), Some("md2"));
        assert_eq!(entry2.etag.as_deref(), Some("etag"));
    }

    #[test]
    fn research_budget_increment_and_cap() {
        let db = Db::open_in_memory().unwrap();
        let g = games::insert_game(&db, "Bud", None, None).unwrap();
        assert_eq!(research::current_budget(&db, &g).unwrap(), 0);

        // Burn through the cap.
        for i in 1..=research::RESEARCH_DAILY_CAP {
            let v = research::increment_budget(&db, &g).unwrap();
            assert_eq!(v, i);
        }
        assert_eq!(
            research::current_budget(&db, &g).unwrap(),
            research::RESEARCH_DAILY_CAP
        );
        // One more should error.
        let res = research::increment_budget(&db, &g);
        assert!(res.is_err(), "expected cap error, got {res:?}");
    }
}
