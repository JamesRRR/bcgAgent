//! Q&A pairs harvested from BGG forum threads (and later, publisher FAQs).
//!
//! Same idempotency convention as `components`: the extractor wipes the
//! game's rows before re-emitting.

use rusqlite::params;
use serde::Serialize;
use time::OffsetDateTime;

use super::db::Db;
use crate::error::AppResult;

#[derive(Debug, Clone, Serialize)]
pub struct FaqPair {
    pub id: i64,
    pub game_id: String,
    pub question_zh: String,
    pub answer_zh: String,
    pub source_kind: String,
    pub source_url: Option<String>,
    pub trust_tier: String,
    pub official: bool,
    pub confidence: f64,
    pub fetched_at: i64,
}

#[derive(Debug, Clone)]
pub struct NewFaqPair<'a> {
    pub question_zh: &'a str,
    pub answer_zh: &'a str,
    pub source_kind: &'a str,
    pub source_url: Option<&'a str>,
    pub trust_tier: &'a str,
    pub official: bool,
    pub confidence: f64,
    pub fetched_at: Option<i64>,
}

pub fn insert_faq_pair(db: &Db, game_id: &str, f: &NewFaqPair<'_>) -> AppResult<i64> {
    let fetched_at = f
        .fetched_at
        .unwrap_or_else(|| OffsetDateTime::now_utc().unix_timestamp());
    let conn = db.lock();
    conn.execute(
        "INSERT INTO faq_pairs \
         (game_id, question_zh, answer_zh, source_kind, source_url, \
          trust_tier, official, confidence, fetched_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            game_id,
            f.question_zh,
            f.answer_zh,
            f.source_kind,
            f.source_url,
            f.trust_tier,
            if f.official { 1i64 } else { 0i64 },
            f.confidence,
            fetched_at,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_faqs_for_game(db: &Db, game_id: &str) -> AppResult<Vec<FaqPair>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT id, game_id, question_zh, answer_zh, source_kind, source_url, \
                trust_tier, official, confidence, fetched_at \
         FROM faq_pairs WHERE game_id = ? ORDER BY id ASC",
    )?;
    let rows = stmt
        .query_map(params![game_id], |row| {
            let official_int: i64 = row.get(7)?;
            Ok(FaqPair {
                id: row.get(0)?,
                game_id: row.get(1)?,
                question_zh: row.get(2)?,
                answer_zh: row.get(3)?,
                source_kind: row.get(4)?,
                source_url: row.get(5)?,
                trust_tier: row.get(6)?,
                official: official_int != 0,
                confidence: row.get(8)?,
                fetched_at: row.get(9)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn clear_faqs_for_game(db: &Db, game_id: &str) -> AppResult<()> {
    let conn = db.lock();
    conn.execute("DELETE FROM faq_pairs WHERE game_id = ?", params![game_id])?;
    Ok(())
}
