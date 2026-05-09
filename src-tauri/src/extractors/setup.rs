//! Setup-steps extractor.
//!
//! Input: rulebook chunks (heading_path containing "Setup" / "设置" / "准备").
//! Falls back to all rulebook chunks (`source_kind = photo_ocr`) if no
//! setup-tagged chunks are found.
//!
//! Output: rows in `setup_steps` + matching chunks tagged `extracted_setup`.
//! Idempotent: clears the game's setup rows before re-inserting. Cap of 30
//! steps per game.

use serde::Deserialize;

use super::{
    chunk_and_embed_extracted, default_chat_fn, default_embed_fn, strip_json_fences,
    ExtractSummary, ExtractorChatFn, ExtractorEmbedFn,
};
use crate::error::AppResult;
use crate::llm::minimax::{ChatOptions, Message};
use crate::store::{db::Db, setup_steps as store_setup};

const MAX_SETUP_STEPS: usize = 30;
const SOURCE_KIND: &str = "extracted_setup";
const TRUST_TIER: &str = "publisher";

#[derive(Debug, Deserialize)]
struct RawStep {
    #[serde(default)]
    step_no: Option<i64>,
    #[serde(default)]
    player_count_or_null: Option<String>,
    #[serde(default)]
    text_zh: String,
    #[serde(default)]
    components_referenced: Option<serde_json::Value>,
}

/// Pull rulebook chunks for the game, prioritizing those with a setup-like
/// heading. Returns (heading_path, content) tuples.
fn load_setup_chunks(db: &Db, game_id: &str) -> AppResult<Vec<(Option<String>, String)>> {
    let conn = db.lock();
    let mut stmt = conn.prepare(
        "SELECT heading_path, content FROM chunks \
         WHERE game_id = ? AND source_kind = 'photo_ocr' \
         ORDER BY id ASC",
    )?;
    let rows: Vec<(Option<String>, String)> = stmt
        .query_map(rusqlite::params![game_id], |r| {
            Ok((r.get::<_, Option<String>>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    drop(stmt);
    drop(conn);

    let setup_re = ["Setup", "setup", "SETUP", "设置", "准备"];
    let setup_only: Vec<_> = rows
        .iter()
        .cloned()
        .filter(|(h, _)| {
            h.as_deref()
                .map(|s| setup_re.iter().any(|m| s.contains(*m)))
                .unwrap_or(false)
        })
        .collect();
    if !setup_only.is_empty() {
        return Ok(setup_only);
    }
    Ok(rows)
}

pub async fn extract_setup(db: &Db, game_id: &str) -> AppResult<ExtractSummary> {
    extract_setup_with(db, game_id, default_chat_fn(), default_embed_fn()).await
}

pub async fn extract_setup_with(
    db: &Db,
    game_id: &str,
    chat_fn: ExtractorChatFn,
    embed_fn: ExtractorEmbedFn,
) -> AppResult<ExtractSummary> {
    let mut summary = ExtractSummary::default();
    let chunks = load_setup_chunks(db, game_id)?;
    if chunks.is_empty() {
        return Ok(summary);
    }

    // Compose the model context from setup-relevant chunks.
    let mut context = String::new();
    for (heading, body) in &chunks {
        if let Some(h) = heading {
            context.push_str(&format!("\n\n## {}\n\n", h));
        } else {
            context.push_str("\n\n## (无标题)\n\n");
        }
        context.push_str(body);
    }

    // Idempotent: clear before reinsert.
    store_setup::clear_setup_steps_for_game(db, game_id)?;

    let system = "你是桌游规则书的结构化抽取器。从下方『准备 / 设置』段落里抽取按顺序排列的设置步骤。输出 JSON 数组，每个元素：step_no（步骤编号，从 1 开始的整数）、player_count_or_null（人数限制，例如 null（任意人数）、\"2\"、\"3-4\"、\"5+\"）、text_zh（步骤说明，简体中文，1-3 句话）、components_referenced（这个步骤涉及到的组件名，字符串数组，可为空 []）。最多 30 步。只输出合法 JSON 数组，不要 markdown 代码块或解释。";

    let user = format!("规则书相关段落：\n{}", context);
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
    let raw = (chat_fn)(messages, opts).await?;
    let json_slice = strip_json_fences(&raw);
    let parsed: Vec<RawStep> = match serde_json::from_str(json_slice) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("setup extractor: JSON parse failed: {e}; raw: {raw}");
            return Ok(summary);
        }
    };

    let mut combined_text = String::new();
    for (idx, item) in parsed.into_iter().enumerate() {
        if idx >= MAX_SETUP_STEPS {
            summary.dropped += 1;
            continue;
        }
        let text = item.text_zh.trim().to_string();
        if text.is_empty() {
            summary.dropped += 1;
            continue;
        }
        let step_no = item.step_no.unwrap_or((idx + 1) as i64);
        let player_count = item
            .player_count_or_null
            .filter(|s| !s.trim().is_empty() && s.trim().to_lowercase() != "null");
        let component_ids = item
            .components_referenced
            .as_ref()
            .map(|v| v.to_string());

        let new = store_setup::NewSetupStep {
            step_no,
            player_count: player_count.as_deref(),
            text_zh: &text,
            component_ids: component_ids.as_deref(),
            source_kind: SOURCE_KIND,
            source_url: None,
            page_id: None,
            trust_tier: TRUST_TIER,
            confidence: 0.9,
        };
        store_setup::insert_setup_step(db, game_id, &new)?;
        summary.created += 1;
        summary.kept += 1;

        let pc = match &player_count {
            Some(p) => format!("（{} 人）", p),
            None => String::new(),
        };
        combined_text.push_str(&format!("[设置步骤 {}{}] {}\n", step_no, pc, text));
    }

    if !combined_text.trim().is_empty() {
        let payload = format!("# 游戏准备步骤\n\n{}", combined_text.trim());
        let n = chunk_and_embed_extracted(
            db,
            game_id,
            SOURCE_KIND,
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
    use crate::store::{chunks as store_chunks, games, pages};
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

    fn seed_chunk(db: &Db, game_id: &str, heading: Option<&str>, content: &str) {
        let pages = pages::list_pages_by_game(db, game_id).unwrap();
        let anchor = pages.first().map(|p| p.id.clone()).unwrap();
        let prov = store_chunks::ChunkProvenance::photo_ocr();
        store_chunks::insert_chunk_with_embedding_and_provenance(
            db,
            &anchor,
            game_id,
            heading,
            content,
            content.chars().count() as i64,
            &synthetic_vec(),
            &prov,
        )
        .unwrap();
    }

    fn make_game_with_setup_chunk(db: &Db) -> String {
        let g = games::insert_game(db, "Setup", None, None).unwrap();
        pages::insert_page(db, &g, 1, "/tmp/p.png", None).unwrap();
        seed_chunk(db, &g, Some("Setup"), "把所有卡牌洗匀，每名玩家发 5 张。");
        seed_chunk(db, &g, Some("Combat"), "战斗规则与设置无关。");
        g
    }

    #[tokio::test]
    async fn happy_path_inserts_steps() {
        let db = Db::open_in_memory().unwrap();
        let g = make_game_with_setup_chunk(&db);
        let canned = r#"[
          {"step_no": 1, "player_count_or_null": null, "text_zh": "洗牌", "components_referenced": ["卡牌"]},
          {"step_no": 2, "player_count_or_null": "3-4", "text_zh": "每人 5 张", "components_referenced": ["卡牌"]}
        ]"#;
        let summary = extract_setup_with(&db, &g, fake_chat(canned), synth_embed())
            .await
            .unwrap();
        assert_eq!(summary.created, 2);
        let listed = store_setup::list_setup_steps_for_game(&db, &g).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].step_no, 1);
        assert_eq!(listed[1].player_count.as_deref(), Some("3-4"));
        assert!(summary.chunks_added >= 1);
    }

    #[tokio::test]
    async fn idempotent_replaces_previous_steps() {
        let db = Db::open_in_memory().unwrap();
        let g = make_game_with_setup_chunk(&db);
        let canned = r#"[{"step_no":1,"player_count_or_null":null,"text_zh":"洗牌","components_referenced":[]}]"#;
        let _ = extract_setup_with(&db, &g, fake_chat(canned), synth_embed())
            .await
            .unwrap();
        let _ = extract_setup_with(&db, &g, fake_chat(canned), synth_embed())
            .await
            .unwrap();
        let listed = store_setup::list_setup_steps_for_game(&db, &g).unwrap();
        assert_eq!(listed.len(), 1);
    }

    #[tokio::test]
    async fn falls_back_to_all_chunks_when_no_setup_heading() {
        let db = Db::open_in_memory().unwrap();
        let g = games::insert_game(&db, "Fallback", None, None).unwrap();
        pages::insert_page(&db, &g, 1, "/tmp/p.png", None).unwrap();
        seed_chunk(&db, &g, Some("游戏目的"), "目标是收集 10 个金币。");
        let canned = r#"[{"step_no":1,"player_count_or_null":null,"text_zh":"准备金币堆","components_referenced":["金币"]}]"#;
        let summary = extract_setup_with(&db, &g, fake_chat(canned), synth_embed())
            .await
            .unwrap();
        assert_eq!(summary.created, 1);
    }

    #[tokio::test]
    async fn no_chunks_short_circuits() {
        let db = Db::open_in_memory().unwrap();
        let g = games::insert_game(&db, "Empty", None, None).unwrap();
        let summary = extract_setup_with(&db, &g, fake_chat("[]"), synth_embed())
            .await
            .unwrap();
        assert_eq!(summary.created, 0);
    }
}
