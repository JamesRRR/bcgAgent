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
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Acquire the connection lock. All other modules call this per-operation.
    pub(crate) fn lock(&self) -> parking_lot::MutexGuard<'_, Connection> {
        self.conn.lock()
    }
}
