use rusqlite::params;
use time::OffsetDateTime;
use uuid::Uuid;

use super::db::Db;
use super::models::Game;
use crate::error::AppResult;

fn now_secs() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

/// Insert a new game and return its uuid.
pub fn insert_game(
    db: &Db,
    name_zh: &str,
    name_en: Option<&str>,
    publisher: Option<&str>,
) -> AppResult<String> {
    let id = Uuid::new_v4().to_string();
    let conn = db.lock();
    conn.execute(
        "INSERT INTO games (id, name_zh, name_en, publisher, page_count, created_at) \
         VALUES (?, ?, ?, ?, 0, ?)",
        params![id, name_zh, name_en, publisher, now_secs()],
    )?;
    Ok(id)
}

pub fn list_games(db: &Db) -> AppResult<Vec<Game>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT id, name_zh, name_en, publisher, cover_path, page_count, created_at \
         FROM games ORDER BY created_at DESC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok(Game {
                id: row.get(0)?,
                name_zh: row.get(1)?,
                name_en: row.get(2)?,
                publisher: row.get(3)?,
                cover_path: row.get(4)?,
                page_count: row.get(5)?,
                created_at: row.get(6)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_game(db: &Db, id: &str) -> AppResult<Option<Game>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT id, name_zh, name_en, publisher, cover_path, page_count, created_at \
         FROM games WHERE id = ?",
    )?;
    let mut rows = stmt.query(params![id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(Game {
            id: row.get(0)?,
            name_zh: row.get(1)?,
            name_en: row.get(2)?,
            publisher: row.get(3)?,
            cover_path: row.get(4)?,
            page_count: row.get(5)?,
            created_at: row.get(6)?,
        }))
    } else {
        Ok(None)
    }
}

pub fn set_cover(db: &Db, game_id: &str, cover_path: &str) -> AppResult<()> {
    let conn = db.lock();
    conn.execute(
        "UPDATE games SET cover_path = ? WHERE id = ?",
        params![cover_path, game_id],
    )?;
    Ok(())
}

pub fn update_name(db: &Db, game_id: &str, name_zh: &str, name_en: Option<&str>) -> AppResult<()> {
    let conn = db.lock();
    conn.execute(
        "UPDATE games SET name_zh = ?, name_en = ? WHERE id = ?",
        params![name_zh, name_en, game_id],
    )?;
    Ok(())
}

pub fn set_bgg_id(db: &Db, game_id: &str, bgg_id: u32) -> AppResult<()> {
    let conn = db.lock();
    conn.execute(
        "UPDATE games SET bgg_id = ? WHERE id = ?",
        params![bgg_id as i64, game_id],
    )?;
    Ok(())
}

pub fn get_bgg_id(db: &Db, game_id: &str) -> AppResult<Option<u32>> {
    let conn = db.lock();
    let v: Option<i64> = conn
        .query_row(
            "SELECT bgg_id FROM games WHERE id = ?",
            params![game_id],
            |row| row.get(0),
        )
        .ok()
        .flatten();
    Ok(v.map(|n| n as u32))
}

pub fn increment_page_count(db: &Db, game_id: &str) -> AppResult<()> {
    let conn = db.lock();
    conn.execute(
        "UPDATE games SET page_count = page_count + 1 WHERE id = ?",
        params![game_id],
    )?;
    Ok(())
}

/// Hard-delete a game and all dependent rows. ON DELETE CASCADE handles
/// pages/chunks/illustrations/walkthroughs, but the FTS5/vec0 virtual
/// tables aren't FK-bound — we wipe their rows here based on the chunk
/// ids that *would* be cascaded, BEFORE issuing the DELETE on games.
pub fn delete_game(db: &Db, game_id: &str) -> AppResult<()> {
    let conn = db.lock();
    let tx = conn.unchecked_transaction()?;
    // Collect chunk ids for this game so we can wipe their FTS/vec rows.
    let mut stmt = tx.prepare("SELECT id FROM chunks WHERE game_id = ?")?;
    let ids: Vec<i64> = stmt
        .query_map(params![game_id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;
    drop(stmt);
    for cid in &ids {
        // Best-effort: virtual tables may be empty if indexing crashed midway.
        let _ = tx.execute("DELETE FROM chunks_fts WHERE rowid = ?", params![cid]);
        let _ = tx.execute("DELETE FROM chunks_vec WHERE rowid = ?", params![cid]);
    }
    // Cascade: pages → chunks/illustrations, sessions → turns, walkthroughs.
    tx.execute("DELETE FROM games WHERE id = ?", params![game_id])?;
    tx.commit()?;
    Ok(())
}
