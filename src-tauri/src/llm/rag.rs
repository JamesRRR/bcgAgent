use crate::llm::minimax::Message;
use crate::llm::prompts::SYSTEM_PROMPT_ZH;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct RetrievedChunk {
    pub chunk_id: i64,
    pub game_name: String,
    pub page_number: i64,
    pub heading_path: Option<String>,
    pub content: String,
    pub fused_score: f32,
}

/// Tier multiplier used for the confidence score. Mirrors the spec's
/// "tier_weights" table:
///
/// | tier        | weight |
/// | ----------- | ------ |
/// | publisher   | 1.0    |
/// | designer    | 0.9    |
/// | community   | 0.7    |
/// | unverified  | 0.5    |
pub fn tier_weight(tier: &str) -> f32 {
    match tier {
        "publisher" => 1.0,
        "designer" => 0.9,
        "community" => 0.7,
        "unverified" => 0.5,
        _ => 0.5,
    }
}

/// Compute the ask-time confidence score for a single retrieved hit:
///
/// `0.6 * top_cosine + 0.3 * fts_rank_normalized + 0.1 * tier_weight`
///
/// All inputs are clamped to `[0,1]` so caller bugs can't produce negative
/// confidences. The retrieval layer takes the max over the top-K hits.
pub fn compute_confidence(top_cosine: f32, fts_rank_normalized: f32, trust_tier: &str) -> f32 {
    let c = top_cosine.clamp(0.0, 1.0);
    let f = fts_rank_normalized.clamp(0.0, 1.0);
    let t = tier_weight(trust_tier).clamp(0.0, 1.0);
    0.6 * c + 0.3 * f + 0.1 * t
}

/// Per-hit endorsement adjustment used by retrieval-time scoring (NOT
/// embedding). Adds +0.1 for thumbs-up, -0.2 for thumbs-down, 0 if unset.
pub fn endorsement_boost(endorsed: Option<bool>) -> f32 {
    match endorsed {
        Some(true) => 0.1,
        Some(false) => -0.2,
        None => 0.0,
    }
}

/// Reciprocal Rank Fusion of two ranked lists.
/// `vec_ranked` and `fts_ranked` are chunk ids ordered best-first.
/// Returns the top `top_n` chunk ids with their fused scores, best-first.
pub fn rrf(vec_ranked: &[i64], fts_ranked: &[i64], k: usize, top_n: usize) -> Vec<(i64, f32)> {
    let mut scores: HashMap<i64, f32> = HashMap::new();
    let kf = k as f32;

    for (rank, id) in vec_ranked.iter().enumerate() {
        let r = (rank + 1) as f32;
        *scores.entry(*id).or_insert(0.0) += 1.0 / (kf + r);
    }
    for (rank, id) in fts_ranked.iter().enumerate() {
        let r = (rank + 1) as f32;
        *scores.entry(*id).or_insert(0.0) += 1.0 / (kf + r);
    }

    let mut fused: Vec<(i64, f32)> = scores.into_iter().collect();
    fused.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    fused.truncate(top_n);
    fused
}

/// Build the message list for MiniMax: system prompt + user question with embedded chunks.
pub fn build_messages(question: &str, chunks: &[RetrievedChunk]) -> Vec<Message> {
    let mut user = String::from("以下是从规则书中检索到的相关片段：\n\n");
    for (i, c) in chunks.iter().enumerate() {
        user.push_str(&format!(
            "[{}] 《{}》 p.{}",
            i + 1,
            c.game_name,
            c.page_number
        ));
        if let Some(h) = &c.heading_path {
            if !h.is_empty() {
                user.push_str("  · ");
                user.push_str(h);
            }
        }
        user.push('\n');
        user.push_str(&c.content);
        user.push_str("\n\n");
    }
    user.push_str("请回答以下问题：\n");
    user.push_str(question);

    vec![
        Message {
            role: "system".into(),
            content: SYSTEM_PROMPT_ZH.into(),
        },
        Message {
            role: "user".into(),
            content: user,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rrf_smoke() {
        let vec_ranked = vec![10, 20, 30];
        let fts_ranked = vec![20, 40];
        let out = rrf(&vec_ranked, &fts_ranked, 60, 10);
        assert_eq!(
            out[0].0, 20,
            "chunk 20 appears in both lists, should be top"
        );
        let ids: Vec<i64> = out.iter().map(|(i, _)| *i).collect();
        assert!(ids.contains(&10));
        assert!(ids.contains(&30));
        assert!(ids.contains(&40));
    }

    #[test]
    fn confidence_formula_examples() {
        // All-zero inputs → tier-only contribution.
        let publisher_only = compute_confidence(0.0, 0.0, "publisher");
        assert!((publisher_only - 0.10).abs() < 1e-5);

        // Strong cosine + perfect fts + community: 0.6*0.9 + 0.3*1 + 0.1*0.7
        // = 0.54 + 0.30 + 0.07 = 0.91.
        let strong = compute_confidence(0.9, 1.0, "community");
        assert!((strong - 0.91).abs() < 1e-5, "got {strong}");

        // Below threshold case used in spec τ=0.45: weak cosine + weak fts.
        let weak = compute_confidence(0.4, 0.2, "unverified");
        // 0.6*0.4 + 0.3*0.2 + 0.1*0.5 = 0.24 + 0.06 + 0.05 = 0.35.
        assert!((weak - 0.35).abs() < 1e-5);
        assert!(weak < 0.45, "weak should be below default threshold");
    }

    #[test]
    fn endorsement_boost_signs() {
        assert!((endorsement_boost(Some(true)) - 0.1).abs() < 1e-6);
        assert!((endorsement_boost(Some(false)) + 0.2).abs() < 1e-6);
        assert!(endorsement_boost(None) == 0.0);
    }

    #[test]
    fn build_messages_shape() {
        let chunks = vec![
            RetrievedChunk {
                chunk_id: 1,
                game_name: "卡坦岛".into(),
                page_number: 5,
                heading_path: Some("建造规则".into()),
                content: "玩家可以建造城镇。".into(),
                fused_score: 0.9,
            },
            RetrievedChunk {
                chunk_id: 2,
                game_name: "Wingspan".into(),
                page_number: 12,
                heading_path: None,
                content: "Play a bird card.".into(),
                fused_score: 0.5,
            },
        ];
        let msgs = build_messages("城镇怎么建？", &chunks);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].role, "system");
        assert_eq!(msgs[1].role, "user");
        let u = &msgs[1].content;
        assert!(u.contains("[1]"));
        assert!(u.contains("[2]"));
        assert!(u.contains("卡坦岛"));
        assert!(u.contains("Wingspan"));
        assert!(u.contains("p.5"));
        assert!(u.contains("p.12"));
        assert!(u.contains("城镇怎么建？"));
        assert!(u.contains("建造规则"));
    }
}
