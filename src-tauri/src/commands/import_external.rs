//! Import rulebook content from external sources (currently: BGG).
//!
//! Unlike the photo-based ingest pipeline, this path:
//! - skips OCR (text comes ready from BGG's description field),
//! - has no per-page images (`pages.image_path = ""`),
//! - still feeds chunking + embedding so RAG/Q&A works as usual.

use serde::Serialize;
use tauri::State;

use crate::cover::{self, bgg};
use crate::error::{AppError, AppResult};
use crate::store::{chunks as store_chunks, games as store_games};

use super::chunker::chunk_markdown;
use super::AppState;

#[derive(Serialize)]
pub struct BggImportResult {
    pub game_id: String,
    pub page_count: usize,
    pub chunk_count: usize,
}

#[tauri::command(rename_all = "snake_case")]
pub async fn bgg_search(query: String) -> AppResult<Vec<bgg::BggMatch>> {
    bgg::search_many(&query).await
}

/// Create a game (or use existing if `existing_game_id` is provided), pull
/// the BGG description, split it into "pages", chunk + embed, and kick off
/// the auto-cover pipeline. Returns the game id + counts so the UI can
/// navigate to the bookshelf afterwards.
#[tauri::command(rename_all = "snake_case")]
pub async fn import_from_bgg(
    state: State<'_, AppState>,
    bgg_id: u32,
    name_zh_override: Option<String>,
    existing_game_id: Option<String>,
) -> AppResult<BggImportResult> {
    let thing = bgg::fetch_thing_full(bgg_id)
        .await?
        .ok_or_else(|| AppError::Other(anyhow::anyhow!("BGG returned no item for id {bgg_id}")))?;

    if thing.description.trim().is_empty() {
        return Err(AppError::Other(anyhow::anyhow!(
            "BGG entry has no usable description text"
        )));
    }

    let display_name = name_zh_override
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| thing.primary_name.clone());

    // Either reuse the existing game or create a new one.
    let game_id = if let Some(gid) = existing_game_id {
        gid
    } else {
        let db = state.db.clone();
        let name_zh = display_name.clone();
        let name_en = if thing.primary_name.is_empty() {
            None
        } else {
            Some(thing.primary_name.clone())
        };
        tokio::task::spawn_blocking(move || {
            store_games::insert_game(&db, &name_zh, name_en.as_deref(), None)
        })
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??
    };

    // Split the description into ~2000-char "pages" by paragraph boundaries.
    // BGG descriptions for big games are 4-15 KB; chunking again at the
    // existing chunker will further subdivide as needed.
    let pages = split_into_pages(&thing.description, 2000);
    let mut total_chunks = 0usize;

    for (idx, page_md) in pages.iter().enumerate() {
        let page_no = (idx as i64) + 1;
        let page_id = uuid::Uuid::new_v4().to_string();

        // Insert page row. image_path is empty since we have no image.
        let db = state.db.clone();
        let pid = page_id.clone();
        let gid = game_id.clone();
        let md_for_db = page_md.clone();
        tokio::task::spawn_blocking(move || -> AppResult<()> {
            let conn = db.lock();
            conn.execute(
                "INSERT INTO pages \
                    (id, game_id, page_number, image_path, thumb_path, ocr_status, ocr_markdown, ocr_json, created_at) \
                 VALUES (?, ?, ?, '', NULL, 'external', ?, NULL, ?)",
                rusqlite::params![
                    pid,
                    gid,
                    page_no,
                    md_for_db,
                    time::OffsetDateTime::now_utc().unix_timestamp()
                ],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;

        // Chunk + embed.
        let chunks_built = chunk_markdown(page_md);
        let chunk_count = chunks_built.len();
        total_chunks += chunk_count;
        if chunk_count > 0 {
            let texts: Vec<String> = chunks_built.iter().map(|c| c.content.clone()).collect();
            let db = state.db.clone();
            let pid = page_id.clone();
            let gid = game_id.clone();
            tokio::task::spawn_blocking(move || -> AppResult<()> {
                let embeds = crate::embed::embed_batch(&texts)?;
                for (chunk, vec) in chunks_built.into_iter().zip(embeds.into_iter()) {
                    store_chunks::insert_chunk_with_embedding(
                        &db,
                        &pid,
                        &gid,
                        chunk.heading_path.as_deref(),
                        &chunk.content,
                        chunk.token_count as i64,
                        &vec,
                    )?;
                }
                Ok(())
            })
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;
        }

        // Bump game.page_count.
        let db = state.db.clone();
        let gid = game_id.clone();
        tokio::task::spawn_blocking(move || store_games::increment_page_count(&db, &gid))
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;
    }

    // Auto-cover + research pass (background — caller already has the data
    // it needs to navigate to the bookshelf).
    let db = state.db.clone();
    let gid = game_id.clone();
    let bgg_id_for_persist = bgg_id;
    tauri::async_runtime::spawn(async move {
        // Persist the resolved bgg_id up front so the research pipeline
        // skips its own BGG search.
        let db_set = db.clone();
        let gid_set = gid.clone();
        let _ = tokio::task::spawn_blocking(move || {
            store_games::set_bgg_id(&db_set, &gid_set, bgg_id_for_persist)
        })
        .await;

        if let Err(e) = cover::auto::auto_set_cover(&db, &gid).await {
            tracing::warn!("BGG import auto-cover failed: {e}");
        }
        if let Err(e) = crate::research::pipeline::run_for_game(&db, &gid).await {
            tracing::warn!("BGG import research pass failed: {e}");
        }
    });

    Ok(BggImportResult {
        game_id,
        page_count: pages.len(),
        chunk_count: total_chunks,
    })
}

/// Split a long description into roughly `target_size`-character pages,
/// breaking on paragraph boundaries (`\n\n`) to keep heading context together.
fn split_into_pages(text: &str, target_size: usize) -> Vec<String> {
    let mut pages: Vec<String> = Vec::new();
    let mut current = String::with_capacity(target_size);

    for para in text.split("\n\n") {
        let p = para.trim();
        if p.is_empty() {
            continue;
        }
        if current.len() + p.len() + 2 > target_size && !current.is_empty() {
            pages.push(std::mem::take(&mut current).trim().to_string());
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(p);
    }
    if !current.trim().is_empty() {
        pages.push(current.trim().to_string());
    }
    if pages.is_empty() {
        pages.push(text.trim().to_string());
    }
    pages
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_into_pages_keeps_paragraph_boundaries() {
        let text = "Para one with several words.\n\nPara two also with content.\n\nPara three appended here.";
        let pages = split_into_pages(text, 50);
        assert!(pages.len() >= 2);
        assert!(pages.iter().all(|p| !p.is_empty()));
    }

    #[test]
    fn split_into_pages_returns_single_page_for_short_input() {
        let pages = split_into_pages("Short text.", 2000);
        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0], "Short text.");
    }
}
