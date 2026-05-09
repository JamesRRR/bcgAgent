//! Wave 1 schema migration: provenance fields on `chunks`, new
//! KB/research tables, and a one-time backfill from `game_external_refs`
//! into `chunks` so the new spine becomes the single source of truth.
//!
//! All steps are idempotent. The structural part (ALTERs + CREATE TABLEs)
//! runs every startup but is no-op after first success. The data backfill
//! is gated by a `kv_meta` row and only runs once. When the backfill is
//! about to run, the caller is expected to have already taken a backup.

use rusqlite::{params, Connection};
use time::OffsetDateTime;
use zerocopy::AsBytes;

use super::db::Db;
use super::jieba;
use crate::commands::chunker::chunk_markdown;
use crate::error::AppResult;

/// Marker key in `kv_meta` indicating the backfill has run successfully.
pub(crate) const BACKFILL_KEY: &str = "wave1_external_refs_backfill";
const BACKFILL_VALUE: &str = "done";

/// Add a column to a table if it isn't already present. Reads
/// `PRAGMA table_info` to decide; does nothing if the column exists. Used
/// for the additive ALTERs on `chunks` so the schema can be re-applied
/// safely on every startup.
pub(crate) fn add_column_if_missing(
    conn: &Connection,
    table: &str,
    column: &str,
    decl: &str,
) -> AppResult<()> {
    let mut stmt = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let names: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(1))?
        .collect::<Result<_, _>>()?;
    drop(stmt);
    if names.iter().any(|n| n == column) {
        return Ok(());
    }
    conn.execute(
        &format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"),
        [],
    )?;
    Ok(())
}

/// Run all idempotent Wave 1 structural changes (columns + tables + index).
/// Safe to call on a fresh DB or one that already has Wave 1 applied.
pub(crate) fn ensure_schema(conn: &Connection) -> AppResult<()> {
    // kv_meta — generic key-value store for migration markers. We could
    // reuse `settings` but keeping migrations on their own table makes
    // intent explicit and avoids accidental UI overwrites.
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS kv_meta (
             key   TEXT PRIMARY KEY,
             value TEXT NOT NULL
         );",
    )?;

    // Provenance columns on chunks.
    for (col, decl) in [
        ("source_kind", "TEXT NOT NULL DEFAULT 'photo_ocr'"),
        ("source_url", "TEXT"),
        ("trust_tier", "TEXT NOT NULL DEFAULT 'publisher'"),
        ("official", "INTEGER NOT NULL DEFAULT 1"),
        ("confidence", "REAL NOT NULL DEFAULT 1.0"),
        ("fetched_at", "INTEGER"),
        ("endorsed", "INTEGER"),
        ("content_lang", "TEXT NOT NULL DEFAULT 'zh'"),
        ("content_orig", "TEXT"),
    ] {
        add_column_if_missing(conn, "chunks", col, decl)?;
    }

    conn.execute_batch(
        "CREATE INDEX IF NOT EXISTS idx_chunks_game_tier \
            ON chunks(game_id, trust_tier, official);

         CREATE TABLE IF NOT EXISTS components (
             id              INTEGER PRIMARY KEY AUTOINCREMENT,
             game_id         TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
             name_zh         TEXT NOT NULL,
             category        TEXT,
             effect_zh       TEXT,
             source_kind     TEXT NOT NULL,
             source_url      TEXT,
             page_id         TEXT,
             bbox_json       TEXT,
             illustration_id TEXT,
             trust_tier      TEXT NOT NULL,
             confidence      REAL NOT NULL,
             created_at      INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_components_game ON components(game_id);

         CREATE TABLE IF NOT EXISTS faq_pairs (
             id            INTEGER PRIMARY KEY AUTOINCREMENT,
             game_id       TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
             question_zh   TEXT NOT NULL,
             answer_zh     TEXT NOT NULL,
             source_kind   TEXT NOT NULL,
             source_url    TEXT,
             trust_tier    TEXT NOT NULL,
             official      INTEGER NOT NULL,
             confidence    REAL NOT NULL,
             fetched_at    INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_faq_game ON faq_pairs(game_id);

         CREATE TABLE IF NOT EXISTS setup_steps (
             id            INTEGER PRIMARY KEY AUTOINCREMENT,
             game_id       TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
             step_no       INTEGER NOT NULL,
             player_count  TEXT,
             text_zh       TEXT NOT NULL,
             component_ids TEXT,
             source_kind   TEXT NOT NULL,
             source_url    TEXT,
             page_id       TEXT,
             trust_tier    TEXT NOT NULL,
             confidence    REAL NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_setup_game ON setup_steps(game_id, step_no);

         CREATE TABLE IF NOT EXISTS research_events (
             id               INTEGER PRIMARY KEY AUTOINCREMENT,
             game_id          TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
             trigger          TEXT NOT NULL,
             query            TEXT NOT NULL,
             query_normalized TEXT NOT NULL,
             hits_json        TEXT NOT NULL,
             chunks_added     INTEGER NOT NULL,
             cost_estimate    REAL,
             created_at       INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_research_game
             ON research_events(game_id, query_normalized);

         CREATE TABLE IF NOT EXISTS web_cache (
             url        TEXT PRIMARY KEY,
             status     INTEGER,
             fetched_at INTEGER NOT NULL,
             content_md TEXT,
             content_zh TEXT,
             etag       TEXT,
             expires_at INTEGER NOT NULL
         );

         CREATE TABLE IF NOT EXISTS research_budget (
             game_id  INTEGER NOT NULL,
             date_utc TEXT NOT NULL,
             events   INTEGER NOT NULL DEFAULT 0,
             PRIMARY KEY (game_id, date_utc)
         );",
    )?;
    Ok(())
}

/// Has the external_refs → chunks backfill already run?
pub(crate) fn backfill_already_done(conn: &Connection) -> AppResult<bool> {
    let v: Option<String> = conn
        .query_row(
            "SELECT value FROM kv_meta WHERE key = ?",
            params![BACKFILL_KEY],
            |row| row.get(0),
        )
        .ok();
    Ok(matches!(v.as_deref(), Some(BACKFILL_VALUE)))
}

fn mark_backfill_done(conn: &Connection) -> AppResult<()> {
    conn.execute(
        "INSERT INTO kv_meta (key, value) VALUES (?, ?) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![BACKFILL_KEY, BACKFILL_VALUE],
    )?;
    Ok(())
}

/// Map a `game_external_refs.kind` value to the new `source_kind` vocabulary.
fn kind_to_source_kind(kind: &str) -> &'static str {
    match kind {
        "description" => "bgg_description",
        "forum_thread" => "bgg_forum",
        "gallery_caption" | "illustration_caption" => "bgg_gallery",
        _ => "external",
    }
}

/// Run the one-time external_refs → chunks backfill if it hasn't already.
/// `embed_fn` is injected so tests can substitute a deterministic stand-in
/// for the heavyweight BGE-M3 model. Production callers pass
/// `crate::embed::embed_batch`.
///
/// Each external_refs row turns into N chunks (one per chunker output) with
/// provenance derived from its kind. The legacy table is left untouched so
/// old code paths still work; this is a copy, not a move.
pub fn run_external_refs_backfill<F>(db: &Db, embed_fn: F) -> AppResult<usize>
where
    F: Fn(&[String]) -> AppResult<Vec<Vec<f32>>>,
{
    {
        let conn = db.lock();
        if backfill_already_done(&conn)? {
            return Ok(0);
        }
    }

    // Pull all external_refs rows up front so we don't hold the connection
    // lock across the embed call (which can be slow on first model init).
    struct Row {
        game_id: String,
        kind: String,
        content: String,
        url: Option<String>,
        fetched_at: i64,
    }

    let rows: Vec<Row> = {
        let conn = db.lock();
        let mut stmt = conn.prepare(
            "SELECT game_id, kind, content, url, fetched_at FROM game_external_refs \
             ORDER BY id ASC",
        )?;
        let collected: Vec<Row> = stmt
            .query_map([], |r| {
                Ok(Row {
                    game_id: r.get(0)?,
                    kind: r.get(1)?,
                    content: r.get(2)?,
                    url: r.get(3)?,
                    fetched_at: r.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        collected
    };

    if rows.is_empty() {
        let conn = db.lock();
        mark_backfill_done(&conn)?;
        return Ok(0);
    }

    // Find the canonical "external" page per game so backfilled chunks have
    // a valid page_id (chunks.page_id has a NOT NULL FK to pages). If a
    // game has no external page yet, create one with image_path = '' to
    // mirror what the BGG import pipeline does.
    fn external_page_id(conn: &Connection, game_id: &str) -> AppResult<String> {
        let existing: Option<String> = conn
            .query_row(
                "SELECT id FROM pages \
                 WHERE game_id = ? AND ocr_status = 'external' \
                 ORDER BY page_number ASC LIMIT 1",
                params![game_id],
                |r| r.get(0),
            )
            .ok();
        if let Some(id) = existing {
            return Ok(id);
        }
        let pid = uuid::Uuid::new_v4().to_string();
        // page_number = max + 1
        let next_no: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(page_number), 0) + 1 FROM pages WHERE game_id = ?",
                params![game_id],
                |r| r.get(0),
            )
            .unwrap_or(1);
        conn.execute(
            "INSERT INTO pages \
                (id, game_id, page_number, image_path, thumb_path, ocr_status, \
                 ocr_markdown, ocr_json, created_at) \
             VALUES (?, ?, ?, '', NULL, 'external', NULL, NULL, ?)",
            params![
                pid,
                game_id,
                next_no,
                OffsetDateTime::now_utc().unix_timestamp(),
            ],
        )?;
        Ok(pid)
    }

    let mut inserted = 0usize;
    for row in rows {
        let chunked = chunk_markdown(&row.content);
        if chunked.is_empty() {
            continue;
        }

        let texts: Vec<String> = chunked.iter().map(|c| c.content.clone()).collect();
        let embeds = embed_fn(&texts)?;
        if embeds.len() != chunked.len() {
            return Err(crate::error::AppError::Embed(format!(
                "embed_fn returned {} vectors for {} texts",
                embeds.len(),
                chunked.len()
            )));
        }

        let source_kind = kind_to_source_kind(&row.kind);
        let trust_tier = if source_kind == "bgg_description" {
            "publisher"
        } else {
            "community"
        };
        let official: i64 = if trust_tier == "publisher" { 1 } else { 0 };

        let conn = db.lock();
        let page_id = external_page_id(&conn, &row.game_id)?;
        for (chunk, vec) in chunked.into_iter().zip(embeds.into_iter()) {
            conn.execute(
                "INSERT INTO chunks \
                    (page_id, game_id, heading_path, content, token_count, \
                     source_kind, source_url, trust_tier, official, confidence, \
                     fetched_at, content_lang, content_orig) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'zh', NULL)",
                params![
                    page_id,
                    row.game_id,
                    chunk.heading_path,
                    chunk.content,
                    chunk.token_count as i64,
                    source_kind,
                    row.url,
                    trust_tier,
                    official,
                    0.9,
                    row.fetched_at,
                ],
            )?;
            let chunk_id = conn.last_insert_rowid();
            let embedding_bytes: &[u8] = vec.as_bytes();
            conn.execute(
                "INSERT INTO chunks_vec(rowid, embedding) VALUES (?, ?)",
                params![chunk_id, embedding_bytes],
            )?;
            let tokens = jieba::tokenize_for_index(&chunk.content);
            conn.execute(
                "INSERT INTO chunks_fts(rowid, tokens, heading_path) VALUES (?, ?, ?)",
                params![chunk_id, tokens, chunk.heading_path],
            )?;
            inserted += 1;
        }
    }

    let conn = db.lock();
    mark_backfill_done(&conn)?;
    Ok(inserted)
}

/// Should the backfill actually run? True iff we haven't marked it done AND
/// there is at least one external_refs row to copy. Used by the startup
/// path to decide whether to take a DB backup before mutating.
pub(crate) fn backfill_will_run(conn: &Connection) -> AppResult<bool> {
    if backfill_already_done(conn)? {
        return Ok(false);
    }
    let n: i64 = conn
        .query_row("SELECT COUNT(*) FROM game_external_refs", [], |r| r.get(0))
        .unwrap_or(0);
    Ok(n > 0)
}
