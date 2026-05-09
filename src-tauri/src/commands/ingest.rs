use std::path::{Path, PathBuf};
use std::sync::Arc;

use image::imageops::FilterType;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::events::{emit as sink_emit, EventSink};
use crate::ocr;
use crate::paths;
use crate::store::{
    chunks as store_chunks, games as store_games, illustrations as store_illustrations,
    pages as store_pages,
};

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

fn ext_of(p: &Path) -> String {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| "png".into())
}

/// HEIC images (iPhone photos) aren't supported by the `image` crate.
/// On macOS we shell out to `sips` to transcode to JPEG first; the rest
/// of the pipeline (thumbnailing, OCR base64) then works as-is.
/// Returns the path to use for processing — either the original (if not
/// HEIC) or a sibling JPEG written next to the original.
fn normalize_for_processing(src: &Path) -> AppResult<PathBuf> {
    let ext = ext_of(src);
    if ext != "heic" && ext != "heif" {
        return Ok(src.to_path_buf());
    }
    let dst = src.with_extension("converted.jpg");
    if dst.exists() {
        return Ok(dst);
    }
    let status = std::process::Command::new("sips")
        .args(["-s", "format", "jpeg"])
        .arg(src)
        .arg("--out")
        .arg(&dst)
        .status()
        .map_err(|e| {
            crate::error::AppError::Other(anyhow::anyhow!(
                "sips not available — needed to convert HEIC: {e}"
            ))
        })?;
    if !status.success() {
        return Err(crate::error::AppError::Other(anyhow::anyhow!(
            "sips failed to convert HEIC ({})",
            src.display()
        )));
    }
    Ok(dst)
}

fn copy_into_game(src: &Path, game_id: &str, page_no: i64, page_id: &str) -> AppResult<PathBuf> {
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
    resized.to_rgba8().save(&dst)?;
    Ok(dst)
}

async fn process_one(
    state: &AppState,
    sink: &EventSink,
    game_id: &str,
    page_number: i64,
    src: &Path,
) -> AppResult<usize> {
    let page_id = Uuid::new_v4().to_string();

    // Transcode HEIC → JPEG up front so the rest of the pipeline (thumb,
    // OCR base64) handles a format the `image` crate can read.
    let normalized = normalize_for_processing(src)?;
    let stored_image = copy_into_game(&normalized, game_id, page_number, &page_id)?;
    let thumb = match make_thumb(&stored_image, game_id, &page_id) {
        Ok(p) => Some(p.to_string_lossy().to_string()),
        Err(e) => {
            tracing::warn!("thumb failed for {}: {e}", stored_image.display());
            None
        }
    };

    {
        let db = state.db.clone();
        let stored = stored_image.to_string_lossy().to_string();
        let thumb_clone = thumb.clone();
        let page_id_clone = page_id.clone();
        let game_id_clone = game_id.to_string();
        tokio::task::spawn_blocking(move || -> AppResult<()> {
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

    sink_emit(
        sink,
        "ingest:page_started",
        &PageStartedEvent {
            page_id: page_id.clone(),
            page_number,
        },
    );

    let (markdown, illustrations) = match ocr::extract_grounded(&stored_image).await {
        Ok(pair) => pair,
        Err(e) => {
            // Grounded mode generates more output (markdown + JSON) and can
            // time out on very dense or very large pages. Fall back to plain
            // text-only OCR so a flaky grounded call doesn't leave the page
            // unindexed — we just lose illustration crops for that page.
            tracing::warn!(
                page_number,
                error = %e,
                "grounded OCR failed; falling back to plain markdown"
            );
            match ocr::extract_markdown(&stored_image).await {
                Ok(md) => (md, Vec::new()),
                Err(e2) => {
                    let err_msg = e2.to_string();
                    let db = state.db.clone();
                    let page_id_clone = page_id.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        store_pages::set_ocr_result(&db, &page_id_clone, "failed", None, None)
                    })
                    .await;
                    sink_emit(
                        sink,
                        "ingest:page_failed",
                        &PageFailedEvent {
                            page_id,
                            page_number,
                            error: err_msg,
                        },
                    );
                    return Err(e2);
                }
            }
        }
    };

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

    // Crop and persist each detected illustration. Cropping happens off the
    // async thread (image decode is CPU-bound), and a single bad crop does
    // not abort the rest of the page — we just log and continue.
    if !illustrations.is_empty() {
        let stored_image = stored_image.clone();
        let game_id_owned = game_id.to_string();
        let page_id_owned = page_id.clone();
        let db = state.db.clone();
        let _ = tokio::task::spawn_blocking(move || {
            crop_and_save_illustrations(
                &db,
                &game_id_owned,
                &page_id_owned,
                &stored_image,
                &illustrations,
            )
        })
        .await;
    }

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

    sink_emit(
        sink,
        "ingest:page_done",
        &PageDoneEvent {
            page_id: page_id.clone(),
            page_number,
            chunk_count,
        },
    );

    {
        let db = state.db.clone();
        let game_id_clone = game_id.to_string();
        tokio::task::spawn_blocking(move || store_games::increment_page_count(&db, &game_id_clone))
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;
    }

    Ok(chunk_count)
}

/// How many pages we OCR concurrently. OCR is network-bound (~10s/page
/// against DashScope), so 4-way concurrency gives ~4x throughput without
/// hammering the API. DB writes serialize through the connection mutex.
const INGEST_CONCURRENCY: usize = 4;

/// Transport-agnostic ingest orchestration. Used by both the Tauri command
/// and the HTTP test-server.
pub async fn run_ingest(
    state: &AppState,
    sink: EventSink,
    game_id: String,
    image_paths: Vec<String>,
) -> AppResult<()> {
    use futures::stream::{self, StreamExt};

    let total = image_paths.len();
    let results: Vec<bool> = stream::iter(image_paths.into_iter().enumerate())
        .map(|(idx, path_str)| {
            let sink = sink.clone();
            let game_id = game_id.clone();
            async move {
                let page_number = (idx as i64) + 1;
                let src = PathBuf::from(&path_str);
                match process_one(state, &sink, &game_id, page_number, &src).await {
                    Ok(_) => true,
                    Err(e) => {
                        tracing::error!("ingest page {page_number} failed: {e}");
                        false
                    }
                }
            }
        })
        .buffer_unordered(INGEST_CONCURRENCY.min(total.max(1)))
        .collect()
        .await;

    let succeeded = results.iter().filter(|ok| **ok).count();
    let failed = results.len() - succeeded;

    sink_emit(
        &sink,
        "ingest:done",
        &IngestDoneEvent {
            game_id: game_id.clone(),
            succeeded,
            failed,
        },
    );

    // Post-ingest knowledge enrichment. All errors swallowed: a missing
    // cover or BGG outage never fails an import. Runs in the background so
    // the user can navigate away as soon as `ingest:done` fires.
    if succeeded > 0 {
        let db = state.db.clone();
        let game_id_bg = game_id.clone();
        let sink_bg = sink.clone();
        tauri::async_runtime::spawn(async move {
            if let Err(e) = crate::cover::auto::auto_set_cover(&db, &game_id_bg).await {
                tracing::warn!("auto_set_cover for {game_id_bg} failed: {e}");
            }
            sink_emit(
                &sink_bg,
                "research:started",
                &serde_json::json!({"game_id": game_id_bg}),
            );
            match crate::research::pipeline::run_for_game_with_sink(
                &db,
                &game_id_bg,
                Some(sink_bg.clone()),
            )
            .await
            {
                Ok(summary) => {
                    sink_emit(
                        &sink_bg,
                        "research:done",
                        &serde_json::json!({
                            "game_id": game_id_bg,
                            "summary": summary,
                        }),
                    );
                }
                Err(e) => tracing::warn!("research pass for {game_id_bg} failed: {e}"),
            }
        });
    }

    Ok(())
}

/// Crop each Qwen-VL-detected illustration out of the page photo and persist
/// the crop alongside a row in `page_illustrations`. Bad crops (decode error,
/// out-of-bounds bbox, write failure) are skipped with a warn — we never want
/// a flaky illustration to fail the whole OCR pass.
fn crop_and_save_illustrations(
    db: &crate::store::Db,
    game_id: &str,
    page_id: &str,
    src_image: &Path,
    illustrations: &[ocr::Illustration],
) -> AppResult<()> {
    if illustrations.is_empty() {
        return Ok(());
    }
    let dir = paths::games_dir().join(game_id).join("illustrations");
    std::fs::create_dir_all(&dir)?;

    let img = match image::open(src_image) {
        Ok(i) => i,
        Err(e) => {
            tracing::warn!(
                "could not decode {} for cropping: {}",
                src_image.display(),
                e
            );
            return Ok(());
        }
    };
    let (img_w, img_h) = (img.width(), img.height());

    for (idx, ill) in illustrations.iter().enumerate() {
        let x = ill.x1.min(img_w);
        let y = ill.y1.min(img_h);
        let x2 = ill.x2.min(img_w);
        let y2 = ill.y2.min(img_h);
        if x2 <= x || y2 <= y {
            continue;
        }
        let w = x2 - x;
        let h = y2 - y;
        let crop = img.crop_imm(x, y, w, h);
        let id = uuid::Uuid::new_v4().to_string();
        let dst = dir.join(format!("{page_id}_{idx}_{id}.jpg"));
        if let Err(e) = crop.to_rgb8().save(&dst) {
            tracing::warn!("save crop {} failed: {}", dst.display(), e);
            continue;
        }
        if let Err(e) = store_illustrations::insert(
            db,
            &id,
            page_id,
            game_id,
            idx as i64,
            &dst.to_string_lossy(),
            (x, y, x2, y2),
            ill.label.as_deref(),
            Some(&ill.token),
        ) {
            tracing::warn!("insert illustration row failed: {}", e);
        }
    }
    Ok(())
}

#[tauri::command(rename_all = "snake_case")]
pub async fn ingest_pages(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    game_id: String,
    image_paths: Vec<String>,
) -> AppResult<()> {
    let app = app_handle.clone();
    let sink: EventSink = Arc::new(move |event: &str, payload: serde_json::Value| {
        if let Err(e) = app.emit(event, payload) {
            tracing::warn!("emit {event} failed: {e}");
        }
    });
    run_ingest(&state, sink, game_id, image_paths).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ImageBuffer, Rgb};

    #[test]
    fn heic_normalizes_to_readable_jpeg() {
        // Synthesize a JPEG, convert to HEIC via sips, then run our normalizer
        // and confirm the output is a JPEG the `image` crate can decode.
        let dir = tempfile::tempdir().expect("tempdir");
        let jpeg = dir.path().join("source.jpg");
        let heic = dir.path().join("source.heic");

        let img: ImageBuffer<Rgb<u8>, _> = ImageBuffer::from_fn(64, 64, |_, _| Rgb([200, 85, 61]));
        img.save(&jpeg).expect("write jpg");

        let s = std::process::Command::new("sips")
            .args(["-s", "format", "heic"])
            .arg(&jpeg)
            .arg("--out")
            .arg(&heic)
            .status();
        let s = match s {
            Ok(s) => s,
            Err(_) => {
                eprintln!("sips not available — skipping HEIC test");
                return;
            }
        };
        assert!(s.success(), "sips heic encode failed");
        assert!(heic.exists(), "heic source missing");

        let out = normalize_for_processing(&heic).expect("normalize");
        assert!(out.exists());
        assert_ne!(
            out.extension().unwrap().to_str().unwrap().to_lowercase(),
            "heic"
        );
        // The whole point: the `image` crate must be able to open it.
        let _ = image::open(&out).expect("image crate must decode normalized output");
    }

    #[test]
    fn jpeg_passes_through_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let jpeg = dir.path().join("p.jpg");
        ImageBuffer::<Rgb<u8>, _>::from_fn(8, 8, |_, _| Rgb([0, 0, 0]))
            .save(&jpeg)
            .unwrap();
        let out = normalize_for_processing(&jpeg).unwrap();
        assert_eq!(out, jpeg);
    }
}
