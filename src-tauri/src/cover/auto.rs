//! Auto-set cover for a game after ingest:
//! 1. Try BoardGameGeek by name (zh, then en)
//! 2. Fall back to the first imported page's thumbnail
//! 3. Update games.cover_path

use std::path::PathBuf;

use crate::cover::bgg;
use crate::error::{AppError, AppResult};
use crate::paths;
use crate::store::{games as store_games, pages as store_pages, Db};

/// Pick a cover for `game_id` if none is set yet. Logs and ignores all errors —
/// failure to set a cover never fails the import.
pub async fn auto_set_cover(db: &Db, game_id: &str) -> AppResult<()> {
    let game = match store_games::get_game(db, game_id)? {
        Some(g) => g,
        None => return Ok(()),
    };
    if game.cover_path.is_some() {
        return Ok(());
    }

    let cover_dir = paths::games_dir().join(game_id);
    std::fs::create_dir_all(&cover_dir)?;

    // Step 1: BGG by zh name, then en name.
    let queries: Vec<&str> = std::iter::once(game.name_zh.as_str())
        .chain(game.name_en.as_deref())
        .filter(|s| !s.trim().is_empty())
        .collect();

    for q in queries {
        match bgg::search(q).await {
            Ok(Some(m)) => {
                tracing::info!("bgg match for {q:?}: id={} name={:?}", m.id, m.name);
                match bgg::fetch_cover(m.id).await {
                    Ok(Some(bytes)) => {
                        let dst = cover_dir.join("cover.jpg");
                        if let Err(e) = std::fs::write(&dst, &bytes) {
                            tracing::warn!("write bgg cover failed: {e}");
                            break;
                        }
                        store_games::set_cover(db, game_id, &dst.to_string_lossy())?;
                        return Ok(());
                    }
                    Ok(None) => {
                        tracing::info!("bgg id {} has no image, falling through", m.id);
                    }
                    Err(e) => {
                        tracing::warn!("bgg fetch cover error: {e}");
                    }
                }
                break;
            }
            Ok(None) => continue,
            Err(e) => {
                tracing::warn!("bgg search error for {q:?}: {e}");
                break;
            }
        }
    }

    // Step 2: first-page thumbnail fallback.
    let pages = store_pages::list_pages_by_game(db, game_id)?;
    let first_thumb = pages
        .iter()
        .find_map(|p| p.thumb_path.as_deref().filter(|s| !s.is_empty()));
    if let Some(thumb) = first_thumb {
        let src = PathBuf::from(thumb);
        let ext = src.extension().and_then(|s| s.to_str()).unwrap_or("webp");
        let dst = cover_dir.join(format!("cover.{ext}"));
        if let Err(e) = std::fs::copy(&src, &dst) {
            return Err(AppError::Other(anyhow::anyhow!(
                "copy first-page thumb to cover: {e}"
            )));
        }
        store_games::set_cover(db, game_id, &dst.to_string_lossy())?;
        tracing::info!("set first-page thumb as cover for game {game_id}");
    }
    Ok(())
}

/// Copy a user-chosen image into the game folder and persist as cover.
/// Used by the "更换封面" UI override.
pub fn set_cover_from_file(db: &Db, game_id: &str, src_path: &str) -> AppResult<String> {
    let src = PathBuf::from(src_path);
    if !src.exists() {
        return Err(AppError::Other(anyhow::anyhow!(
            "cover source does not exist: {src_path}"
        )));
    }
    let ext = src
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| "jpg".into());
    let cover_dir = paths::games_dir().join(game_id);
    std::fs::create_dir_all(&cover_dir)?;
    let dst = cover_dir.join(format!("cover.{ext}"));
    std::fs::copy(&src, &dst)?;
    let dst_str = dst.to_string_lossy().to_string();
    store_games::set_cover(db, game_id, &dst_str)?;
    Ok(dst_str)
}
