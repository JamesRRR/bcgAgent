//! Components extractor.
//!
//! Input: rulebook markdown (concatenated `pages.ocr_markdown`) + the game's
//! `page_illustrations` rows. Output: rows in `components` + matching
//! chunks tagged `extracted_components`.
//!
//! Idempotent: clears the game's components rows before re-inserting.

use std::collections::HashMap;

use serde::Deserialize;

use super::{
    chunk_and_embed_extracted, default_chat_fn, default_embed_fn, strip_json_fences,
    ExtractSummary, ExtractorChatFn, ExtractorEmbedFn,
};
use crate::error::AppResult;
use crate::llm::minimax::{ChatOptions, Message};
use crate::store::{components as store_components, illustrations as store_ill, pages as store_pages, Db};

const MAX_COMPONENTS: usize = 60;
const SOURCE_KIND: &str = "extracted_components";
const TRUST_TIER: &str = "publisher";

#[derive(Debug, Deserialize)]
struct RawComponent {
    #[serde(default)]
    name_zh: String,
    #[serde(default)]
    category: Option<String>,
    #[serde(default)]
    effect_zh: Option<String>,
    #[serde(default)]
    page_no: Option<i64>,
    #[serde(default)]
    illustration_label_or_null: Option<String>,
}

/// Public entry — uses production chat + embed fns.
pub async fn extract_components(db: &Db, game_id: &str) -> AppResult<ExtractSummary> {
    extract_components_with(db, game_id, default_chat_fn(), default_embed_fn()).await
}

/// Test-friendly variant.
pub async fn extract_components_with(
    db: &Db,
    game_id: &str,
    chat_fn: ExtractorChatFn,
    embed_fn: ExtractorEmbedFn,
) -> AppResult<ExtractSummary> {
    let mut summary = ExtractSummary::default();

    // Pull pages + illustrations.
    let pages = store_pages::list_pages_by_game(db, game_id)?;
    if pages.is_empty() {
        return Ok(summary);
    }
    let illustrations = store_ill::list_for_game(db, game_id)?;

    // Build the rulebook context: header per page + its OCR markdown.
    let mut context = String::new();
    let mut has_text = false;
    let mut page_id_by_no: HashMap<i64, String> = HashMap::new();
    for p in &pages {
        page_id_by_no.insert(p.page_number, p.id.clone());
        if let Some(md) = p.ocr_markdown.as_deref() {
            let md = md.trim();
            if md.is_empty() {
                continue;
            }
            context.push_str(&format!("\n\n## 第 {} 页\n\n", p.page_number));
            context.push_str(md);
            has_text = true;
        }
    }
    if !has_text {
        return Ok(summary);
    }

    // Append illustration labels with their page numbers so the model can
    // reference them by name.
    let mut ill_by_label_page: HashMap<(i64, String), &store_ill::PageIllustration> =
        HashMap::new();
    if !illustrations.is_empty() {
        context.push_str("\n\n## 插图清单\n\n");
        for ill in &illustrations {
            let page_no = pages
                .iter()
                .find(|p| p.id == ill.page_id)
                .map(|p| p.page_number)
                .unwrap_or(-1);
            let label = ill.label.clone().unwrap_or_default();
            let token = ill.token.clone().unwrap_or_default();
            let desc = ill.description.clone().unwrap_or_default();
            context.push_str(&format!(
                "- 页 {} | 标签：{} | 标记：{} | 描述：{}\n",
                page_no, label, token, desc
            ));
            if !label.is_empty() {
                ill_by_label_page.insert((page_no, label.to_lowercase()), ill);
            }
        }
    }

    // Idempotent: clear before reinsert.
    store_components::clear_components_for_game(db, game_id)?;

    let system = "你是桌游规则的结构化抽取器。从下方规则书内容中识别所有具名的游戏组件（卡牌、棋子、骰子、地图板等），输出 JSON 数组。每个元素的字段：name_zh（必填，中文名）、category（取值：card | token | tile | board | dice | other）、effect_zh（这个组件的作用或规则的简要中文描述，可为空字符串）、page_no（这个组件第一次定义所在的页码整数，如不确定填 null）、illustration_label_or_null（如果在『插图清单』里有匹配项，写下其『标签』；否则填 null）。只输出合法 JSON 数组，不要写解释、不要写 markdown 代码块。最多 60 项。";
    let user = format!("以下是规则书与插图：\n\n{}", context);

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
        max_tokens: 3000,
    };
    let raw = (chat_fn)(messages, opts).await?;
    let json_slice = strip_json_fences(&raw);
    let parsed: Vec<RawComponent> = match serde_json::from_str(json_slice) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("components extractor: JSON parse failed: {e}; raw: {raw}");
            return Ok(summary);
        }
    };

    let mut combined_text = String::new();
    for (idx, item) in parsed.into_iter().enumerate() {
        if idx >= MAX_COMPONENTS {
            summary.dropped += 1;
            continue;
        }
        let name = item.name_zh.trim().to_string();
        if name.is_empty() {
            summary.dropped += 1;
            continue;
        }
        let category = item.category.as_deref().map(|s| s.trim().to_string());
        let effect = item.effect_zh.as_deref().map(|s| s.trim().to_string());

        let page_id = item
            .page_no
            .and_then(|n| page_id_by_no.get(&n).cloned());

        // Best-effort illustration match: same page + label match.
        let ill = item
            .illustration_label_or_null
            .as_ref()
            .and_then(|label| {
                let label_lc = label.trim().to_lowercase();
                if label_lc.is_empty() {
                    return None;
                }
                let pn = item.page_no.unwrap_or(-1);
                ill_by_label_page.get(&(pn, label_lc)).copied()
            });

        let bbox_json = ill.map(|i| {
            serde_json::json!({
                "x1": i.bbox_x1,
                "y1": i.bbox_y1,
                "x2": i.bbox_x2,
                "y2": i.bbox_y2,
            })
            .to_string()
        });

        let confidence = if ill.is_some() {
            0.85
        } else if page_id.is_some() {
            0.7
        } else {
            0.5
        };

        let new = store_components::NewComponent {
            name_zh: &name,
            category: category.as_deref(),
            effect_zh: effect.as_deref(),
            source_kind: SOURCE_KIND,
            source_url: None,
            page_id: page_id.as_deref(),
            bbox_json: bbox_json.as_deref(),
            illustration_id: ill.map(|i| i.id.as_str()),
            trust_tier: TRUST_TIER,
            confidence,
        };
        store_components::insert_component(db, game_id, &new)?;
        summary.created += 1;
        summary.kept += 1;

        let line = match &effect {
            Some(e) if !e.is_empty() => format!("- {}：{}", name, e),
            _ => format!("- {}", name),
        };
        combined_text.push_str(&line);
        combined_text.push('\n');
    }

    if !combined_text.trim().is_empty() {
        let payload = format!("# 游戏组件\n\n{}", combined_text);
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
    use crate::store::{games, pages};
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

    fn make_game_with_pages(db: &Db) -> String {
        let g = games::insert_game(db, "Test", None, None).unwrap();
        let p1 = pages::insert_page(db, &g, 1, "/tmp/p1.png", None).unwrap();
        let p2 = pages::insert_page(db, &g, 2, "/tmp/p2.png", None).unwrap();
        pages::set_ocr_result(db, &p1, "done", Some("# 第 1 页\n\n这里写着骑士卡的描述。"), None).unwrap();
        pages::set_ocr_result(db, &p2, "done", Some("# 第 2 页\n\n龙骑士登场。"), None).unwrap();
        g
    }

    #[tokio::test]
    async fn happy_path_parses_and_inserts() {
        let db = Db::open_in_memory().unwrap();
        let g = make_game_with_pages(&db);
        let canned = r#"```json
[
  {"name_zh": "骑士", "category": "card", "effect_zh": "攻击力2", "page_no": 1, "illustration_label_or_null": null},
  {"name_zh": "龙骑士", "category": "card", "effect_zh": "攻击力5", "page_no": 2, "illustration_label_or_null": null}
]
```"#;
        let summary = extract_components_with(&db, &g, fake_chat(canned), synth_embed())
            .await
            .unwrap();
        assert_eq!(summary.created, 2);
        let listed = store_components::list_components_for_game(&db, &g).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].name_zh, "骑士");
        assert!(listed[0].page_id.is_some(), "page resolved → has page_id");
        // chunks_added > 0
        assert!(summary.chunks_added >= 1);
    }

    #[tokio::test]
    async fn idempotent_clears_before_reinsert() {
        let db = Db::open_in_memory().unwrap();
        let g = make_game_with_pages(&db);
        let canned = r#"[{"name_zh": "骑士", "category": "card", "page_no": 1, "illustration_label_or_null": null}]"#;
        let s1 = extract_components_with(&db, &g, fake_chat(canned), synth_embed())
            .await
            .unwrap();
        let s2 = extract_components_with(&db, &g, fake_chat(canned), synth_embed())
            .await
            .unwrap();
        assert_eq!(s1.created, 1);
        assert_eq!(s2.created, 1);
        let listed = store_components::list_components_for_game(&db, &g).unwrap();
        assert_eq!(listed.len(), 1, "second run replaced first run, not appended");
    }

    #[tokio::test]
    async fn no_pages_short_circuits() {
        let db = Db::open_in_memory().unwrap();
        let g = games::insert_game(&db, "Empty", None, None).unwrap();
        let summary = extract_components_with(
            &db,
            &g,
            fake_chat("[{\"name_zh\":\"X\"}]"),
            synth_embed(),
        )
        .await
        .unwrap();
        assert_eq!(summary.created, 0);
    }

    #[tokio::test]
    async fn malformed_json_returns_empty_summary() {
        let db = Db::open_in_memory().unwrap();
        let g = make_game_with_pages(&db);
        let summary =
            extract_components_with(&db, &g, fake_chat("not even json"), synth_embed())
                .await
                .unwrap();
        assert_eq!(summary.created, 0);
        assert_eq!(summary.kept, 0);
    }
}
