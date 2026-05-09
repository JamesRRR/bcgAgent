//! Research-pipeline storage: events, web fetch cache, daily budget.
//!
//! - `record_research_event` logs a single ask-time research pass for audit
//!   and de-duplication (`(game_id, query_normalized)`).
//! - `web_cache` stores fetched URLs (raw markdown + Chinese translation).
//!   TTL is enforced by callers via `expires_at`.
//! - `research_budget` caps how many events a game may fire per UTC day
//!   (default `RESEARCH_DAILY_CAP`). `increment_budget` returns an error when
//!   the cap is exceeded so callers can short-circuit before paying for a hit.

use rusqlite::params;
use serde::Serialize;
use time::OffsetDateTime;

use super::db::Db;
use crate::error::{AppError, AppResult};

/// Per-game daily ceiling on research events. Mirrors the value documented
/// in the design spec; UI may override per-user later.
pub const RESEARCH_DAILY_CAP: u32 = 20;

#[derive(Debug, Clone, Serialize)]
pub struct ResearchEvent {
    pub id: i64,
    pub game_id: String,
    pub trigger: String,
    pub query: String,
    pub query_normalized: String,
    pub hits_json: String,
    pub chunks_added: i64,
    pub cost_estimate: Option<f64>,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct NewResearchEvent<'a> {
    pub trigger: &'a str,
    pub query: &'a str,
    pub query_normalized: &'a str,
    pub hits_json: &'a str,
    pub chunks_added: i64,
    pub cost_estimate: Option<f64>,
}

pub fn record_research_event(
    db: &Db,
    game_id: &str,
    e: &NewResearchEvent<'_>,
) -> AppResult<i64> {
    let conn = db.lock();
    conn.execute(
        "INSERT INTO research_events \
         (game_id, trigger, query, query_normalized, hits_json, chunks_added, cost_estimate, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            game_id,
            e.trigger,
            e.query,
            e.query_normalized,
            e.hits_json,
            e.chunks_added,
            e.cost_estimate,
            OffsetDateTime::now_utc().unix_timestamp(),
        ],
    )?;
    Ok(conn.last_insert_rowid())
}

#[derive(Debug, Clone, Serialize)]
pub struct WebCacheEntry {
    pub url: String,
    pub status: Option<i64>,
    pub fetched_at: i64,
    pub content_md: Option<String>,
    pub content_zh: Option<String>,
    pub etag: Option<String>,
    pub expires_at: i64,
}

pub fn get_web_cache(db: &Db, url: &str) -> AppResult<Option<WebCacheEntry>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT url, status, fetched_at, content_md, content_zh, etag, expires_at \
         FROM web_cache WHERE url = ?",
    )?;
    let mut rows = stmt.query(params![url])?;
    if let Some(row) = rows.next()? {
        Ok(Some(WebCacheEntry {
            url: row.get(0)?,
            status: row.get(1)?,
            fetched_at: row.get(2)?,
            content_md: row.get(3)?,
            content_zh: row.get(4)?,
            etag: row.get(5)?,
            expires_at: row.get(6)?,
        }))
    } else {
        Ok(None)
    }
}

pub fn put_web_cache(
    db: &Db,
    url: &str,
    status: Option<i64>,
    content_md: Option<&str>,
    content_zh: Option<&str>,
    etag: Option<&str>,
    expires_at: i64,
) -> AppResult<()> {
    let conn = db.lock();
    conn.execute(
        "INSERT INTO web_cache (url, status, fetched_at, content_md, content_zh, etag, expires_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(url) DO UPDATE SET \
           status = excluded.status, \
           fetched_at = excluded.fetched_at, \
           content_md = excluded.content_md, \
           content_zh = excluded.content_zh, \
           etag = excluded.etag, \
           expires_at = excluded.expires_at",
        params![
            url,
            status,
            OffsetDateTime::now_utc().unix_timestamp(),
            content_md,
            content_zh,
            etag,
            expires_at,
        ],
    )?;
    Ok(())
}

fn today_utc() -> String {
    let now = OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}",
        now.year(),
        u8::from(now.month()),
        now.day()
    )
}

/// Read the budget counter for `game_id` on today's UTC date. Returns 0 if
/// no row exists yet.
pub fn current_budget(db: &Db, game_id: &str) -> AppResult<u32> {
    let date = today_utc();
    let conn = db.lock();
    let n: Option<i64> = conn
        .query_row(
            "SELECT events FROM research_budget WHERE game_id = ? AND date_utc = ?",
            params![game_id, date],
            |row| row.get(0),
        )
        .ok();
    Ok(n.unwrap_or(0).max(0) as u32)
}

/// Atomically bump today's counter and return the post-increment value.
/// Errors with `AppError::Other` if the cap would be exceeded — caller is
/// expected to abort the research pass when this happens.
///
/// Reads the user-configurable cap from `settings.kb.research_daily_cap` if
/// set, falling back to `RESEARCH_DAILY_CAP`. The setting is read at call
/// time so a UI change takes effect on the next research event without a
/// process restart.
pub fn increment_budget(db: &Db, game_id: &str) -> AppResult<u32> {
    let cap =
        super::settings::get_u32(db, super::settings::KB_RESEARCH_DAILY_CAP, RESEARCH_DAILY_CAP);
    increment_budget_with_cap(db, game_id, cap)
}

/// Same as `increment_budget` but with an explicit cap. Useful for tests
/// and for callers that already resolved the user override.
pub fn increment_budget_with_cap(db: &Db, game_id: &str, cap: u32) -> AppResult<u32> {
    let date = today_utc();
    let conn = db.lock();
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO research_budget (game_id, date_utc, events) VALUES (?, ?, 0) \
         ON CONFLICT(game_id, date_utc) DO NOTHING",
        params![game_id, date],
    )?;
    let current: i64 = tx.query_row(
        "SELECT events FROM research_budget WHERE game_id = ? AND date_utc = ?",
        params![game_id, date],
        |row| row.get(0),
    )?;
    if (current as u32) >= cap {
        return Err(AppError::Other(anyhow::anyhow!(
            "research daily cap reached for game {game_id} ({current}/{cap})"
        )));
    }
    tx.execute(
        "UPDATE research_budget SET events = events + 1 \
         WHERE game_id = ? AND date_utc = ?",
        params![game_id, date],
    )?;
    let new_val: i64 = tx.query_row(
        "SELECT events FROM research_budget WHERE game_id = ? AND date_utc = ?",
        params![game_id, date],
        |row| row.get(0),
    )?;
    tx.commit()?;
    Ok(new_val as u32)
}
