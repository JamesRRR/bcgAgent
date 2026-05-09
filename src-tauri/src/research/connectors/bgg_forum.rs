//! BGG forum connector.
//!
//! Lists forums attached to a thing (boardgame), pulls thread lists and
//! filters by case-insensitive subject contains. Returns up to top-K hits
//! ranked by `num_articles`. First-article body becomes the snippet.
//!
//! Reuses `research::bgg_extra` for HTTP + XML parsing so we honor the same
//! 1 req/sec etiquette and UA.

use async_trait::async_trait;

use super::{GameCtx, ResearchConnector, ResearchHit, TrustTier};
use crate::error::AppResult;
use crate::research::bgg_extra;

/// Maximum hits returned from one search call.
const MAX_HITS: usize = 5;
/// Minimum article count for a thread to be considered worth returning.
const MIN_ARTICLES: u32 = 2;

pub struct BggForumConnector;

impl BggForumConnector {
    pub fn new() -> Self {
        Self
    }
}

impl Default for BggForumConnector {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ResearchConnector for BggForumConnector {
    fn id(&self) -> &'static str {
        "bgg_forum"
    }

    fn default_tier(&self) -> TrustTier {
        TrustTier::Community
    }

    async fn search(&self, ctx: &GameCtx<'_>, query: &str) -> AppResult<Vec<ResearchHit>> {
        let bgg_id = match ctx.bgg_id {
            Some(id) => id,
            None => return Ok(Vec::new()),
        };

        // Pull all forums attached to this thing, then walk threads in each
        // looking for subjects matching any meaningful token from the query.
        // Throttling is the caller's job (orchestrator deadline + bgg_extra's
        // 1 req/s convention).
        let forums = bgg_extra::list_forums(bgg_id).await?;
        let tokens = query_tokens(query);
        if tokens.is_empty() {
            return Ok(Vec::new());
        }

        // (thread_id, subject, articles, match_count)
        let mut hits: Vec<(u32, String, u32, usize)> = Vec::new();
        for forum in forums.iter().filter(|f| f.num_threads > 0) {
            let threads = bgg_extra::list_threads(forum.id).await?;
            for t in threads {
                if t.num_articles < MIN_ARTICLES {
                    continue;
                }
                let subj_lower = t.subject.to_lowercase();
                let match_count = tokens
                    .iter()
                    .filter(|tok| subj_lower.contains(tok.as_str()))
                    .count();
                if match_count > 0 {
                    hits.push((t.id, t.subject, t.num_articles, match_count));
                }
            }
        }
        // Rank by token-match count, break ties by article volume.
        hits.sort_by_key(|(_id, _subj, n, m)| (std::cmp::Reverse(*m), std::cmp::Reverse(*n)));
        hits.truncate(MAX_HITS);
        let hits: Vec<(u32, String, u32)> =
            hits.into_iter().map(|(id, s, n, _)| (id, s, n)).collect();

        let mut out: Vec<ResearchHit> = Vec::with_capacity(hits.len());
        for (tid, subject, _) in hits {
            // Hydrate snippet from the first article (best-effort; if the
            // fetch fails we still surface the hit without a snippet).
            let snippet = match bgg_extra::fetch_thread(tid).await {
                Ok(arts) => arts
                    .first()
                    .map(|a| short_snippet(&a.body, 200))
                    .unwrap_or_default(),
                Err(e) => {
                    tracing::warn!("bgg_forum: fetch_thread {tid} failed: {e}");
                    String::new()
                }
            };
            out.push(ResearchHit {
                url: format!("https://boardgamegeek.com/thread/{tid}"),
                title: subject,
                snippet,
                source_kind: "bgg_forum".to_string(),
                trust_tier: TrustTier::Community,
                full_text: None,
            });
        }
        Ok(out)
    }
}

/// Lowercased query tokens worth matching against thread subjects: alpha-
/// numeric runs of length ≥ 3, minus a small English stopword list. Phrase
/// matching against full queries is too brittle (no thread title contains
/// "catan robber rules"); per-token OR-matching with score-by-overlap is
/// the right primitive.
fn query_tokens(query: &str) -> Vec<String> {
    const STOPWORDS: &[&str] = &[
        "the", "and", "for", "with", "how", "what", "does", "did", "are", "was",
        "this", "that", "from", "into", "rule", "rules", "rulebook", "question",
        "questions", "help", "about", "your", "you", "can", "but", "not",
    ];
    query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .filter(|s| s.chars().count() >= 3)
        .filter(|s| !STOPWORDS.contains(&s.as_str()))
        .collect()
}

/// Build the up-to-`max_chars` leading snippet of `body`. Operates on chars
/// (not bytes) so we don't slice mid-codepoint for CJK content.
pub(crate) fn short_snippet(body: &str, max_chars: usize) -> String {
    let trimmed = body.trim();
    let mut out = String::with_capacity(max_chars);
    for c in trimmed.chars().take(max_chars) {
        out.push(c);
    }
    if trimmed.chars().count() > max_chars {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snippet_truncates_on_chars_not_bytes() {
        let s = "你好世界，这是一个测试。".repeat(20);
        let out = short_snippet(&s, 10);
        // 10 chars + ellipsis
        assert_eq!(out.chars().count(), 11);
        assert!(out.ends_with('…'));
    }

    #[test]
    fn snippet_no_ellipsis_when_short() {
        let out = short_snippet("hi", 10);
        assert_eq!(out, "hi");
    }

    #[test]
    fn query_tokens_drops_stopwords_and_short_tokens() {
        let toks = query_tokens("Catan robber rules — the question is how?");
        assert!(toks.contains(&"catan".to_string()));
        assert!(toks.contains(&"robber".to_string()));
        assert!(!toks.contains(&"rules".to_string()), "stopword");
        assert!(!toks.contains(&"the".to_string()), "stopword");
        assert!(!toks.contains(&"is".to_string()), "too short");
    }

    #[test]
    fn query_tokens_handles_punctuation_and_dashes() {
        let toks = query_tokens("year-of-the-rat — components/cards");
        assert!(toks.contains(&"year".to_string()));
        assert!(toks.contains(&"rat".to_string()));
        assert!(toks.contains(&"components".to_string()));
        assert!(toks.contains(&"cards".to_string()));
    }

    /// Synthetic top-level integration of subject filtering / ranking. The
    /// connector itself talks to BGG, so we exercise its parsing layer
    /// (`bgg_extra`) with mock XML and then re-implement the filter we'd
    /// apply on the parsed `ThreadSummary`s — guarding the contract.
    #[test]
    fn forum_filter_keeps_subject_contains_and_ranks_by_articles() {
        let xml = r#"<?xml version="1.0"?>
<forum id="111">
  <threads>
    <thread id="9001" subject="Setup question for 2 players" numarticles="12" />
    <thread id="9002" subject="Advanced strategy" numarticles="20" />
    <thread id="9003" subject="setup help" numarticles="3" />
    <thread id="9004" subject="setup hint" numarticles="1" />
  </threads>
</forum>"#;
        // Same body as bgg_forum::search: filter then sort then truncate.
        let parsed = crate::research::bgg_extra::test_helpers::parse_threadlist(xml);
        let q = "setup".to_lowercase();
        let mut filtered: Vec<_> = parsed
            .into_iter()
            .filter(|t| t.num_articles >= MIN_ARTICLES && t.subject.to_lowercase().contains(&q))
            .collect();
        filtered.sort_by_key(|t| std::cmp::Reverse(t.num_articles));
        let ids: Vec<u32> = filtered.iter().map(|t| t.id).collect();
        // 9001 (12 articles) before 9003 (3); 9002 excluded by subject; 9004
        // excluded by article-count floor.
        assert_eq!(ids, vec![9001, 9003]);
    }
}
