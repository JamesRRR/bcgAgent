use rusqlite::params;
use time::OffsetDateTime;
use uuid::Uuid;

use super::db::Db;
use super::models::Page;
use crate::error::AppResult;

fn now_secs() -> i64 {
    OffsetDateTime::now_utc().unix_timestamp()
}

/// Insert a page (status defaults to `pending`) and return its uuid.
pub fn insert_page(
    db: &Db,
    game_id: &str,
    page_number: i64,
    image_path: &str,
    thumb_path: Option<&str>,
) -> AppResult<String> {
    let id = Uuid::new_v4().to_string();
    let conn = db.lock();
    conn.execute(
        "INSERT INTO pages (id, game_id, page_number, image_path, thumb_path, created_at) \
         VALUES (?, ?, ?, ?, ?, ?)",
        params![id, game_id, page_number, image_path, thumb_path, now_secs()],
    )?;
    Ok(id)
}

fn row_to_page(row: &rusqlite::Row<'_>) -> rusqlite::Result<Page> {
    Ok(Page {
        id: row.get(0)?,
        game_id: row.get(1)?,
        page_number: row.get(2)?,
        image_path: row.get(3)?,
        thumb_path: row.get(4)?,
        ocr_status: row.get(5)?,
        ocr_markdown: row.get(6)?,
        ocr_json: row.get(7)?,
        created_at: row.get(8)?,
    })
}

pub fn list_pages_by_game(db: &Db, game_id: &str) -> AppResult<Vec<Page>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT id, game_id, page_number, image_path, thumb_path, ocr_status, \
                ocr_markdown, ocr_json, created_at \
         FROM pages WHERE game_id = ? ORDER BY page_number ASC",
    )?;
    let rows = stmt
        .query_map(params![game_id], row_to_page)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn get_page(db: &Db, id: &str) -> AppResult<Option<Page>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT id, game_id, page_number, image_path, thumb_path, ocr_status, \
                ocr_markdown, ocr_json, created_at \
         FROM pages WHERE id = ?",
    )?;
    let mut rows = stmt.query(params![id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row_to_page(row)?))
    } else {
        Ok(None)
    }
}

/// Mark OCR result. `status` is one of `pending|done|failed`.
pub fn set_ocr_result(
    db: &Db,
    page_id: &str,
    status: &str,
    markdown: Option<&str>,
    json: Option<&str>,
) -> AppResult<()> {
    let conn = db.lock();
    conn.execute(
        "UPDATE pages SET ocr_status = ?, ocr_markdown = ?, ocr_json = ? WHERE id = ?",
        params![status, markdown, json, page_id],
    )?;
    Ok(())
}
