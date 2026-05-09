//! Structured `components` rows extracted from rulebook + illustrations.
//!
//! Each row represents one named game component (e.g. 骑士, 龙骑士) with
//! optional category, effect text, page+bbox locator, and provenance. The
//! extractor (Wave 3) is idempotent: it clears the game's rows before
//! re-extracting, so callers should reach for `clear_components_for_game`
//! before bulk inserts.

use rusqlite::params;
use serde::Serialize;
use time::OffsetDateTime;

use super::db::Db;
use crate::error::AppResult;

#[derive(Debug, Clone, Serialize)]
pub struct Component {
    pub id: i64,
    pub game_id: String,
    pub name_zh: String,
    pub category: Option<String>,
    pub effect_zh: Option<String>,
    pub source_kind: String,
    pub source_url: Option<String>,
    pub page_id: Option<String>,
    pub bbox_json: Option<String>,
    pub illustration_id: Option<String>,
    pub trust_tier: String,
    pub confidence: f64,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct NewComponent<'a> {
    pub name_zh: &'a str,
    pub category: Option<&'a str>,
    pub effect_zh: Option<&'a str>,
    pub source_kind: &'a str,
    pub source_url: Option<&'a str>,
    pub page_id: Option<&'a str>,
    pub bbox_json: Option<&'a str>,
    pub illustration_id: Option<&'a str>,
    pub trust_tier: &'a str,
    pub confidence: f64,
}

pub fn insert_component(db: &Db, game_id: &str, c: &NewComponent<'_>) -> AppResult<i64> {
    let conn = db.lock();
    conn.execute(
        "INSERT INTO components \
         (game_id, name_zh, category, effect_zh, source_kind, source_url, \
          page_id, bbox_json, illustration_id, trust_tier, confidence, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            game_id,
            c.name_zh,
            c.category,
            c.effect_zh,
            c.source_kind,
            c.source_url,
            c.page_id,
            c.bbox_json,
            c.illustration_id,
            c.trust_tier,
            c.confidence,
            OffsetDateTime::now_utc().unix_timestamp(),
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_components_for_game(db: &Db, game_id: &str) -> AppResult<Vec<Component>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT id, game_id, name_zh, category, effect_zh, source_kind, source_url, \
                page_id, bbox_json, illustration_id, trust_tier, confidence, created_at \
         FROM components WHERE game_id = ? ORDER BY id ASC",
    )?;
    let rows = stmt
        .query_map(params![game_id], |row| {
            Ok(Component {
                id: row.get(0)?,
                game_id: row.get(1)?,
                name_zh: row.get(2)?,
                category: row.get(3)?,
                effect_zh: row.get(4)?,
                source_kind: row.get(5)?,
                source_url: row.get(6)?,
                page_id: row.get(7)?,
                bbox_json: row.get(8)?,
                illustration_id: row.get(9)?,
                trust_tier: row.get(10)?,
                confidence: row.get(11)?,
                created_at: row.get(12)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn clear_components_for_game(db: &Db, game_id: &str) -> AppResult<()> {
    let conn = db.lock();
    conn.execute("DELETE FROM components WHERE game_id = ?", params![game_id])?;
    Ok(())
}
