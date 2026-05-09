use rusqlite::params;
use serde::Serialize;

use super::db::Db;
use crate::error::AppResult;

#[derive(Debug, Clone, Serialize)]
pub struct PageIllustration {
    pub id: String,
    pub page_id: String,
    pub game_id: String,
    pub position: i64,
    pub image_path: String,
    pub bbox_x1: i64,
    pub bbox_y1: i64,
    pub bbox_x2: i64,
    pub bbox_y2: i64,
    pub label: Option<String>,
    /// Opaque marker the OCR'd markdown uses to anchor this illustration
    /// (e.g. "ill:0"). Frontend renderer matches by token instead of by
    /// position so the markdown can place crops between specific paragraphs.
    pub token: Option<String>,
    pub description: Option<String>,
    pub created_at: i64,
}

pub fn set_description(db: &Db, illustration_id: &str, description: &str) -> AppResult<()> {
    let conn = db.lock();
    conn.execute(
        "UPDATE page_illustrations SET description = ? WHERE id = ?",
        params![description, illustration_id],
    )?;
    Ok(())
}

pub fn list_for_game(db: &Db, game_id: &str) -> AppResult<Vec<PageIllustration>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT id, page_id, game_id, position, image_path, \
         bbox_x1, bbox_y1, bbox_x2, bbox_y2, label, token, description, created_at \
         FROM page_illustrations \
         WHERE game_id = ? \
         ORDER BY page_id, position",
    )?;
    let rows: Vec<PageIllustration> = stmt
        .query_map(params![game_id], |row| {
            Ok(PageIllustration {
                id: row.get(0)?,
                page_id: row.get(1)?,
                game_id: row.get(2)?,
                position: row.get(3)?,
                image_path: row.get(4)?,
                bbox_x1: row.get(5)?,
                bbox_y1: row.get(6)?,
                bbox_x2: row.get(7)?,
                bbox_y2: row.get(8)?,
                label: row.get(9)?,
                token: row.get(10)?,
                description: row.get(11)?,
                created_at: row.get(12)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

#[allow(clippy::too_many_arguments)]
pub fn insert(
    db: &Db,
    id: &str,
    page_id: &str,
    game_id: &str,
    position: i64,
    image_path: &str,
    bbox: (u32, u32, u32, u32),
    label: Option<&str>,
    token: Option<&str>,
) -> AppResult<()> {
    let conn = db.lock();
    conn.execute(
        "INSERT INTO page_illustrations \
         (id, page_id, game_id, position, image_path, bbox_x1, bbox_y1, bbox_x2, bbox_y2, label, token, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            id,
            page_id,
            game_id,
            position,
            image_path,
            bbox.0 as i64,
            bbox.1 as i64,
            bbox.2 as i64,
            bbox.3 as i64,
            label,
            token,
            time::OffsetDateTime::now_utc().unix_timestamp(),
        ],
    )?;
    Ok(())
}

pub fn list_by_page(db: &Db, page_id: &str) -> AppResult<Vec<PageIllustration>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT id, page_id, game_id, position, image_path, \
         bbox_x1, bbox_y1, bbox_x2, bbox_y2, label, token, description, created_at \
         FROM page_illustrations \
         WHERE page_id = ? \
         ORDER BY position ASC",
    )?;
    let rows: Vec<PageIllustration> = stmt
        .query_map(params![page_id], |row| {
            Ok(PageIllustration {
                id: row.get(0)?,
                page_id: row.get(1)?,
                game_id: row.get(2)?,
                position: row.get(3)?,
                image_path: row.get(4)?,
                bbox_x1: row.get(5)?,
                bbox_y1: row.get(6)?,
                bbox_x2: row.get(7)?,
                bbox_y2: row.get(8)?,
                label: row.get(9)?,
                token: row.get(10)?,
                description: row.get(11)?,
                created_at: row.get(12)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Wipe all illustrations for a page (used when re-running OCR on a page).
pub fn delete_for_page(db: &Db, page_id: &str) -> AppResult<()> {
    let conn = db.lock();
    conn.execute(
        "DELETE FROM page_illustrations WHERE page_id = ?",
        params![page_id],
    )?;
    Ok(())
}
