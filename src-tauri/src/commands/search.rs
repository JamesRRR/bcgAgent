use serde::Serialize;
use tauri::State;

use crate::error::{AppError, AppResult};
use crate::store::{chunks, games, pages, Db};

use super::AppState;

#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub chunk_id: i64,
    pub game_id: String,
    pub game_name: String,
    pub page_id: String,
    pub page_number: i64,
    pub heading_path: Option<String>,
    pub content: String,
    pub score: f32,
}

/// Hydrate `(chunk_id, score)` pairs into `SearchHit`s.
fn hydrate(db: &Db, hits: Vec<(i64, f32)>) -> AppResult<Vec<SearchHit>> {
    let mut out = Vec::with_capacity(hits.len());
    for (id, score) in hits {
        let chunk = match chunks::get_chunk(db, id)? {
            Some(c) => c,
            None => continue,
        };
        let page = match pages::get_page(db, &chunk.page_id)? {
            Some(p) => p,
            None => continue,
        };
        let game = match games::get_game(db, &chunk.game_id)? {
            Some(g) => g,
            None => continue,
        };
        out.push(SearchHit {
            chunk_id: chunk.id,
            game_id: chunk.game_id.clone(),
            game_name: game.name_zh,
            page_id: page.id,
            page_number: page.page_number,
            heading_path: chunk.heading_path,
            content: chunk.content,
            score,
        });
    }
    Ok(out)
}

#[tauri::command(rename_all = "snake_case")]
pub async fn search_keyword(
    state: State<'_, AppState>,
    query: String,
    game_id: Option<String>,
    k: usize,
) -> AppResult<Vec<SearchHit>> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || -> AppResult<Vec<SearchHit>> {
        let raw = chunks::fts_search(&db, &query, game_id.as_deref(), k)?;
        hydrate(&db, raw)
    })
    .await
    .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))?
}

#[tauri::command(rename_all = "snake_case")]
pub async fn search_semantic(
    state: State<'_, AppState>,
    query: String,
    game_id: Option<String>,
    k: usize,
) -> AppResult<Vec<SearchHit>> {
    let db = state.db.clone();
    tokio::task::spawn_blocking(move || -> AppResult<Vec<SearchHit>> {
        let qv = crate::embed::embed_query(&query)?;
        let raw = chunks::vec_search(&db, &qv, game_id.as_deref(), k)?;
        hydrate(&db, raw)
    })
    .await
    .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))?
}
