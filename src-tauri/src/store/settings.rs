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

// ---- Wave 4 typed accessors for KB settings -----------------------------

/// Wave 4 setting keys. Centralised so backend + frontend stay in sync.
pub const KB_AUTO_RESEARCH_ENABLED: &str = "kb.auto_research_enabled";
pub const KB_INCLUDE_UNOFFICIAL: &str = "kb.include_unofficial";
pub const KB_CONFIDENCE_THRESHOLD: &str = "kb.confidence_threshold";
pub const KB_RESEARCH_DAILY_CAP: &str = "kb.research_daily_cap";

fn parse_bool(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Read a bool setting. Falls back to `default` if unset or unparseable.
pub fn get_bool(db: &Db, key: &str, default: bool) -> bool {
    match get(db, key) {
        Ok(Some(v)) => parse_bool(&v).unwrap_or(default),
        _ => default,
    }
}

/// Read an f32 setting. Falls back to `default` if unset or unparseable.
pub fn get_f32(db: &Db, key: &str, default: f32) -> f32 {
    match get(db, key) {
        Ok(Some(v)) => v.trim().parse::<f32>().unwrap_or(default),
        _ => default,
    }
}

/// Read a u32 setting. Falls back to `default` if unset or unparseable.
pub fn get_u32(db: &Db, key: &str, default: u32) -> u32 {
    match get(db, key) {
        Ok(Some(v)) => v.trim().parse::<u32>().unwrap_or(default),
        _ => default,
    }
}
