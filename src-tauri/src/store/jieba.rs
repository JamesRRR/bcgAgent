use jieba_rs::Jieba;
use once_cell::sync::Lazy;

static JIEBA: Lazy<Jieba> = Lazy::new(Jieba::new);

/// Tokenize Chinese text with jieba and join cuts with single spaces so the
/// FTS5 `unicode61` tokenizer (which splits on whitespace) can index them.
/// Latin tokens pass through unchanged because jieba keeps them as-is.
pub fn tokenize_zh(text: &str) -> String {
    JIEBA.cut(text, false).join(" ")
}
