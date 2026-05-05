use jieba_rs::Jieba;
use once_cell::sync::Lazy;

static JIEBA: Lazy<Jieba> = Lazy::new(Jieba::new);

/// Indexing-side tokenization: use search-mode cut so compound words like
/// "弗雷曼人羁绊" are stored as MULTIPLE overlapping tokens
/// (e.g. "弗雷曼", "羁绊", "弗雷曼人羁绊"). This is the only way a query
/// for "羁绊" can find content that jieba's precise mode would otherwise
/// hide inside a longer compound.
pub fn tokenize_for_index(text: &str) -> String {
    JIEBA.cut_for_search(text, true).join(" ")
}

/// Query-side tokenization: precise mode produces the user's intended
/// terms without expansion, so a deliberate query for a long phrase still
/// matches as a phrase rather than dissolving into overly broad tokens.
pub fn tokenize_for_query(text: &str) -> String {
    JIEBA.cut(text, false).join(" ")
}

/// Back-compat alias for older call sites; defaults to indexing mode.
#[allow(dead_code)]
pub fn tokenize_zh(text: &str) -> String {
    tokenize_for_index(text)
}
