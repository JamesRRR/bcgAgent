//! CRUD for cached beginner-mode walkthroughs (one row per game).

use rusqlite::params;
use time::OffsetDateTime;

use super::db::Db;
use crate::error::AppResult;

fn now_secs() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

/// Read the cached walkthrough markdown for a game, or `None` if not yet generated.
pub fn get(db: &Db, game_id: &str) -> AppResult<Option<String>> {
    let conn = db.lock();
    let mut stmt = conn.prepare("SELECT content FROM walkthroughs WHERE game_id = ?")?;
    let mut rows = stmt.query(params![game_id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row.get(0)?))
    } else {
        Ok(None)
    }
}

/// Insert or replace the walkthrough for a game.
pub fn upsert(db: &Db, game_id: &str, content: &str) -> AppResult<()> {
    let conn = db.lock();
    conn.execute(
        "INSERT INTO walkthroughs (game_id, content, created_at) \
         VALUES (?, ?, ?) \
         ON CONFLICT(game_id) DO UPDATE SET content = excluded.content, created_at = excluded.created_at",
        params![game_id, content, now_secs()],
    )?;
    Ok(())
}
