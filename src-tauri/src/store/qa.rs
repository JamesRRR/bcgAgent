use rusqlite::params;
use time::OffsetDateTime;
use uuid::Uuid;

use super::db::Db;
use super::models::QAHistory;
use crate::error::AppResult;

fn now_secs() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

/// Insert a Q&A record. `retrieved_chunk_ids` is a JSON-encoded array of
/// chunk ids produced by the caller (e.g. `serde_json::to_string(&ids)`).
pub fn insert_qa(
    db: &Db,
    game_id: Option<&str>,
    question: &str,
    answer: Option<&str>,
    audio_path: Option<&str>,
    retrieved_chunk_ids: Option<&str>,
) -> AppResult<String> {
    let id = Uuid::new_v4().to_string();
    let conn = db.lock();
    conn.execute(
        "INSERT INTO qa_history \
            (id, game_id, question, answer, audio_path, retrieved_chunk_ids, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?)",
        params![
            id,
            game_id,
            question,
            answer,
            audio_path,
            retrieved_chunk_ids,
            now_secs()
        ],
    )?;
    Ok(id)
}

/// List most-recent-first. Pass `game_id = None` for all games.
pub fn list_qa(db: &Db, game_id: Option<&str>, limit: usize) -> AppResult<Vec<QAHistory>> {
    let conn = db.lock();
    let mapper = |row: &rusqlite::Row<'_>| {
        Ok(QAHistory {
            id: row.get(0)?,
            game_id: row.get(1)?,
            question: row.get(2)?,
            answer: row.get(3)?,
            audio_path: row.get(4)?,
            retrieved_chunk_ids: row.get(5)?,
            created_at: row.get(6)?,
        })
    };

    match game_id {
        None => {
            let mut stmt = conn.prepare(
                "SELECT id, game_id, question, answer, audio_path, retrieved_chunk_ids, created_at \
                 FROM qa_history ORDER BY created_at DESC LIMIT ?",
            )?;
            let rows = stmt
                .query_map(params![limit as i64], mapper)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        }
        Some(gid) => {
            let mut stmt = conn.prepare(
                "SELECT id, game_id, question, answer, audio_path, retrieved_chunk_ids, created_at \
                 FROM qa_history WHERE game_id = ? ORDER BY created_at DESC LIMIT ?",
            )?;
            let rows = stmt
                .query_map(params![gid, limit as i64], mapper)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(rows)
        }
    }
}
