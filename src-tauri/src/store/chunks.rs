use rusqlite::params;
use zerocopy::AsBytes;

use super::db::Db;
use super::jieba;
use super::models::Chunk;
use crate::error::AppResult;

/// Provenance metadata attached to every chunk. See the design spec
/// "Provenance spine" section for the full vocabulary. Defaults match
/// the legacy photo-OCR flow: `photo_ocr` / `publisher` / official.
#[derive(Debug, Clone)]
pub struct ChunkProvenance<'a> {
    pub source_kind: &'a str,
    pub source_url: Option<&'a str>,
    pub trust_tier: &'a str,
    pub official: bool,
    pub confidence: f64,
    pub fetched_at: Option<i64>,
    pub content_lang: &'a str,
    pub content_orig: Option<&'a str>,
}

impl<'a> ChunkProvenance<'a> {
    /// The default (legacy) provenance — what photo-OCR ingest produces.
    pub fn photo_ocr() -> Self {
        Self {
            source_kind: "photo_ocr",
            source_url: None,
            trust_tier: "publisher",
            official: true,
            confidence: 1.0,
            fetched_at: None,
            content_lang: "zh",
            content_orig: None,
        }
    }
}

/// Convert a jieba-cut query string into an FTS5 OR expression like
/// `"羁绊" OR "什么"`. Each term is wrapped in double quotes so FTS5 treats
/// it as a phrase literal — that way punctuation, hyphens, or any character
/// FTS5 reserves for syntax is escaped in the simplest possible way.
/// Empty whitespace tokens are dropped; embedded `"` is doubled per FTS5
/// quoting rules.
fn build_or_match_expr(tokenized: &str) -> String {
    let parts: Vec<String> = tokenized
        .split_whitespace()
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{}\"", t.replace('"', "\"\"")))
        .collect();
    parts.join(" OR ")
}

/// Insert a chunk, its 1024-d embedding, and its jieba-tokenized FTS row.
/// All three rows share the same `rowid` so we can join by it later.
///
/// Legacy entry point — uses default photo-OCR provenance so existing call
/// sites keep their behavior. New code should call
/// `insert_chunk_with_embedding_and_provenance` directly.
pub fn insert_chunk_with_embedding(
    db: &Db,
    page_id: &str,
    game_id: &str,
    heading_path: Option<&str>,
    content: &str,
    token_count: i64,
    embedding: &[f32],
) -> AppResult<i64> {
    insert_chunk_with_embedding_and_provenance(
        db,
        page_id,
        game_id,
        heading_path,
        content,
        token_count,
        embedding,
        &ChunkProvenance::photo_ocr(),
    )
}

/// Same as `insert_chunk_with_embedding`, but also writes the provenance
/// columns added in Wave 1. Holds the connection lock for all three writes
/// so a half-inserted chunk can't be observed.
pub fn insert_chunk_with_embedding_and_provenance(
    db: &Db,
    page_id: &str,
    game_id: &str,
    heading_path: Option<&str>,
    content: &str,
    token_count: i64,
    embedding: &[f32],
    prov: &ChunkProvenance<'_>,
) -> AppResult<i64> {
    let conn = db.lock();
    conn.execute(
        "INSERT INTO chunks \
            (page_id, game_id, heading_path, content, token_count, \
             source_kind, source_url, trust_tier, official, confidence, \
             fetched_at, content_lang, content_orig) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        params![
            page_id,
            game_id,
            heading_path,
            content,
            token_count,
            prov.source_kind,
            prov.source_url,
            prov.trust_tier,
            if prov.official { 1i64 } else { 0i64 },
            prov.confidence,
            prov.fetched_at,
            prov.content_lang,
            prov.content_orig,
        ],
    )?;
    let chunk_id = conn.last_insert_rowid();

    let embedding_bytes: &[u8] = embedding.as_bytes();
    conn.execute(
        "INSERT INTO chunks_vec(rowid, embedding) VALUES (?, ?)",
        params![chunk_id, embedding_bytes],
    )?;

    let tokens = jieba::tokenize_for_index(content);
    conn.execute(
        "INSERT INTO chunks_fts(rowid, tokens, heading_path) VALUES (?, ?, ?)",
        params![chunk_id, tokens, heading_path],
    )?;

    Ok(chunk_id)
}

/// Group `chunks` by `source_kind` for one game. Returned as `(kind, count)`
/// rows ordered by descending count then ascending kind for stable diffs.
/// Used by the Wave 5 KB-diff harness; safe to call at any time.
pub fn count_chunks_by_source_kind(db: &Db, game_id: &str) -> AppResult<Vec<(String, u64)>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT source_kind, COUNT(*) as n FROM chunks WHERE game_id = ? \
         GROUP BY source_kind ORDER BY n DESC, source_kind ASC",
    )?;
    let rows = stmt
        .query_map(params![game_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u64))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Update the user-endorsement flag on a chunk. `None` clears the flag,
/// `Some(true)` thumbs-up, `Some(false)` thumbs-down. The retrieval layer
/// reads this column; we don't recompute embeddings here.
pub fn update_chunk_endorsed(db: &Db, chunk_id: i64, endorsed: Option<bool>) -> AppResult<()> {
    let conn = db.lock();
    let val: Option<i64> = endorsed.map(|b| if b { 1 } else { 0 });
    conn.execute(
        "UPDATE chunks SET endorsed = ? WHERE id = ?",
        params![val, chunk_id],
    )?;
    Ok(())
}

pub fn get_chunk(db: &Db, id: i64) -> AppResult<Option<Chunk>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT id, page_id, game_id, heading_path, content, token_count \
         FROM chunks WHERE id = ?",
    )?;
    let mut rows = stmt.query(params![id])?;
    if let Some(row) = rows.next()? {
        Ok(Some(Chunk {
            id: row.get(0)?,
            page_id: row.get(1)?,
            game_id: row.get(2)?,
            heading_path: row.get(3)?,
            content: row.get(4)?,
            token_count: row.get(5)?,
        }))
    } else {
        Ok(None)
    }
}

/// kNN search against `chunks_vec`. Returns `(chunk_id, l2_distance)` sorted
/// best-first (smaller distance = closer). If `game_id` is `Some`, results are
/// filtered to that game; we over-fetch by 4x then filter, which is good
/// enough for the small libraries this app handles.
pub fn vec_search(
    db: &Db,
    query: &[f32],
    game_id: Option<&str>,
    k: usize,
) -> AppResult<Vec<(i64, f32)>> {
    let conn = db.lock();
    let query_bytes: &[u8] = query.as_bytes();

    let fetch_k = if game_id.is_some() { k * 4 } else { k };

    let mut stmt = conn.prepare(
        "SELECT rowid, distance FROM chunks_vec \
         WHERE embedding MATCH ? AND k = ? \
         ORDER BY distance",
    )?;
    let raw: Vec<(i64, f32)> = stmt
        .query_map(params![query_bytes, fetch_k as i64], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)? as f32))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    let filtered: Vec<(i64, f32)> = match game_id {
        None => raw.into_iter().take(k).collect(),
        Some(gid) => {
            let mut out = Vec::with_capacity(k);
            let mut check = conn.prepare("SELECT 1 FROM chunks WHERE id = ? AND game_id = ?")?;
            for (id, dist) in raw {
                let mut rows = check.query(params![id, gid])?;
                if rows.next()?.is_some() {
                    out.push((id, dist));
                    if out.len() >= k {
                        break;
                    }
                }
            }
            out
        }
    };

    Ok(filtered)
}

/// FTS5 search against the jieba-tokenized `tokens` column. Returns
/// `(chunk_id, bm25_score)` sorted best-first (smaller bm25 = better match).
/// If `game_id` is `Some`, results are filtered to that game post-hoc.
///
/// We OR the query terms together rather than relying on FTS5's default AND,
/// because a question like "什么是羁绊" tokenizes to ["什么","是","羁绊"] and
/// an AND match misses every chunk that doesn't contain all three — including
/// the chunk that actually defines 羁绊. BM25 up-weights rare terms, so OR +
/// bm25 ranking surfaces the relevant chunk naturally.
pub fn fts_search(
    db: &Db,
    query: &str,
    game_id: Option<&str>,
    k: usize,
) -> AppResult<Vec<(i64, f32)>> {
    let tokenized = jieba::tokenize_for_query(query);
    let match_expr = build_or_match_expr(&tokenized);
    if match_expr.is_empty() {
        return Ok(Vec::new());
    }
    let conn = db.lock();

    let fetch_k = if game_id.is_some() { k * 4 } else { k };

    let mut stmt = conn.prepare(
        "SELECT rowid, bm25(chunks_fts) FROM chunks_fts \
         WHERE tokens MATCH ? \
         ORDER BY bm25(chunks_fts) LIMIT ?",
    )?;
    let raw: Vec<(i64, f32)> = stmt
        .query_map(params![match_expr, fetch_k as i64], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)? as f32))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);

    let filtered: Vec<(i64, f32)> = match game_id {
        None => raw.into_iter().take(k).collect(),
        Some(gid) => {
            let mut out = Vec::with_capacity(k);
            let mut check = conn.prepare("SELECT 1 FROM chunks WHERE id = ? AND game_id = ?")?;
            for (id, score) in raw {
                let mut rows = check.query(params![id, gid])?;
                if rows.next()?.is_some() {
                    out.push((id, score));
                    if out.len() >= k {
                        break;
                    }
                }
            }
            out
        }
    };

    Ok(filtered)
}

/// Fetch every chunk for a game, joined with its page so callers get the
/// page number and heading without extra round-trips. Ordered by page then
/// chunk insertion order so the result reads top-to-bottom like the book.
/// Used by the walkthrough generator, which needs the full rulebook in
/// context rather than retrieval-filtered slices.
pub fn list_chunks_for_game(
    db: &Db,
    game_id: &str,
) -> AppResult<Vec<(i64, i64, Option<String>, String)>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT c.id, p.page_number, c.heading_path, c.content \
         FROM chunks c \
         JOIN pages p ON p.id = c.page_id \
         WHERE c.game_id = ? \
         ORDER BY p.page_number ASC, c.id ASC",
    )?;
    let rows: Vec<(i64, i64, Option<String>, String)> = stmt
        .query_map(params![game_id], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}
