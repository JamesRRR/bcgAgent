//! Wave 2 research-connector layer.
//!
//! Each connector implements [`ResearchConnector`], which takes a
//! `GameCtx` + free-text query and returns a list of [`ResearchHit`]s.
//! The orchestrator (see `super::orchestrator`) fans out across registered
//! connectors and feeds the results through a URL fetcher.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::AppResult;

pub mod bgg_forum;
pub mod url_fetch;
pub mod web_search;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TrustTier {
    Publisher,
    Designer,
    Community,
    Unverified,
}

impl TrustTier {
    /// Canonical lower-case string used everywhere in the chunks/components
    /// schema (`trust_tier` column).
    pub fn as_str(self) -> &'static str {
        match self {
            TrustTier::Publisher => "publisher",
            TrustTier::Designer => "designer",
            TrustTier::Community => "community",
            TrustTier::Unverified => "unverified",
        }
    }

    pub fn is_official(self) -> bool {
        matches!(self, TrustTier::Publisher | TrustTier::Designer)
    }
}

/// Light context the orchestrator hands every connector. Borrowed because
/// every field is read-only for the duration of one research pass.
///
/// Note: `game_id` is the canonical TEXT (uuid) used everywhere else in the
/// code base — the spec's `i64` was incorrect for this repo.
#[derive(Debug, Clone)]
pub struct GameCtx<'a> {
    pub game_id: &'a str,
    pub bgg_id: Option<u32>,
    pub name_zh: &'a str,
    pub name_en: Option<&'a str>,
    pub publisher_url: Option<&'a str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ResearchHit {
    pub url: String,
    pub title: String,
    pub snippet: String,
    /// `bgg_forum` | `web` | `publisher_faq` | ...
    pub source_kind: String,
    pub trust_tier: TrustTier,
    /// Populated by the URL fetcher; `None` until then.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub full_text: Option<String>,
}

#[async_trait]
pub trait ResearchConnector: Send + Sync {
    fn id(&self) -> &'static str;
    fn default_tier(&self) -> TrustTier;
    async fn search(&self, ctx: &GameCtx<'_>, query: &str) -> AppResult<Vec<ResearchHit>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trust_tier_serialization_is_stable() {
        assert_eq!(TrustTier::Publisher.as_str(), "publisher");
        assert_eq!(TrustTier::Community.as_str(), "community");
        assert!(TrustTier::Designer.is_official());
        assert!(!TrustTier::Community.is_official());
    }
}
