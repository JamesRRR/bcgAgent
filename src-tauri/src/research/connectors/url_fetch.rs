//! URL fetcher connector.
//!
//! Given a URL, returns the page as readable markdown. Layered:
//!
//! 1. Hit `web_cache`. If we have a non-expired entry with `content_md`,
//!    return it without going to the network.
//! 2. Otherwise GET the URL with `reqwest`, run readability, convert to
//!    markdown, persist to `web_cache` with a 7-day TTL.
//!
//! Tier defaults to `Unverified`. If the URL host matches the host of
//! `ctx.publisher_url`, upgrade to `Publisher`.
//!
//! The `fetch()` helper is also called by the orchestrator to hydrate hits
//! produced by other connectors (e.g. `web_search`).

use std::time::Duration;

use async_trait::async_trait;
use time::OffsetDateTime;

use super::{GameCtx, ResearchConnector, ResearchHit, TrustTier};
use crate::error::{AppError, AppResult};
use crate::store::{research as store_research, Db};

const HTTP_TIMEOUT: Duration = Duration::from_secs(20);
const CACHE_TTL_SECS: i64 = 7 * 86_400;

/// Concrete result of a fetch — markdown + provenance metadata.
#[derive(Debug, Clone)]
pub struct CachedPage {
    pub url: String,
    pub title: Option<String>,
    pub content_md: String,
    pub from_cache: bool,
}

pub struct UrlFetchConnector {
    db: Db,
    /// Override the HTTP base for tests. When `Some`, every fetched URL is
    /// rewritten so its scheme+host+port comes from this base. Production
    /// always sets it to `None`.
    http_override: Option<String>,
}

impl UrlFetchConnector {
    pub fn new(db: Db) -> Self {
        Self {
            db,
            http_override: None,
        }
    }

    #[cfg(test)]
    pub fn with_http_base(db: Db, base: impl Into<String>) -> Self {
        Self {
            db,
            http_override: Some(base.into()),
        }
    }

    fn http_client() -> AppResult<reqwest::Client> {
        reqwest::Client::builder()
            .timeout(HTTP_TIMEOUT)
            .user_agent("bcgAgent/0.1 (https://github.com/JamesRRR/bcgAgent)")
            .build()
            .map_err(|e| AppError::Other(anyhow::anyhow!("build http client: {e}")))
    }

    /// Apply the test-only override: keep the original URL's path+query, but
    /// swap scheme/host/port for the configured base. No-op when `None`.
    fn rewrite_url(&self, url: &str) -> String {
        match self.http_override.as_deref() {
            None => url.to_string(),
            Some(base) => {
                // Strip trailing slash on base, keep path on url.
                let base = base.trim_end_matches('/');
                let path_q = match url.find("://") {
                    Some(i) => match url[i + 3..].find('/') {
                        Some(j) => &url[i + 3 + j..],
                        None => "/",
                    },
                    None => url,
                };
                format!("{base}{path_q}")
            }
        }
    }

    /// Fetch the URL, going through the on-disk web_cache when possible.
    /// Returned `CachedPage::url` is always the **logical** (input) URL — the
    /// cache key — never the rewritten test URL.
    pub async fn fetch(&self, url: &str) -> AppResult<CachedPage> {
        let now = OffsetDateTime::now_utc().unix_timestamp();
        // 1. Cache lookup.
        let cached = {
            let db = self.db.clone();
            let key = url.to_string();
            tokio::task::spawn_blocking(move || store_research::get_web_cache(&db, &key))
                .await
                .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??
        };
        if let Some(entry) = cached {
            if entry.expires_at > now {
                if let Some(md) = entry.content_md.clone() {
                    return Ok(CachedPage {
                        url: url.to_string(),
                        title: None,
                        content_md: md,
                        from_cache: true,
                    });
                }
            }
        }

        // 2. Network.
        let rewritten = self.rewrite_url(url);
        let client = Self::http_client()?;
        let resp = client
            .get(&rewritten)
            .send()
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("url_fetch: {e}")))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(AppError::Other(anyhow::anyhow!(
                "url_fetch: {url} returned {status}"
            )));
        }
        let html = resp
            .text()
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("url_fetch body: {e}")))?;

        // 3. Readability + html→markdown. Both are CPU work; parking it on a
        // blocking thread keeps the runtime responsive even on giant pages.
        let url_owned = url.to_string();
        let (title, md) = tokio::task::spawn_blocking(move || extract_to_markdown(&html, &url_owned))
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;

        // 4. Persist to cache. Chinese translation is Wave 3, so leave
        // `content_zh` empty here.
        let expires_at = now + CACHE_TTL_SECS;
        {
            let db = self.db.clone();
            let key = url.to_string();
            let md_owned = md.clone();
            let status_i = Some(status.as_u16() as i64);
            tokio::task::spawn_blocking(move || -> AppResult<()> {
                store_research::put_web_cache(
                    &db,
                    &key,
                    status_i,
                    Some(&md_owned),
                    None,
                    None,
                    expires_at,
                )
            })
            .await
            .map_err(|e| AppError::Other(anyhow::anyhow!("join: {e}")))??;
        }

        Ok(CachedPage {
            url: url.to_string(),
            title,
            content_md: md,
            from_cache: false,
        })
    }
}

#[async_trait]
impl ResearchConnector for UrlFetchConnector {
    fn id(&self) -> &'static str {
        "url_fetch"
    }

    fn default_tier(&self) -> TrustTier {
        TrustTier::Unverified
    }

    async fn search(&self, ctx: &GameCtx<'_>, query: &str) -> AppResult<Vec<ResearchHit>> {
        // `query` is interpreted as a literal URL for this connector.
        let url = query.trim();
        if url.is_empty() {
            return Ok(Vec::new());
        }
        let page = self.fetch(url).await?;

        let tier = if same_host(url, ctx.publisher_url) {
            TrustTier::Publisher
        } else {
            TrustTier::Unverified
        };

        let snippet = page
            .content_md
            .chars()
            .take(200)
            .collect::<String>()
            .trim()
            .to_string();
        Ok(vec![ResearchHit {
            url: page.url.clone(),
            title: page.title.clone().unwrap_or_else(|| url.to_string()),
            snippet,
            source_kind: "web".to_string(),
            trust_tier: tier,
            full_text: Some(page.content_md),
        }])
    }
}

/// `true` iff `url`'s host case-insensitively matches the host of
/// `publisher_url`. Both are best-effort parsed; failures → `false`.
pub(crate) fn same_host(url: &str, publisher_url: Option<&str>) -> bool {
    let pub_url = match publisher_url {
        Some(s) if !s.is_empty() => s,
        _ => return false,
    };
    let h1 = host_of(url);
    let h2 = host_of(pub_url);
    match (h1, h2) {
        (Some(a), Some(b)) => a.eq_ignore_ascii_case(&b),
        _ => false,
    }
}

fn host_of(url: &str) -> Option<String> {
    let after_scheme = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
    let host = after_scheme
        .split('/')
        .next()
        .unwrap_or("")
        .split('?')
        .next()
        .unwrap_or("")
        .split('#')
        .next()
        .unwrap_or("");
    if host.is_empty() {
        None
    } else {
        // Strip user:pass@ and port.
        let host = host.rsplit_once('@').map(|(_, h)| h).unwrap_or(host);
        let host = host.split(':').next().unwrap_or(host);
        Some(host.to_string())
    }
}

/// Run the full HTML → markdown pipeline. `readability` extracts the article
/// body as cleaned HTML; `html2md` flattens it.
fn extract_to_markdown(html: &str, url: &str) -> AppResult<(Option<String>, String)> {
    // `readability::extractor::extract` wants a `Read` + a parsed `Url`.
    let parsed_url = match url::Url::parse(url) {
        Ok(u) => u,
        Err(_) => {
            // Fall back to raw html2md when URL parsing fails — readability
            // can't run without a base URL but the markdown converter can.
            let md = html2md::parse_html(html);
            return Ok((None, md));
        }
    };
    let mut cur = std::io::Cursor::new(html.as_bytes());
    match readability::extractor::extract(&mut cur, &parsed_url) {
        Ok(p) => {
            let md = html2md::parse_html(&p.content);
            let title = if p.title.trim().is_empty() {
                None
            } else {
                Some(p.title)
            };
            Ok((title, md))
        }
        Err(_) => {
            // Readability can choke on minimal pages — fall back gracefully.
            let md = html2md::parse_html(html);
            Ok((None, md))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Db;

    #[test]
    fn host_of_extracts_correctly() {
        assert_eq!(host_of("https://www.example.com/path?q=1"), Some("www.example.com".into()));
        assert_eq!(host_of("http://example.com:8080/x"), Some("example.com".into()));
        assert_eq!(host_of("example.com/x"), Some("example.com".into()));
        assert_eq!(host_of("https://user:pw@host.tld/x"), Some("host.tld".into()));
    }

    #[test]
    fn same_host_compares_case_insensitively() {
        assert!(same_host("https://Example.com/a", Some("http://example.com")));
        assert!(!same_host("https://other.com/a", Some("http://example.com")));
        assert!(!same_host("https://example.com/a", None));
    }

    #[tokio::test]
    async fn cache_hit_returns_without_network() {
        let db = Db::open_in_memory().unwrap();
        let url = "https://example.com/already-cached";
        let now = OffsetDateTime::now_utc().unix_timestamp();
        // Pre-seed the cache with a non-expired markdown entry.
        store_research::put_web_cache(
            &db,
            url,
            Some(200),
            Some("# cached title\n\nbody"),
            None,
            None,
            now + CACHE_TTL_SECS,
        )
        .unwrap();

        // Point the fetcher at an unreachable base — if it actually goes to
        // the network we'd see an error.
        let fetcher = UrlFetchConnector::with_http_base(db.clone(), "http://127.0.0.1:1");
        let page = fetcher.fetch(url).await.unwrap();
        assert!(page.from_cache);
        assert!(page.content_md.contains("cached title"));
    }

    #[tokio::test]
    async fn cache_miss_fetches_then_persists() {
        let db = Db::open_in_memory().unwrap();
        let mut server = mockito::Server::new_async().await;
        let _m = server
            .mock("GET", "/article")
            .with_status(200)
            .with_header("content-type", "text/html; charset=utf-8")
            .with_body(
                r#"<!DOCTYPE html><html><head><title>Hello</title></head>
<body><article><h1>Hello</h1><p>This is a long enough article body
to satisfy any readability heuristic about minimum content length.
We just keep typing words so the extractor is happy.</p></article></body></html>"#,
            )
            .create_async()
            .await;

        let logical_url = "https://example.com/article";
        let fetcher = UrlFetchConnector::with_http_base(db.clone(), server.url());
        let page = fetcher.fetch(logical_url).await.unwrap();
        assert!(!page.from_cache);
        assert!(!page.content_md.is_empty());

        // Cache row exists keyed by the LOGICAL url, not the rewritten one.
        let entry = store_research::get_web_cache(&db, logical_url)
            .unwrap()
            .expect("expected cache row to be written");
        assert_eq!(entry.status, Some(200));
        assert!(entry.content_md.is_some());
    }

    #[tokio::test]
    async fn publisher_host_match_upgrades_tier() {
        let db = Db::open_in_memory().unwrap();
        let now = OffsetDateTime::now_utc().unix_timestamp();
        let url = "https://publisher.example.com/rules-faq";
        store_research::put_web_cache(
            &db,
            url,
            Some(200),
            Some("FAQ\n\nQ. Hi?\nA. Yes."),
            None,
            None,
            now + CACHE_TTL_SECS,
        )
        .unwrap();

        let fetcher = UrlFetchConnector::with_http_base(db, "http://127.0.0.1:1");
        let ctx = GameCtx {
            game_id: "g1",
            bgg_id: None,
            name_zh: "测试",
            name_en: None,
            publisher_url: Some("https://publisher.example.com"),
        };
        let hits = fetcher.search(&ctx, url).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].trust_tier, TrustTier::Publisher);
        assert!(hits[0].full_text.is_some());

        // Different host → unverified.
        let ctx2 = GameCtx {
            game_id: "g1",
            bgg_id: None,
            name_zh: "测试",
            name_en: None,
            publisher_url: Some("https://other.example.com"),
        };
        let hits2 = fetcher.search(&ctx2, url).await.unwrap();
        assert_eq!(hits2[0].trust_tier, TrustTier::Unverified);
    }
}
