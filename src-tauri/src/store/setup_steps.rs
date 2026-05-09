//! Ordered setup steps extracted from rulebook setup sections.
//!
//! `step_no` is monotonic per game (the extractor preserves rulebook order).
//! `player_count` is a free-form filter like "2", "3-4", or NULL for "all".
//! `component_ids` is a JSON array of component-row ids referenced by the
//! step, written as a TEXT column for portability.

use rusqlite::params;
use serde::Serialize;

use super::db::Db;
use crate::error::AppResult;

#[derive(Debug, Clone, Serialize)]
pub struct SetupStep {
    pub id: i64,
    pub game_id: String,
    pub step_no: i64,
    pub player_count: Option<String>,
    pub text_zh: String,
    pub component_ids: Option<String>,
    pub source_kind: String,
    pub source_url: Option<String>,
    pub page_id: Option<String>,
    pub trust_tier: String,
    pub confidence: f64,
}

#[derive(Debug, Clone)]
pub struct NewSetupStep<'a> {
    pub step_no: i64,
    pub player_count: Option<&'a str>,
    pub text_zh: &'a str,
    pub component_ids: Option<&'a str>,
    pub source_kind: &'a str,
    pub source_url: Option<&'a str>,
    pub page_id: Option<&'a str>,
    pub trust_tier: &'a str,
    pub confidence: f64,
}

pub fn insert_setup_step(db: &Db, game_id: &str, s: &NewSetupStep<'_>) -> AppResult<i64> {
    let conn = db.lock();
    conn.execute(
        "INSERT INTO setup_steps \
         (game_id, step_no, player_count, text_zh, component_ids, \
          source_kind, source_url, page_id, trust_tier, confidence) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            game_id,
            s.step_no,
            s.player_count,
            s.text_zh,
            s.component_ids,
            s.source_kind,
            s.source_url,
            s.page_id,
            s.trust_tier,
            s.confidence,
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn list_setup_steps_for_game(db: &Db, game_id: &str) -> AppResult<Vec<SetupStep>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT id, game_id, step_no, player_count, text_zh, component_ids, \
                source_kind, source_url, page_id, trust_tier, confidence \
         FROM setup_steps WHERE game_id = ? ORDER BY step_no ASC, id ASC",
    )?;
    let rows = stmt
        .query_map(params![game_id], |row| {
            Ok(SetupStep {
                id: row.get(0)?,
                game_id: row.get(1)?,
                step_no: row.get(2)?,
                player_count: row.get(3)?,
                text_zh: row.get(4)?,
                component_ids: row.get(5)?,
                source_kind: row.get(6)?,
                source_url: row.get(7)?,
                page_id: row.get(8)?,
                trust_tier: row.get(9)?,
                confidence: row.get(10)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn clear_setup_steps_for_game(db: &Db, game_id: &str) -> AppResult<()> {
    let conn = db.lock();
    conn.execute(
        "DELETE FROM setup_steps WHERE game_id = ?",
        params![game_id],
    )?;
    Ok(())
}
