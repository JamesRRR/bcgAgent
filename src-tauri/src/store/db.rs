use std::sync::Arc;
use std::sync::Once;

use parking_lot::Mutex;
use rusqlite::Connection;

use crate::error::AppResult;
use crate::paths;

const MIGRATIONS: &str = include_str!("migrations/0001_init.sql");

static REGISTER_VEC: Once = Once::new();

/// Register the sqlite-vec extension as a SQLite auto-extension. This is a
/// process-wide one-shot — every `Connection` opened afterwards will have
/// `vec0` virtual tables, `vec_distance_l2`, etc. available.
fn register_vec_extension() {
    REGISTER_VEC.call_once(|| unsafe {
        rusqlite::ffi::sqlite3_auto_extension(Some(std::mem::transmute(
            sqlite_vec::sqlite3_vec_init as *const (),
        )));
    });
}

/// Thread-safe wrapper around a single rusqlite `Connection`. SQLite handles
/// concurrent reads but we serialize all access via a `parking_lot::Mutex`.
#[derive(Clone)]
pub struct Db {
    pub(crate) conn: Arc<Mutex<Connection>>,
}

impl Db {
    /// Open the on-disk database, creating directories as needed and running
    /// migrations.
    pub fn open() -> AppResult<Self> {
        paths::ensure_layout()?;
        register_vec_extension();
        let conn = Connection::open(paths::db_path())?;
        Self::init(conn)
    }

    /// In-memory database for tests.
    pub fn open_in_memory() -> AppResult<Self> {
        register_vec_extension();
        let conn = Connection::open_in_memory()?;
        Self::init(conn)
    }

    fn init(conn: Connection) -> AppResult<Self> {
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(MIGRATIONS)?;
        // Defensive ALTERs for new columns added after first launch. SQLite
        // has no IF NOT EXISTS for columns, so we just swallow the duplicate
        // column error. Keep this list short — full schema changes go in
        // migrations/.
        for sql in &[
            "ALTER TABLE page_illustrations ADD COLUMN token TEXT",
            "ALTER TABLE page_illustrations ADD COLUMN description TEXT",
            "ALTER TABLE games ADD COLUMN bgg_id INTEGER",
        ] {
            let _ = conn.execute(sql, []);
        }
        ensure_external_refs_table(&conn)?;
        retokenize_fts_if_outdated(&conn)?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Acquire the connection lock. All other modules call this per-operation.
    pub(crate) fn lock(&self) -> parking_lot::MutexGuard<'_, Connection> {
        self.conn.lock()
    }
}

/// Create the `game_external_refs` table on first run if missing. Stores
/// pre-fetched knowledge from BGG (description, forum threads, gallery
/// captions) and Qwen-VL illustration captions. Each row is idempotently
/// keyed by (game_id, source, kind, ext_id) so re-imports overwrite cleanly.
fn ensure_external_refs_table(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS game_external_refs (
             id          INTEGER PRIMARY KEY AUTOINCREMENT,
             game_id     TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
             source      TEXT NOT NULL,
             kind        TEXT NOT NULL,
             ext_id      TEXT,
             title       TEXT,
             content     TEXT NOT NULL,
             url         TEXT,
             fetched_at  INTEGER NOT NULL,
             UNIQUE (game_id, source, kind, ext_id)
         );
         CREATE INDEX IF NOT EXISTS idx_external_refs_game
             ON game_external_refs(game_id);",
    )?;
    Ok(())
}

/// Bump this whenever the indexing-side jieba behavior changes. On startup we
/// compare against the value persisted in `settings`; if they differ, we
/// rebuild every row of `chunks_fts` from the canonical `chunks.content`.
const FTS_INDEX_VERSION: &str = "v2-search-mode";

fn retokenize_fts_if_outdated(conn: &Connection) -> AppResult<()> {
    use rusqlite::params;
    let stored: Option<String> = conn
        .query_row(
            "SELECT value FROM settings WHERE key = 'fts_index_version'",
            [],
            |row| row.get(0),
        )
        .ok();
    if stored.as_deref() == Some(FTS_INDEX_VERSION) {
        return Ok(());
    }
    let chunk_count: i64 = conn.query_row("SELECT COUNT(*) FROM chunks", [], |row| row.get(0))?;
    if chunk_count > 0 {
        tracing::info!(
            "rebuilding chunks_fts ({} rows) for tokenizer={}",
            chunk_count,
            FTS_INDEX_VERSION
        );
        conn.execute("DELETE FROM chunks_fts", [])?;
        let mut stmt = conn.prepare("SELECT id, content, heading_path FROM chunks")?;
        let rows: Vec<(i64, String, Option<String>)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        drop(stmt);
        for (id, content, heading) in rows {
            let tokens = super::jieba::tokenize_for_index(&content);
            conn.execute(
                "INSERT INTO chunks_fts(rowid, tokens, heading_path) VALUES (?, ?, ?)",
                params![id, tokens, heading],
            )?;
        }
    }
    conn.execute(
        "INSERT INTO settings(key, value) VALUES('fts_index_version', ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![FTS_INDEX_VERSION],
    )?;
    Ok(())
}
