//! BGG XMLAPI2 endpoints not covered by `cover::bgg`: forum list, forum
//! threads, and the per-game image gallery.
//!
//! All requests use the same UA + 15s timeout convention as `cover::bgg`.
//! Caller is responsible for spacing requests (we use a 1 req/sec throttle
//! at the pipeline layer).

use std::time::Duration;

use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::{AppError, AppResult};

const FORUMLIST_URL: &str = "https://boardgamegeek.com/xmlapi2/forumlist";
const FORUM_URL: &str = "https://boardgamegeek.com/xmlapi2/forum";
const THREAD_URL: &str = "https://boardgamegeek.com/xmlapi2/thread";
const IMAGES_URL: &str = "https://boardgamegeek.com/xmlapi2/images";
const HTTP_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone)]
pub struct ForumSummary {
    pub id: u32,
    pub title: String,
    pub num_threads: u32,
}

#[derive(Debug, Clone)]
pub struct ThreadSummary {
    pub id: u32,
    pub subject: String,
    pub num_articles: u32,
}

#[derive(Debug, Clone)]
pub struct ThreadArticle {
    pub subject: String,
    pub username: String,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct GalleryImage {
    pub id: u32,
    pub caption: String,
    pub image_url: String,
}

fn http_client() -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent("bcgAgent/0.1 (https://github.com/JamesRRR/bcgAgent)")
        .build()
        .map_err(|e| AppError::Other(anyhow::anyhow!("build http client: {e}")))
}

/// List forums attached to a BGG thing (boardgame). Filters to forums whose
/// title hints at rules / how-to discussion.
pub async fn list_forums(thing_id: u32) -> AppResult<Vec<ForumSummary>> {
    let client = http_client()?;
    let resp = client
        .get(FORUMLIST_URL)
        .query(&[("id", thing_id.to_string().as_str()), ("type", "thing")])
        .send()
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("forumlist: {e}")))?;
    if !resp.status().is_success() {
        return Ok(vec![]);
    }
    let body = resp
        .text()
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("forumlist body: {e}")))?;
    Ok(parse_forumlist(&body))
}

fn parse_forumlist(xml: &str) -> Vec<ForumSummary> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut out = Vec::new();
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) if e.name().as_ref() == b"forum" => {
                let mut id: Option<u32> = None;
                let mut title: Option<String> = None;
                let mut num_threads: u32 = 0;
                for attr in e.attributes().flatten() {
                    match attr.key.as_ref() {
                        b"id" => {
                            if let Ok(s) = attr.unescape_value() {
                                id = s.parse().ok();
                            }
                        }
                        b"title" => {
                            if let Ok(s) = attr.unescape_value() {
                                title = Some(s.into_owned());
                            }
                        }
                        b"numthreads" => {
                            if let Ok(s) = attr.unescape_value() {
                                num_threads = s.parse().unwrap_or(0);
                            }
                        }
                        _ => {}
                    }
                }
                if let (Some(id), Some(title)) = (id, title) {
                    out.push(ForumSummary {
                        id,
                        title,
                        num_threads,
                    });
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                tracing::warn!("forumlist parse: {e}");
                break;
            }
            _ => {}
        }
        buf.clear();
    }
    out
}

/// List threads in a forum. Caller can then pick the most-replied "rules"
/// threads to hydrate.
pub async fn list_threads(forum_id: u32) -> AppResult<Vec<ThreadSummary>> {
    let client = http_client()?;
    let resp = client
        .get(FORUM_URL)
        .query(&[("id", forum_id.to_string().as_str())])
        .send()
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("forum: {e}")))?;
    if !resp.status().is_success() {
        return Ok(vec![]);
    }
    let body = resp
        .text()
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("forum body: {e}")))?;
    Ok(parse_threadlist(&body))
}

fn parse_threadlist(xml: &str) -> Vec<ThreadSummary> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut out = Vec::new();
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) if e.name().as_ref() == b"thread" => {
                let mut id: Option<u32> = None;
                let mut subject: Option<String> = None;
                let mut num_articles: u32 = 0;
                for attr in e.attributes().flatten() {
                    match attr.key.as_ref() {
                        b"id" => {
                            if let Ok(s) = attr.unescape_value() {
                                id = s.parse().ok();
                            }
                        }
                        b"subject" => {
                            if let Ok(s) = attr.unescape_value() {
                                subject = Some(s.into_owned());
                            }
                        }
                        b"numarticles" => {
                            if let Ok(s) = attr.unescape_value() {
                                num_articles = s.parse().unwrap_or(0);
                            }
                        }
                        _ => {}
                    }
                }
                if let (Some(id), Some(subject)) = (id, subject) {
                    out.push(ThreadSummary {
                        id,
                        subject,
                        num_articles,
                    });
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                tracing::warn!("threadlist parse: {e}");
                break;
            }
            _ => {}
        }
        buf.clear();
    }
    out
}

/// Fetch every article in a thread. We only keep the body text, no images.
pub async fn fetch_thread(thread_id: u32) -> AppResult<Vec<ThreadArticle>> {
    let client = http_client()?;
    let resp = client
        .get(THREAD_URL)
        .query(&[("id", thread_id.to_string().as_str())])
        .send()
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("thread: {e}")))?;
    if !resp.status().is_success() {
        return Ok(vec![]);
    }
    let body = resp
        .text()
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("thread body: {e}")))?;
    Ok(parse_thread(&body))
}

fn parse_thread(xml: &str) -> Vec<ThreadArticle> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut out = Vec::new();
    let mut buf = Vec::new();

    let mut cur_subject = String::new();
    let mut cur_username = String::new();
    let mut cur_body = String::new();

    let mut in_article = false;
    let mut in_subject = false;
    let mut in_body = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) | Ok(Event::Empty(e)) => {
                let n = e.name();
                let nb = n.as_ref();
                if nb == b"article" {
                    in_article = true;
                    cur_subject.clear();
                    cur_username.clear();
                    cur_body.clear();
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"username" {
                            if let Ok(s) = attr.unescape_value() {
                                cur_username = s.into_owned();
                            }
                        }
                    }
                } else if in_article && nb == b"subject" {
                    in_subject = true;
                } else if in_article && nb == b"body" {
                    in_body = true;
                }
            }
            Ok(Event::End(e)) => {
                let nb = e.name();
                if nb.as_ref() == b"subject" {
                    in_subject = false;
                } else if nb.as_ref() == b"body" {
                    in_body = false;
                } else if nb.as_ref() == b"article" {
                    in_article = false;
                    let body_clean = strip_bgg_html(&cur_body);
                    if !body_clean.trim().is_empty() {
                        out.push(ThreadArticle {
                            subject: cur_subject.clone(),
                            username: cur_username.clone(),
                            body: body_clean,
                        });
                    }
                }
            }
            Ok(Event::Text(t)) => {
                if let Ok(s) = t.unescape() {
                    if in_subject {
                        cur_subject.push_str(&s);
                    } else if in_body {
                        cur_body.push_str(&s);
                    }
                }
            }
            Ok(Event::CData(t)) => {
                if let Ok(s) = std::str::from_utf8(t.as_ref()) {
                    if in_body {
                        cur_body.push_str(s);
                    } else if in_subject {
                        cur_subject.push_str(s);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                tracing::warn!("thread parse: {e}");
                break;
            }
            _ => {}
        }
        buf.clear();
    }
    out
}

/// BGG forum bodies are HTML-ish: BBCode + raw tags. Strip enough so the
/// embedded text is readable plain text. Cheap, lossy, good enough for RAG.
fn strip_bgg_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    let out = out
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ");
    let mut nl_run = 0;
    let mut compact = String::with_capacity(out.len());
    for c in out.chars() {
        if c == '\n' {
            nl_run += 1;
            if nl_run <= 2 {
                compact.push(c);
            }
        } else {
            nl_run = 0;
            compact.push(c);
        }
    }
    compact.trim().to_string()
}

/// Fetch one page of the BGG image gallery for a game (50 per page).
pub async fn fetch_gallery_page(thing_id: u32, page: u32) -> AppResult<Vec<GalleryImage>> {
    let client = http_client()?;
    let resp = client
        .get(IMAGES_URL)
        .query(&[
            ("ajax", "1"),
            ("gallery", "all"),
            ("nosession", "1"),
            ("objectid", thing_id.to_string().as_str()),
            ("objecttype", "thing"),
            ("pageid", page.to_string().as_str()),
            ("showcount", "50"),
            ("size", "crop100"),
            ("sort", "hot"),
        ])
        .send()
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("gallery: {e}")))?;
    if !resp.status().is_success() {
        return Ok(vec![]);
    }
    let body = resp
        .text()
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("gallery body: {e}")))?;
    // The legacy ajax endpoint returns JSON, not XML.
    Ok(parse_gallery_json(&body))
}

fn parse_gallery_json(body: &str) -> Vec<GalleryImage> {
    let v: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("gallery json parse: {e}");
            return vec![];
        }
    };
    let arr = match v.get("images").and_then(|x| x.as_array()) {
        Some(a) => a,
        None => return vec![],
    };
    let mut out = Vec::with_capacity(arr.len());
    for item in arr {
        let id = item
            .get("imageid")
            .and_then(|x| {
                x.as_str()
                    .and_then(|s| s.parse().ok())
                    .or_else(|| x.as_u64().map(|n| n as u32))
            })
            .unwrap_or(0);
        let caption = item
            .get("caption")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let image_url = item
            .get("imageurl_lg")
            .or_else(|| item.get("imageurl"))
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        if id != 0 && (!caption.is_empty() || !image_url.is_empty()) {
            out.push(GalleryImage {
                id,
                caption,
                image_url,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_forumlist() {
        let xml = r#"<?xml version="1.0"?>
<forums termsofuse="https://...">
  <forum id="111" title="Rules" numthreads="42" />
  <forum id="222" title="General" numthreads="100" />
</forums>"#;
        let f = parse_forumlist(xml);
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].title, "Rules");
        assert_eq!(f[0].num_threads, 42);
    }

    #[test]
    fn parses_threadlist() {
        let xml = r#"<?xml version="1.0"?>
<forum id="111">
  <threads>
    <thread id="1001" subject="How does X work?" numarticles="12" />
    <thread id="1002" subject="Setup question" numarticles="3" />
  </threads>
</forum>"#;
        let t = parse_threadlist(xml);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].id, 1001);
        assert_eq!(t[1].num_articles, 3);
    }

    #[test]
    fn parses_thread_articles() {
        let xml = r#"<?xml version="1.0"?>
<thread id="1001">
  <articles>
    <article id="9001" username="alice">
      <subject>How does X work?</subject>
      <body>You play X by &lt;b&gt;rolling dice&lt;/b&gt;.&#10;Then move.</body>
    </article>
    <article id="9002" username="bob">
      <subject>Re: How does X work?</subject>
      <body>Yep, that's right.</body>
    </article>
  </articles>
</thread>"#;
        let arts = parse_thread(xml);
        assert_eq!(arts.len(), 2);
        assert_eq!(arts[0].username, "alice");
        assert!(arts[0].body.contains("rolling dice"));
        assert!(!arts[0].body.contains("<b>"));
    }

    #[test]
    fn strips_bgg_html_basics() {
        let s = strip_bgg_html("<p>Hello &amp; <b>world</b></p>\n\n\n\nbye");
        assert!(s.contains("Hello & world"));
        assert!(!s.contains("<"));
    }

    #[test]
    fn parses_gallery_json() {
        let body = r#"{"images":[{"imageid":"123","caption":"Card front","imageurl_lg":"https://example.com/a.jpg"},{"imageid":456,"caption":"","imageurl":"https://example.com/b.jpg"}]}"#;
        let g = parse_gallery_json(body);
        assert_eq!(g.len(), 2);
        assert_eq!(g[0].id, 123);
        assert_eq!(g[0].caption, "Card front");
        assert_eq!(g[1].image_url, "https://example.com/b.jpg");
    }
}
