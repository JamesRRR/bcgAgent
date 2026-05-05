use rusqlite::params;

use super::db::Db;
use crate::error::AppResult;

pub fn get(db: &Db, key: &str) -> AppResult<Option<String>> {
    let conn = db.lock();
    let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?")?;
    let mut rows = stmt.query(params![key])?;
    if let Some(row) = rows.next()? {
        Ok(Some(row.get(0)?))
    } else {
        Ok(None)
    }
}

pub fn set(db: &Db, key: &str, value: &str) -> AppResult<()> {
    let conn = db.lock();
    conn.execute(
        "INSERT INTO settings (key, value) VALUES (?, ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}
