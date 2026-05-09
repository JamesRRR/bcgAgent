//! Persistence for the conversational walkthrough.
//!
//! Schema lives in `migrations/0001_init.sql`. A session is one playthrough's
//! live coaching dialogue with the LLM; turns are agent/user messages within
//! that session.

use rusqlite::params;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use super::db::Db;
use crate::error::AppResult;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub session_id: String,
    pub game_id: String,
    pub phase: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Turn {
    pub turn_no: i64,
    pub role: String, // "agent" | "user"
    pub kind: String, // "instruction" | "question" | "confirm" | "answer" | "greeting" | "summary"
    pub content: String,
    pub created_at: i64,
}

fn now_secs() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

/// Find the most recent session for this game. Returns `None` if none exists.
pub fn latest_for_game(db: &Db, game_id: &str) -> AppResult<Option<Session>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT session_id, game_id, phase, created_at, updated_at \
         FROM walkthrough_sessions \
         WHERE game_id = ? \
         ORDER BY updated_at DESC \
         LIMIT 1",
    )?;
    let mut rows = stmt.query(params![game_id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(Session {
            session_id: row.get(0)?,
            game_id: row.get(1)?,
            phase: row.get(2)?,
            created_at: row.get(3)?,
            updated_at: row.get(4)?,
        }))
    } else {
        Ok(None)
    }
}

pub fn create(db: &Db, game_id: &str) -> AppResult<Session> {
    let session_id = Uuid::new_v4().to_string();
    let ts = now_secs();
    {
        let conn = db.lock();
        conn.execute(
            "INSERT INTO walkthrough_sessions \
                (session_id, game_id, phase, created_at, updated_at) \
             VALUES (?, ?, 'setup', ?, ?)",
            params![session_id, game_id, ts, ts],
        )?;
    }
    Ok(Session {
        session_id,
        game_id: game_id.to_string(),
        phase: "setup".into(),
        created_at: ts,
        updated_at: ts,
    })
}

/// Drop a session and its turns. Used by `walkthrough_session_reset`.
pub fn delete_for_game(db: &Db, game_id: &str) -> AppResult<()> {
    let conn = db.lock();
    conn.execute(
        "DELETE FROM walkthrough_sessions WHERE game_id = ?",
        params![game_id],
    )?;
    Ok(())
}

pub fn turns(db: &Db, session_id: &str) -> AppResult<Vec<Turn>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT turn_no, role, kind, content, created_at \
         FROM walkthrough_turns \
         WHERE session_id = ? \
         ORDER BY turn_no ASC",
    )?;
    let rows = stmt
        .query_map(params![session_id], |row| {
            Ok(Turn {
                turn_no: row.get(0)?,
                role: row.get(1)?,
                kind: row.get(2)?,
                content: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn next_turn_no(db: &Db, session_id: &str) -> AppResult<i64> {
    let conn = db.lock();
    let n: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(turn_no) + 1, 0) \
             FROM walkthrough_turns \
             WHERE session_id = ?",
            params![session_id],
            |row| row.get(0),
        )
        .unwrap_or(0);
    Ok(n)
}

pub fn append_turn(
    db: &Db,
    session_id: &str,
    turn_no: i64,
    role: &str,
    kind: &str,
    content: &str,
) -> AppResult<()> {
    let ts = now_secs();
    let conn = db.lock();
    conn.execute(
        "INSERT INTO walkthrough_turns \
            (session_id, turn_no, role, kind, content, created_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
        params![session_id, turn_no, role, kind, content, ts],
    )?;
    conn.execute(
        "UPDATE walkthrough_sessions SET updated_at = ? WHERE session_id = ?",
        params![ts, session_id],
    )?;
    Ok(())
}

pub fn set_phase(db: &Db, session_id: &str, phase: &str) -> AppResult<()> {
    let conn = db.lock();
    conn.execute(
        "UPDATE walkthrough_sessions SET phase = ?, updated_at = ? WHERE session_id = ?",
        params![phase, now_secs(), session_id],
    )?;
    Ok(())
}
