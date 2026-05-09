//! FAQ extractor.
//!
//! Input: BGG forum threads stored in `chunks` (source_kind = `bgg_forum`).
//! Output: rows in `faq_pairs` + matching chunks tagged `extracted_faq`.
//!
//! Threads with no extractable Q&A are silently skipped. Idempotent: clears
//! the game's faq rows before re-extracting. Cap of 30 Q&A pairs per game.

use serde::Deserialize;

use super::{
    chunk_and_embed_extracted, default_chat_fn, default_embed_fn, strip_json_fences,
    ExtractSummary, ExtractorChatFn, ExtractorEmbedFn,
};
use crate::error::AppResult;
use crate::llm::minimax::{ChatOptions, Message};
use crate::store::{db::Db, faq_pairs as store_faq};

const MAX_FAQ_PAIRS: usize = 30;
const SOURCE_KIND_DB: &str = "bgg_forum";
const SOURCE_KIND_CHUNK: &str = "extracted_faq";
const TRUST_TIER: &str = "community";

#[derive(Debug, Deserialize)]
struct RawFaq {
    #[serde(default)]
    question_zh: String,
    #[serde(default)]
    answer_zh: String,
    #[serde(default)]
    source_url: Option<String>,
}

/// Pull every (source_url, content) pair for the game's bgg_forum chunks.
/// Returns a vector of `(url, concatenated_thread_text)` deduped by URL.
fn load_threads(db: &Db, game_id: &str) -> AppResult<Vec<(String, String)>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT COALESCE(source_url, ''), content FROM chunks \
         WHERE game_id = ? AND source_kind = 'bgg_forum' \
         ORDER BY id ASC",
    )?;
    let rows: Vec<(String, String)> = stmt
        .query_map(rusqlite::params![game_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    drop(conn);

    // Group by URL. Empty URLs roll up under a single "no-url" bucket.
    use std::collections::BTreeMap;
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    for (url, body) in rows {
        let entry = map.entry(url).or_default();
        if !entry.is_empty() {
            entry.push_str("\n\n");
        }
        entry.push_str(&body);
    }
    Ok(map.into_iter().collect())
}

pub async fn extract_faqs(db: &Db, game_id: &str) -> AppResult<ExtractSummary> {
    extract_faqs_with(db, game_id, default_chat_fn(), default_embed_fn()).await
}

pub async fn extract_faqs_with(
    db: &Db,
    game_id: &str,
    chat_fn: ExtractorChatFn,
    embed_fn: ExtractorEmbedFn,
) -> AppResult<ExtractSummary> {
    let mut summary = ExtractSummary::default();
    let threads = load_threads(db, game_id)?;
    if threads.is_empty() {
        return Ok(summary);
    }

    // Idempotent: clear before reinsert.
    store_faq::clear_faqs_for_game(db, game_id)?;

    let system = "你是桌游论坛资料的结构化整理器。下面会给你一段 BGG 论坛上的提问与回答。请抽取其中关于规则澄清的有意义问答对，输出 JSON 数组。每个元素：question_zh（用户的核心提问，简体中文，简洁一句话）、answer_zh（综合回答，简体中文，1-3 句话），source_url（如未给定则填 null）。忽略闲聊、推荐、感想、与规则无关的内容。如果整段都没有规则问答，返回空数组 []。只输出合法 JSON 数组，不要加 markdown 代码块或解释。";

    let mut all_text = String::new();
    let mut total_inserted = 0u32;
    for (url, body) in threads {
        if total_inserted as usize >= MAX_FAQ_PAIRS {
            break;
        }
        if body.trim().is_empty() {
            continue;
        }
        let user = if url.is_empty() {
            format!("论坛主题（无 URL）：\n\n{}", body)
        } else {
            format!("论坛主题（{}）：\n\n{}", url, body)
        };
        let messages = vec![
            Message {
                role: "system".into(),
                content: system.into(),
            },
            Message {
                role: "user".into(),
                content: user,
            },
        ];
        let opts = ChatOptions {
            temperature: 0.2,
            max_tokens: 2048,
        };
        let raw = match (chat_fn)(messages, opts).await {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("faq extractor: chat failed for {url}: {e}");
                continue;
            }
        };
        let json_slice = strip_json_fences(&raw);
        let parsed: Vec<RawFaq> = match serde_json::from_str(json_slice) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("faq extractor: JSON parse failed for {url}: {e}; raw: {raw}");
                continue;
            }
        };
        if parsed.is_empty() {
            continue;
        }
        for item in parsed {
            if total_inserted as usize >= MAX_FAQ_PAIRS {
                summary.dropped += 1;
                break;
            }
            let q = item.question_zh.trim().to_string();
            let a = item.answer_zh.trim().to_string();
            if q.is_empty() || a.is_empty() {
                summary.dropped += 1;
                continue;
            }
            let resolved_url = item
                .source_url
                .filter(|s| !s.trim().is_empty())
                .or_else(|| if url.is_empty() { None } else { Some(url.clone()) });
            let now = time::OffsetDateTime::now_utc().unix_timestamp();
            let new = store_faq::NewFaqPair {
                question_zh: &q,
                answer_zh: &a,
                source_kind: SOURCE_KIND_DB,
                source_url: resolved_url.as_deref(),
                trust_tier: TRUST_TIER,
                official: false,
                confidence: 0.6,
                fetched_at: Some(now),
            };
            store_faq::insert_faq_pair(db, game_id, &new)?;
            summary.created += 1;
            summary.kept += 1;
            total_inserted += 1;

            all_text.push_str(&format!("Q: {}\nA: {}\n\n", q, a));
        }
    }

    if !all_text.trim().is_empty() {
        let payload = format!("# 玩家 FAQ\n\n{}", all_text.trim());
        let n = chunk_and_embed_extracted(
            db,
            game_id,
            SOURCE_KIND_CHUNK,
            TRUST_TIER,
            &payload,
            None,
            &embed_fn,
        )?;
        summary.chunks_added += n;
    }

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{
        chunks as store_chunks, games, pages,
    };
    use std::sync::Arc;

    fn fake_chat(canned: &'static str) -> ExtractorChatFn {
        Arc::new(move |_msgs, _opts| {
            let r = canned.to_string();
            Box::pin(async move { Ok(r) })
        })
    }

    fn synth_embed() -> ExtractorEmbedFn {
        Arc::new(|texts: &[String]| Ok(texts.iter().map(|_| vec![0.0f32; 1024]).collect()))
    }

    fn synthetic_vec() -> Vec<f32> {
        vec![0.0f32; 1024]
    }

    fn seed_forum_thread(db: &Db, game_id: &str, url: &str, body: &str) {
        let pages = pages::list_pages_by_game(db, game_id).unwrap();
        let anchor = pages.first().map(|p| p.id.clone()).unwrap();
        let prov = store_chunks::ChunkProvenance {
            source_kind: "bgg_forum",
            source_url: Some(url),
            trust_tier: "community",
            official: false,
            confidence: 0.9,
            fetched_at: Some(0),
            content_lang: "zh",
            content_orig: None,
        };
        store_chunks::insert_chunk_with_embedding_and_provenance(
            db,
            &anchor,
            game_id,
            Some("BGG 论坛"),
            body,
            body.chars().count() as i64,
            &synthetic_vec(),
            &prov,
        )
        .unwrap();
    }

    fn make_game(db: &Db) -> String {
        let g = games::insert_game(db, "FaqGame", None, None).unwrap();
        pages::insert_page(db, &g, 1, "/tmp/p.png", None).unwrap();
        g
    }

    #[tokio::test]
    async fn happy_path_inserts_pairs_from_thread() {
        let db = Db::open_in_memory().unwrap();
        let g = make_game(&db);
        seed_forum_thread(&db, &g, "https://bgg/t/1", "用户问骑士能远程攻击吗？回答：不能。");
        let canned = r#"[{"question_zh":"骑士能否远程攻击?","answer_zh":"不能。","source_url":"https://bgg/t/1"}]"#;
        let summary = extract_faqs_with(&db, &g, fake_chat(canned), synth_embed())
            .await
            .unwrap();
        assert_eq!(summary.created, 1);
        let listed = store_faq::list_faqs_for_game(&db, &g).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].source_url.as_deref(), Some("https://bgg/t/1"));
        assert_eq!(listed[0].source_kind, "bgg_forum");
        assert_eq!(listed[0].trust_tier, "community");
        assert!(!listed[0].official);
        assert!(summary.chunks_added >= 1);
    }

    #[tokio::test]
    async fn no_threads_short_circuits() {
        let db = Db::open_in_memory().unwrap();
        let g = make_game(&db);
        let summary = extract_faqs_with(&db, &g, fake_chat("[]"), synth_embed())
            .await
            .unwrap();
        assert_eq!(summary.created, 0);
    }

    #[tokio::test]
    async fn idempotent_replaces_previous_rows() {
        let db = Db::open_in_memory().unwrap();
        let g = make_game(&db);
        seed_forum_thread(&db, &g, "https://bgg/t/2", "Q&A 内容");
        let canned = r#"[{"question_zh":"X?","answer_zh":"Y."}]"#;
        let _ = extract_faqs_with(&db, &g, fake_chat(canned), synth_embed())
            .await
            .unwrap();
        let _ = extract_faqs_with(&db, &g, fake_chat(canned), synth_embed())
            .await
            .unwrap();
        let listed = store_faq::list_faqs_for_game(&db, &g).unwrap();
        assert_eq!(listed.len(), 1);
    }

    #[tokio::test]
    async fn empty_array_yields_zero_inserts() {
        let db = Db::open_in_memory().unwrap();
        let g = make_game(&db);
        seed_forum_thread(&db, &g, "https://bgg/t/3", "完全不是规则相关。");
        let summary = extract_faqs_with(&db, &g, fake_chat("[]"), synth_embed())
            .await
            .unwrap();
        assert_eq!(summary.created, 0);
    }
}
