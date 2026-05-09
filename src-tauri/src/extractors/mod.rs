//! Wave 3 — structured extractors.
//!
//! Each extractor takes a game's existing rulebook + research material and
//! emits structured rows (components / faq_pairs / setup_steps). After
//! writing the structured rows it also chunks + embeds the structured output
//! back into `chunks` so the searchable mirror stays in sync. Idempotent:
//! the game's rows in the dedicated table are cleared before re-extracting.

pub mod components;
pub mod faq;
pub mod setup;

pub use components::extract_components;
pub use faq::extract_faqs;
pub use setup::extract_setup;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::commands::chunker;
use crate::error::{AppError, AppResult};
use crate::llm::minimax::{ChatOptions, Message};
use crate::store::{chunks as store_chunks, Db};

/// Chat hook reused by every extractor. Production wiring calls
/// `crate::llm::minimax::chat_completion`. Tests substitute a canned
/// response.
pub type ExtractorChatFn = Arc<
    dyn Fn(Vec<Message>, ChatOptions) -> Pin<Box<dyn Future<Output = AppResult<String>> + Send>>
        + Send
        + Sync,
>;

/// Embed hook with the same shape as `orchestrator::EmbedFn` — kept separate
/// so call sites import from one place per layer.
pub type ExtractorEmbedFn = Arc<dyn Fn(&[String]) -> AppResult<Vec<Vec<f32>>> + Send + Sync>;

pub fn default_chat_fn() -> ExtractorChatFn {
    Arc::new(|messages, opts| {
        Box::pin(crate::llm::minimax::chat_completion(messages, opts))
    })
}

pub fn default_embed_fn() -> ExtractorEmbedFn {
    Arc::new(|texts: &[String]| crate::embed::embed_batch(texts))
}

/// Aggregate counts returned by every extractor: how many rows were created,
/// how many candidates we kept (== created when idempotent), how many we
/// dropped (parse errors, over-cap, empty fields).
#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct ExtractSummary {
    pub created: u32,
    pub kept: u32,
    pub dropped: u32,
    pub chunks_added: u32,
}

/// Strip ```json``` / ``` fences if present and return the largest JSON
/// array substring (between the first `[` and last `]`). Returns the raw
/// input unchanged when no array delimiters exist — `serde_json` will then
/// produce a clear parse error.
pub fn strip_json_fences(s: &str) -> &str {
    let trimmed = s.trim();
    let trimmed = trimmed
        .strip_prefix("```json")
        .or_else(|| trimmed.strip_prefix("```"))
        .unwrap_or(trimmed);
    let trimmed = trimmed.trim_start_matches('\n');
    let trimmed = trimmed.strip_suffix("```").unwrap_or(trimmed).trim();
    // Pick the widest [...] / {...} bracket window so we tolerate the model
    // wrapping its array in commentary.
    let lb = trimmed.find('[');
    let rb = trimmed.rfind(']');
    if let (Some(a), Some(b)) = (lb, rb) {
        if b >= a {
            return &trimmed[a..=b];
        }
    }
    trimmed
}

/// Chunk + embed an extracted structured payload and write the rows into
/// `chunks`. Anchors to the game's first page (same convention as the
/// import-time pipeline). Returns the number of chunks inserted.
pub fn chunk_and_embed_extracted(
    db: &Db,
    game_id: &str,
    source_kind: &str,
    trust_tier: &str,
    text_zh: &str,
    source_url: Option<&str>,
    embed_fn: &ExtractorEmbedFn,
) -> AppResult<u32> {
    if text_zh.trim().is_empty() {
        return Ok(0);
    }
    let pages = crate::store::pages::list_pages_by_game(db, game_id)?;
    let anchor = match pages.first() {
        Some(p) => p.id.clone(),
        None => return Ok(0),
    };
    let chunked = chunker::chunk_markdown(text_zh);
    if chunked.is_empty() {
        return Ok(0);
    }
    let texts: Vec<String> = chunked.iter().map(|c| c.content.clone()).collect();
    let embeds = (embed_fn)(&texts)?;
    if embeds.len() != texts.len() {
        return Err(AppError::Other(anyhow::anyhow!(
            "embed_fn returned {} vectors for {} texts",
            embeds.len(),
            texts.len()
        )));
    }
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let official = trust_tier == "publisher" || trust_tier == "designer";
    let mut inserted = 0u32;
    for (chunk, vec) in chunked.iter().zip(embeds.iter()) {
        let prov = store_chunks::ChunkProvenance {
            source_kind,
            source_url,
            trust_tier,
            official,
            confidence: 0.85,
            fetched_at: Some(now),
            content_lang: "zh",
            content_orig: None,
        };
        store_chunks::insert_chunk_with_embedding_and_provenance(
            db,
            &anchor,
            game_id,
            chunk.heading_path.as_deref(),
            &chunk.content,
            chunk.token_count as i64,
            vec,
            &prov,
        )?;
        inserted += 1;
    }
    Ok(inserted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_json_fences_handles_fenced_block() {
        let s = "```json\n[{\"a\": 1}]\n```";
        assert_eq!(strip_json_fences(s), "[{\"a\": 1}]");
    }

    #[test]
    fn strip_json_fences_handles_plain_array() {
        let s = "[{\"a\": 1}]";
        assert_eq!(strip_json_fences(s), "[{\"a\": 1}]");
    }

    #[test]
    fn strip_json_fences_extracts_array_from_commentary() {
        let s = "Sure, here you go:\n[{\"a\": 1}]\nThat's all.";
        assert_eq!(strip_json_fences(s), "[{\"a\": 1}]");
    }

    #[test]
    fn strip_json_fences_passes_through_unbracketed_input() {
        let s = "no array here";
        assert_eq!(strip_json_fences(s), "no array here");
    }
}
