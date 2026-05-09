//! Brave Search API connector.
//!
//! Reads the API key from `secrets/brave.key`. If no key is present,
//! `search()` is a no-op (returns `Ok(vec![])`) so the orchestrator can
//! always include this connector unconditionally.
//!
//! Returns top-5 hits as `ResearchHit { source_kind: "web", trust_tier:
//! Unverified, full_text: None }`. The orchestrator hydrates `full_text`
//! later via `UrlFetchConnector::fetch`.

use std::time::Duration;

use async_trait::async_trait;

use super::{GameCtx, ResearchConnector, ResearchHit, TrustTier};
use crate::error::{AppError, AppResult};
use crate::secrets;

const BRAVE_URL: &str = "https://api.search.brave.com/res/v1/web/search";
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);
const COUNT: usize = 5;

pub struct WebSearchConnector {
    /// Optional override for tests; production code never sets this.
    base_url: String,
    /// If `None`, the connector goes silent.
    api_key: Option<String>,
}

impl WebSearchConnector {
    /// Construct from disk secrets. Missing/empty key disables the connector.
    pub fn from_secrets() -> AppResult<Self> {
        let api_key = secrets::get_secret("brave")?;
        Ok(Self {
            base_url: BRAVE_URL.to_string(),
            api_key,
        })
    }

    #[cfg(test)]
    pub fn with_overrides(base_url: impl Into<String>, api_key: Option<String>) -> Self {
        Self {
            base_url: base_url.into(),
            api_key,
        }
    }

    fn http_client() -> AppResult<reqwest::Client> {
        reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .user_agent("bcgAgent/0.1 (https://github.com/JamesRRR/bcgAgent)")
            .build()
            .map_err(|e| AppError::Other(anyhow::anyhow!("build http client: {e}")))
    }
}

#[async_trait]
impl ResearchConnector for WebSearchConnector {
    fn id(&self) -> &'static str {
        "web_search"
    }

    fn default_tier(&self) -> TrustTier {
        TrustTier::Unverified
    }

    async fn search(&self, _ctx: &GameCtx<'_>, query: &str) -> AppResult<Vec<ResearchHit>> {
        let key = match self.api_key.as_deref() {
            Some(k) if !k.is_empty() => k,
            _ => return Ok(Vec::new()), // silent disable
        };
        let q = query.trim();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let client = Self::http_client()?;
        let resp = client
            .get(&self.base_url)
            .header("X-Subscription-Token", key)
            .header("Accept", "application/json")
            .query(&[("q", q), ("count", &COUNT.to_string())])
            .send()
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("brave: {e}")))?;
        if !resp.status().is_success() {
            tracing::warn!("brave search status {}", resp.status());
            return Ok(Vec::new());
        }
        let body: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("brave json: {e}")))?;
        Ok(parse_brave_response(&body, COUNT))
    }
}

fn parse_brave_response(body: &serde_json::Value, count: usize) -> Vec<ResearchHit> {
    let arr = body
        .get("web")
        .and_then(|w| w.get("results"))
        .and_then(|r| r.as_array());
    let arr = match arr {
        Some(a) => a,
        None => return Vec::new(),
    };
    let mut out = Vec::with_capacity(arr.len().min(count));
    for item in arr.iter().take(count) {
        let url = item
            .get("url")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let title = item
            .get("title")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let snippet = item
            .get("description")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        if url.is_empty() {
            continue;
        }
        out.push(ResearchHit {
            url,
            title,
            snippet,
            source_kind: "web".to_string(),
            trust_tier: TrustTier::Unverified,
            full_text: None,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> GameCtx<'static> {
        GameCtx {
            game_id: "g",
            bgg_id: None,
            name_zh: "test",
            name_en: None,
            publisher_url: None,
        }
    }

    #[tokio::test]
    async fn no_key_returns_empty() {
        let conn = WebSearchConnector::with_overrides("http://127.0.0.1:1", None);
        let hits = conn.search(&ctx(), "anything").await.unwrap();
        assert!(hits.is_empty());
    }

    #[tokio::test]
    async fn empty_query_returns_empty_even_with_key() {
        let conn = WebSearchConnector::with_overrides("http://127.0.0.1:1", Some("k".into()));
        let hits = conn.search(&ctx(), "   ").await.unwrap();
        assert!(hits.is_empty());
    }

    #[test]
    fn parses_brave_response() {
        let body = serde_json::json!({
            "web": {
                "results": [
                    {
                        "url": "https://example.com/a",
                        "title": "A",
                        "description": "snippet a"
                    },
                    {
                        "url": "https://example.com/b",
                        "title": "B",
                        "description": "snippet b"
                    },
                    {
                        // missing url → skipped
                        "title": "C",
                        "description": ""
                    }
                ]
            }
        });
        let hits = parse_brave_response(&body, 5);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].url, "https://example.com/a");
        assert_eq!(hits[0].source_kind, "web");
        assert_eq!(hits[0].trust_tier, TrustTier::Unverified);
        assert_eq!(hits[1].snippet, "snippet b");
    }

    #[tokio::test]
    async fn live_path_uses_mock_server() {
        let mut server = mockito::Server::new_async().await;
        let body = r#"{"web":{"results":[{"url":"https://x.com/1","title":"T1","description":"D1"}]}}"#;
        let _m = server
            .mock("GET", "/")
            .match_query(mockito::Matcher::AllOf(vec![
                mockito::Matcher::UrlEncoded("q".into(), "wingspan setup".into()),
                mockito::Matcher::UrlEncoded("count".into(), "5".into()),
            ]))
            .match_header("X-Subscription-Token", "abc")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create_async()
            .await;

        let conn = WebSearchConnector::with_overrides(server.url(), Some("abc".into()));
        let hits = conn.search(&ctx(), "wingspan setup").await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].url, "https://x.com/1");
        assert_eq!(hits[0].title, "T1");
    }
}
