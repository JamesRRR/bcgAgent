//! External knowledge cache (BGG description, forum threads, gallery
//! captions, illustration captions). Populated at import time so the
//! walkthrough coach has rich context without per-question network calls.

use rusqlite::params;
use serde::Serialize;
use time::OffsetDateTime;

use super::db::Db;
use crate::error::AppResult;

#[derive(Debug, Clone, Serialize)]
pub struct ExternalRef {
    pub id: i64,
    pub game_id: String,
    pub source: String,
    pub kind: String,
    pub ext_id: Option<String>,
    pub title: Option<String>,
    pub content: String,
    pub url: Option<String>,
    pub fetched_at: i64,
}

#[derive(Debug, Clone)]
pub struct NewExternalRef<'a> {
    pub source: &'a str,
    pub kind: &'a str,
    pub ext_id: Option<&'a str>,
    pub title: Option<&'a str>,
    pub content: &'a str,
    pub url: Option<&'a str>,
}

pub fn upsert(db: &Db, game_id: &str, r: &NewExternalRef<'_>) -> AppResult<i64> {
    let conn = db.lock();
    conn.execute(
        "INSERT INTO game_external_refs \
         (game_id, source, kind, ext_id, title, content, url, fetched_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?) \
         ON CONFLICT(game_id, source, kind, ext_id) DO UPDATE SET \
           title = excluded.title, \
           content = excluded.content, \
           url = excluded.url, \
           fetched_at = excluded.fetched_at",
        params![
            game_id,
            r.source,
            r.kind,
            r.ext_id,
            r.title,
            r.content,
            r.url,
            OffsetDateTime::now_utc().unix_timestamp(),
        ],
    )?;
    let id = conn.last_insert_rowid();
    Ok(id)
}

pub fn list_for_game(db: &Db, game_id: &str) -> AppResult<Vec<ExternalRef>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT id, game_id, source, kind, ext_id, title, content, url, fetched_at \
         FROM game_external_refs WHERE game_id = ? ORDER BY id ASC",
    )?;
    let rows = stmt
        .query_map(params![game_id], |row| {
            Ok(ExternalRef {
                id: row.get(0)?,
                game_id: row.get(1)?,
                source: row.get(2)?,
                kind: row.get(3)?,
                ext_id: row.get(4)?,
                title: row.get(5)?,
                content: row.get(6)?,
                url: row.get(7)?,
                fetched_at: row.get(8)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn delete_for_game(db: &Db, game_id: &str) -> AppResult<()> {
    let conn = db.lock();
    conn.execute(
        "DELETE FROM game_external_refs WHERE game_id = ?",
        params![game_id],
    )?;
    Ok(())
}

pub fn count_for_game(db: &Db, game_id: &str) -> AppResult<i64> {
    let conn = db.lock();
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM game_external_refs WHERE game_id = ?",
        params![game_id],
        |row| row.get(0),
    )?;
    Ok(n)
}
