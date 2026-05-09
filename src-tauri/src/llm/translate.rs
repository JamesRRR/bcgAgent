//! Wave 3 — Chinese translation utility.
//!
//! Used by the research orchestrator (translate fetched English markdown
//! before chunking) and by the structured extractors (translate any English
//! lifted from rulebook OCR).
//!
//! The public entrypoint short-circuits when the input is already Chinese,
//! so callers can pass arbitrary mixed-language text without paying for
//! redundant LLM calls.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::error::AppResult;
use crate::llm::minimax::{chat_completion, ChatOptions, Message};

/// Hard upper bound (chars) for any single translation call. Inputs above
/// this are split into paragraph-bounded slices and translated independently.
const MAX_TRANSLATION_CHARS: usize = 8000;

/// Threshold above which we declare the input "already Chinese" and skip the
/// network round-trip. Counts ratio of CJK ideograph codepoints.
const HAN_RATIO_THRESHOLD: f32 = 0.5;

#[derive(Debug, Clone)]
pub struct TranslateRequest<'a> {
    pub text: &'a str,
    pub source_lang_hint: Option<&'a str>,
    pub domain: Option<&'a str>,
}

/// Type alias for an injectable chat function (so tests can substitute a
/// canned response without hitting the network).
pub type ChatFn = Arc<
    dyn Fn(Vec<Message>, ChatOptions) -> Pin<Box<dyn Future<Output = AppResult<String>> + Send>>
        + Send
        + Sync,
>;

fn default_chat_fn() -> ChatFn {
    Arc::new(|messages, opts| Box::pin(chat_completion(messages, opts)))
}

/// True iff the share of Han codepoints in `text` exceeds
/// `HAN_RATIO_THRESHOLD` of all letter-or-digit characters. Whitespace and
/// punctuation are ignored. Empty / all-symbol inputs return false.
pub fn looks_like_chinese(text: &str) -> bool {
    let mut letters = 0usize;
    let mut han = 0usize;
    for c in text.chars() {
        if c.is_alphanumeric() {
            letters += 1;
            // CJK Unified Ideographs + extension A. Sufficient for our use.
            if ('\u{4E00}'..='\u{9FFF}').contains(&c)
                || ('\u{3400}'..='\u{4DBF}').contains(&c)
            {
                han += 1;
            }
        }
    }
    if letters == 0 {
        return false;
    }
    (han as f32) / (letters as f32) >= HAN_RATIO_THRESHOLD
}

fn build_system_prompt(domain: Option<&str>) -> String {
    let mut s = String::from(
        "你是桌游规则的专业中文翻译。把下面的英文内容翻译成简体中文，保留所有专有名词的原文（用括号注明），不要添加任何解释或前后缀。",
    );
    if let Some(d) = domain {
        if !d.trim().is_empty() {
            s.push_str("\n领域：");
            s.push_str(d);
        }
    }
    s
}

/// Split `text` on paragraph boundaries (`\n\n`) into slices each below
/// `MAX_TRANSLATION_CHARS`. If a single paragraph exceeds the cap, it is
/// returned as-is — the LLM is forgiving enough that an oversized paragraph
/// still produces useful output, and we'd rather not split mid-sentence.
fn split_for_translation(text: &str) -> Vec<String> {
    if text.chars().count() <= MAX_TRANSLATION_CHARS {
        return vec![text.to_string()];
    }
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for para in text.split("\n\n") {
        let p = para.trim_end();
        if p.is_empty() {
            continue;
        }
        if cur.chars().count() + p.chars().count() + 2 > MAX_TRANSLATION_CHARS && !cur.is_empty() {
            out.push(std::mem::take(&mut cur));
        }
        if !cur.is_empty() {
            cur.push_str("\n\n");
        }
        cur.push_str(p);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Translate `text` to simplified Chinese via MiniMax. Skips the network
/// when `text` is already Chinese (per `looks_like_chinese`). Empty input
/// returns an empty string. Errors are bubbled up; callers decide whether
/// to fall back to the original.
pub async fn translate_to_chinese(req: TranslateRequest<'_>) -> AppResult<String> {
    translate_to_chinese_with(req, default_chat_fn()).await
}

/// Test-friendly variant — accepts an injected `ChatFn` so unit tests can
/// short-circuit the HTTP call.
pub async fn translate_to_chinese_with(
    req: TranslateRequest<'_>,
    chat_fn: ChatFn,
) -> AppResult<String> {
    if req.text.trim().is_empty() {
        return Ok(String::new());
    }
    if looks_like_chinese(req.text) {
        return Ok(req.text.to_string());
    }

    let system = build_system_prompt(req.domain);
    let pieces = split_for_translation(req.text);

    let mut out = String::new();
    for piece in pieces {
        let messages = vec![
            Message {
                role: "system".into(),
                content: system.clone(),
            },
            Message {
                role: "user".into(),
                content: piece,
            },
        ];
        let opts = ChatOptions {
            temperature: 0.2,
            max_tokens: 2048,
        };
        let result = (chat_fn)(messages, opts).await?;
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(result.trim());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn canned_chat(response: &'static str, count: Arc<AtomicUsize>) -> ChatFn {
        Arc::new(move |_msgs, _opts| {
            count.fetch_add(1, Ordering::SeqCst);
            let r = response.to_string();
            Box::pin(async move { Ok(r) })
        })
    }

    #[test]
    fn looks_like_chinese_detects_zh_strings() {
        assert!(looks_like_chinese("骑士的攻击力是2点"));
        // Han codepoints dominate even with some Latin punctuation/numbers.
        assert!(looks_like_chinese("骑士的攻击力 2 点，防御力 1 点。"));
        // Predominantly English → not Chinese.
        assert!(!looks_like_chinese("The knight has 2 attack points"));
        assert!(!looks_like_chinese(""));
    }

    #[tokio::test]
    async fn skip_network_when_already_chinese() {
        let count = Arc::new(AtomicUsize::new(0));
        let chat = canned_chat("不应该被调用", count.clone());
        let req = TranslateRequest {
            text: "骑士可以远程攻击吗？不可以。",
            source_lang_hint: Some("zh"),
            domain: Some("桌游规则"),
        };
        let out = translate_to_chinese_with(req, chat).await.unwrap();
        assert_eq!(out, "骑士可以远程攻击吗？不可以。");
        assert_eq!(count.load(Ordering::SeqCst), 0, "must not call chat for zh input");
    }

    #[tokio::test]
    async fn empty_input_returns_empty() {
        let count = Arc::new(AtomicUsize::new(0));
        let chat = canned_chat("nope", count.clone());
        let req = TranslateRequest {
            text: "",
            source_lang_hint: None,
            domain: None,
        };
        let out = translate_to_chinese_with(req, chat).await.unwrap();
        assert!(out.is_empty());
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn english_input_routes_through_chat_fn() {
        let count = Arc::new(AtomicUsize::new(0));
        let chat = canned_chat("骑士的攻击力是2点。", count.clone());
        let req = TranslateRequest {
            text: "The knight has 2 attack points.",
            source_lang_hint: Some("en"),
            domain: Some("桌游规则"),
        };
        let out = translate_to_chinese_with(req, chat).await.unwrap();
        assert!(out.contains("骑士"));
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn split_for_translation_breaks_long_inputs() {
        let para = "para text. ".repeat(200); // ~2200 chars
        let combined = format!("{}\n\n{}\n\n{}\n\n{}", para, para, para, para);
        let pieces = split_for_translation(&combined);
        // 4 paragraphs ~2200 each → should split into multiple chunks.
        assert!(pieces.len() > 1, "expected multi-piece split, got {}", pieces.len());
        for p in &pieces {
            assert!(p.chars().count() <= MAX_TRANSLATION_CHARS + 2200);
        }
    }
}
