//! SQLite storage layer (Wave 1.2).
//!
//! Connection management, schema migrations, and CRUD/search helpers for
//! games, pages, chunks (with `sqlite-vec` 1024-d embeddings + jieba-tokenized
//! FTS5), and Q&A history.

pub mod db;
pub mod jieba;
pub mod models;

pub mod chunks;
pub mod games;
pub mod pages;
pub mod qa;
pub mod settings;

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
            &db, &p1, &game_id, Some("rules"), "low embedding chunk", 4, &low,
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
}
