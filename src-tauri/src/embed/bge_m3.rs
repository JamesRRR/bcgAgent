//! Local text embeddings.
//!
//! NOTE ON MODEL CHOICE: the wave spec asks for BGE-M3, but `fastembed` 4.9.1
//! does not expose a BGE-M3 variant. To preserve the user's actual goal —
//! good Chinese + English coverage at 1024-d — we use
//! `EmbeddingModel::MultilingualE5Large` (intfloat/multilingual-e5-large,
//! 1024-d, multilingual). This is a deliberate deviation; if BGE-M3 becomes
//! available in a later fastembed release, swap the variant here.
//!
//! The 1024-d output matches the `chunks_vec` schema, so downstream code is
//! unaffected.
//!
//! First call to `embed_query` / `embed_batch` triggers the model download
//! (~1.3GB for E5-large) into `paths::bge_m3_dir()`. Subsequent calls reuse
//! the loaded model.

use crate::error::{AppError, AppResult};
use crate::paths;
use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
use once_cell::sync::OnceCell;
use parking_lot::Mutex;

/// Output dimension of the embedding model.
pub const EMBED_DIM: usize = 1024;

/// Embedding model used. See module docs for why this isn't BGE-M3.
const MODEL: EmbeddingModel = EmbeddingModel::MultilingualE5Large;

/// Approximate total bytes for the multilingual-e5-large model. Used only for
/// the UI progress hint when fastembed doesn't expose a real progress callback.
pub const MODEL_TOTAL_BYTES: u64 = 1_300_000_000;

static MODEL_CELL: OnceCell<Mutex<TextEmbedding>> = OnceCell::new();

fn model() -> AppResult<&'static Mutex<TextEmbedding>> {
    if let Some(m) = MODEL_CELL.get() {
        return Ok(m);
    }
    let cache_dir = paths::bge_m3_dir();
    std::fs::create_dir_all(&cache_dir).map_err(AppError::from)?;

    let opts = InitOptions::new(MODEL)
        .with_cache_dir(cache_dir)
        .with_show_download_progress(true);

    let embedder = TextEmbedding::try_new(opts)
        .map_err(|e| AppError::Embed(format!("init {MODEL:?}: {e}")))?;

    tracing::info!("multilingual-e5-large ready, dim={}", EMBED_DIM);

    let _ = MODEL_CELL.set(Mutex::new(embedder));
    Ok(MODEL_CELL.get().expect("just set"))
}

/// Has the embedding model already finished initializing?
pub fn is_ready() -> bool {
    MODEL_CELL.get().is_some()
}

/// Sum the bytes currently on disk under the model cache dir. Cheap-ish; we
/// poll this on a 2s tick during startup to give the UI a "still working"
/// signal even though `fastembed` doesn't expose true progress callbacks.
pub fn cache_size_bytes() -> u64 {
    fn dir_size(p: &std::path::Path) -> u64 {
        let mut total = 0u64;
        let Ok(rd) = std::fs::read_dir(p) else {
            return 0;
        };
        for entry in rd.flatten() {
            let Ok(ft) = entry.file_type() else { continue };
            if ft.is_file() {
                if let Ok(md) = entry.metadata() {
                    total = total.saturating_add(md.len());
                }
            } else if ft.is_dir() {
                total = total.saturating_add(dir_size(&entry.path()));
            }
        }
        total
    }
    dir_size(&paths::bge_m3_dir())
}

/// Block on full model initialization. Identical to the lazy path triggered
/// by `embed_batch`, but exposed so the app can warm the cache during
/// startup with the UI watching.
pub fn warm_up() -> AppResult<()> {
    model()?;
    Ok(())
}

/// Output dimension (1024).
pub fn dim() -> usize {
    EMBED_DIM
}

/// Embed a single query string.
pub fn embed_query(text: &str) -> AppResult<Vec<f32>> {
    let mut v = embed_batch(&[text.to_string()])?;
    v.pop()
        .ok_or_else(|| AppError::Embed("empty embedding result".into()))
}

/// Embed a batch of texts. Returns one 1024-d vector per input, in order.
pub fn embed_batch(texts: &[String]) -> AppResult<Vec<Vec<f32>>> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }
    let m = model()?;
    let guard = m.lock();
    let out = guard
        .embed(texts.to_vec(), None)
        .map_err(|e| AppError::Embed(format!("embed: {e}")))?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dim_is_1024() {
        assert_eq!(dim(), 1024);
    }

    #[test]
    #[ignore]
    fn embed_zh_en_returns_1024d() {
        let zh = embed_query("骑士的攻击力是2点").unwrap();
        let en = embed_query("the knight has 2 attack").unwrap();
        assert_eq!(zh.len(), 1024);
        assert_eq!(en.len(), 1024);
    }
}
