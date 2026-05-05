use std::path::{Path, PathBuf};

use image::imageops::FilterType;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::ocr;
use crate::paths;
use crate::store::{chunks as store_chunks, games as store_games, pages as store_pages};

use super::chunker::chunk_markdown;
use super::AppState;

const THUMB_MAX_EDGE: u32 = 256;

#[derive(Debug, Clone, Serialize)]
struct PageStartedEvent {
    page_id: String,
    page_number: i64,
}

#[derive(Debug, Clone, Serialize)]
struct PageDoneEvent {
    page_id: String,
    page_number: i64,
    chunk_count: usize,
}

#[derive(Debug, Clone, Serialize)]
struct PageFailedEvent {
    page_id: String,
    page_number: i64,
    error: String,
}

#[derive(Debug, Clone, Serialize)]
struct IngestDoneEvent {
    game_id: String,
    succeeded: usize,
    failed: usize,
}

fn emit<T: Serialize + Clone>(app: &AppHandle, event: &str, payload: T) {
    if let Err(e) = app.emit(event, payload) {
        tracing::warn!("emit {event} failed: {e}");
    }
}

fn ext_of(p: &Path) -> String {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| "png".into())
}

fn copy_into_game(
    src: &Path,
    game_id: &str,
    page_no: i64,
    page_id: &str,
) -> AppResult<PathBuf> {
    let dir = paths::games_dir().join(game_id).join("pages");
    std::fs::create_dir_all(&dir)?;
    let ext = ext_of(src);
    let dst = dir.join(format!("{page_no}_{page_id}.{ext}"));
    std::fs::copy(src, &dst)?;
    Ok(dst)
}

fn make_thumb(image_path: &Path, game_id: &str, page_id: &str) -> AppResult<PathBuf> {
    let dir = paths::games_dir().join(game_id).join("thumbs");
    std::fs::create_dir_all(&dir)?;
    let dst = dir.join(format!("{page_id}.webp"));
    let img = image::open(image_path)?;
    let (w, h) = (img.width(), img.height());
    let longest = w.max(h);
    let resized = if longest > THUMB_MAX_EDGE {
        let scale = THUMB_MAX_EDGE as f32 / longest as f32;
        let nw = (w as f32 * scale).round().max(1.0) as u32;
        let nh = (h as f32 * scale).round().max(1.0) as u32;
        img.resize_exact(nw, nh, FilterType::Lanczos3)
    } else {
        img
    };
    // The `image` crate can encode WebP via its `image-webp` feature on output.
    // `save` uses the extension to pick the encoder.
    resized.to_rgba8().save(&dst)?;
    Ok(dst)
}

async fn process_one(
    state: &AppState,
    app: &AppHandle,
    game_id: &str,
    page_number: i64,
    src: &Path,
) -> AppResult<usize> {
    let page_id = Uuid::new_v4().to_string();

    // 1-3. Copy + thumb (sync I/O is fine; small files).
    let stored_image = copy_into_game(src, game_id, page_number, &page_id)?;
    let thumb = match make_thumb(&stored_image, game_id, &page_id) {
        Ok(p) => Some(p.to_string_lossy().to_string()),
        Err(e) => {
            tracing::warn!("thumb failed for {}: {e}", stored_image.display());
            None
        }
    };

    // 4. Insert page row.
    {
        let db = state.db.clone();
        let stored = stored_image.to_string_lossy().to_string();
        let thumb_clone = thumb.clone();
        let page_id_clone = page_id.clone();
        let game_id_clone = game_id.to_string();
        tokio::task::spawn_blocking(move || -> AppResult<()> {
            // Use the existing helper, which assigns a new uuid; we want our id.
            // Insert directly to control id.
            let conn = db.lock();
            conn.execute(
                "INSERT INTO pages (id, game_id, page_number, image_path, thumb_path, created_at) \
                 VALUES (?, ?, ?, ?, ?, ?)",
                rusqlite::params![
                    page_id_clone,
                    game_id_clone,
                    page_number,
                    stored,
                    thumb_clone,
                    time::OffsetDateTime::now_utc().unix_timestamp()
                ],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;
    }

    // 5. Emit started.
    emit(
        app,
        "ingest:page_started",
        PageStartedEvent {
            page_id: page_id.clone(),
            page_number,
        },
    );

    // 6. OCR.
    let markdown = match ocr::extract_markdown(&stored_image).await {
        Ok(md) => md,
        Err(e) => {
            let err_msg = e.to_string();
            let db = state.db.clone();
            let page_id_clone = page_id.clone();
            let _ = tokio::task::spawn_blocking(move || {
                store_pages::set_ocr_result(&db, &page_id_clone, "failed", None, None)
            })
            .await;
            emit(
                app,
                "ingest:page_failed",
                PageFailedEvent {
                    page_id,
                    page_number,
                    error: err_msg,
                },
            );
            return Err(e);
        }
    };

    // 7. Persist OCR.
    {
        let db = state.db.clone();
        let page_id_clone = page_id.clone();
        let md_clone = markdown.clone();
        tokio::task::spawn_blocking(move || {
            store_pages::set_ocr_result(&db, &page_id_clone, "done", Some(&md_clone), None)
        })
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;
    }

    // 8 + 9. Chunk + embed.
    let chunks_built = chunk_markdown(&markdown);
    let chunk_count = chunks_built.len();
    if chunk_count > 0 {
        let texts: Vec<String> = chunks_built.iter().map(|c| c.content.clone()).collect();
        let db = state.db.clone();
        let page_id_clone = page_id.clone();
        let game_id_clone = game_id.to_string();
        tokio::task::spawn_blocking(move || -> AppResult<()> {
            let embeds = crate::embed::embed_batch(&texts)?;
            for (chunk, vec) in chunks_built.into_iter().zip(embeds.into_iter()) {
                store_chunks::insert_chunk_with_embedding(
                    &db,
                    &page_id_clone,
                    &game_id_clone,
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

    // 11. Emit done.
    emit(
        app,
        "ingest:page_done",
        PageDoneEvent {
            page_id: page_id.clone(),
            page_number,
            chunk_count,
        },
    );

    // 12. Increment game page count.
    {
        let db = state.db.clone();
        let game_id_clone = game_id.to_string();
        tokio::task::spawn_blocking(move || store_games::increment_page_count(&db, &game_id_clone))
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;
    }

    Ok(chunk_count)
}

#[tauri::command]
pub async fn ingest_pages(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    game_id: String,
    image_paths: Vec<String>,
) -> AppResult<()> {
    let mut succeeded = 0usize;
    let mut failed = 0usize;
    for (idx, path_str) in image_paths.iter().enumerate() {
        let page_number = (idx as i64) + 1;
        let src = PathBuf::from(path_str);
        match process_one(&state, &app_handle, &game_id, page_number, &src).await {
            Ok(_) => succeeded += 1,
            Err(e) => {
                tracing::error!("ingest page {page_number} failed: {e}");
                failed += 1;
            }
        }
    }
    emit(
        &app_handle,
        "ingest:done",
        IngestDoneEvent {
            game_id,
            succeeded,
            failed,
        },
    );
    Ok(())
}
