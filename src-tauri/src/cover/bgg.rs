//! BoardGameGeek XML API client.
//!
//! Free, no auth, rate-limited to 1 req/sec by us to be polite.

use std::time::Duration;

use bytes::Bytes;
use quick_xml::events::Event;
use quick_xml::Reader;

use crate::error::{AppError, AppResult};

const SEARCH_URL: &str = "https://boardgamegeek.com/xmlapi2/search";
const THING_URL: &str = "https://boardgamegeek.com/xmlapi2/thing";
const HTTP_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone)]
pub struct BggMatch {
    pub id: u32,
    pub name: String,
}

fn http_client() -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(HTTP_TIMEOUT)
        .user_agent("bcgAgent/0.1 (https://github.com/JamesRRR/bcgAgent)")
        .build()
        .map_err(|e| AppError::Other(anyhow::anyhow!("build http client: {e}")))
}

/// Search BGG by game name. Returns the first (highest-confidence) match.
pub async fn search(query: &str) -> AppResult<Option<BggMatch>> {
    if query.trim().is_empty() {
        return Ok(None);
    }
    let client = http_client()?;
    let resp = client
        .get(SEARCH_URL)
        .query(&[("query", query), ("type", "boardgame")])
        .send()
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("bgg search: {e}")))?;
    if !resp.status().is_success() {
        tracing::warn!("bgg search status {}", resp.status());
        return Ok(None);
    }
    let body = resp
        .text()
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("bgg search body: {e}")))?;
    Ok(parse_search(&body))
}

fn parse_search(xml: &str) -> Option<BggMatch> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut current_id: Option<u32> = None;
    let mut current_name: Option<String> = None;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                let tag = e.name();
                let name_bytes = tag.as_ref();
                if name_bytes == b"item" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"id" {
                            if let Ok(s) = attr.unescape_value() {
                                current_id = s.parse().ok();
                            }
                        }
                    }
                } else if name_bytes == b"name" && current_name.is_none() {
                    let mut is_primary = false;
                    let mut value: Option<String> = None;
                    for attr in e.attributes().flatten() {
                        match attr.key.as_ref() {
                            b"type" => {
                                if let Ok(s) = attr.unescape_value() {
                                    is_primary = s == "primary";
                                }
                            }
                            b"value" => {
                                if let Ok(s) = attr.unescape_value() {
                                    value = Some(s.into_owned());
                                }
                            }
                            _ => {}
                        }
                    }
                    if is_primary {
                        current_name = value;
                    }
                }
            }
            Ok(Event::End(e)) if e.name().as_ref() == b"item" => {
                if let (Some(id), Some(name)) = (current_id, current_name.clone()) {
                    return Some(BggMatch { id, name });
                }
                current_id = None;
                current_name = None;
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                tracing::warn!("bgg search parse: {e}");
                break;
            }
            _ => {}
        }
        buf.clear();
    }
    None
}

/// Fetch the cover image bytes for a BGG game id.
pub async fn fetch_cover(bgg_id: u32) -> AppResult<Option<Bytes>> {
    let client = http_client()?;
    let resp = client
        .get(THING_URL)
        .query(&[("id", bgg_id.to_string().as_str())])
        .send()
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("bgg thing: {e}")))?;
    if !resp.status().is_success() {
        tracing::warn!("bgg thing status {}", resp.status());
        return Ok(None);
    }
    let body = resp
        .text()
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("bgg thing body: {e}")))?;
    let image_url = match parse_image_url(&body) {
        Some(u) => u,
        None => return Ok(None),
    };
    let img_resp = client
        .get(&image_url)
        .send()
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("bgg image: {e}")))?;
    if !img_resp.status().is_success() {
        return Ok(None);
    }
    let bytes = img_resp
        .bytes()
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("bgg image bytes: {e}")))?;
    Ok(Some(bytes))
}

fn parse_image_url(xml: &str) -> Option<String> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut in_image = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) if e.name().as_ref() == b"image" => {
                in_image = true;
            }
            Ok(Event::End(e)) if e.name().as_ref() == b"image" => {
                in_image = false;
            }
            Ok(Event::Text(t)) if in_image => {
                if let Ok(s) = t.unescape() {
                    let url = s.trim().to_string();
                    if !url.is_empty() {
                        return Some(url);
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                tracing::warn!("bgg thing parse: {e}");
                break;
            }
            _ => {}
        }
        buf.clear();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_search_first_primary_name() {
        let xml = r#"<?xml version="1.0"?>
<items total="2">
  <item type="boardgame" id="13">
    <name type="primary" value="Catan"/>
    <yearpublished value="1995"/>
  </item>
  <item type="boardgame" id="14">
    <name type="primary" value="Catan: Cities &amp; Knights"/>
  </item>
</items>"#;
        let m = parse_search(xml).unwrap();
        assert_eq!(m.id, 13);
        assert_eq!(m.name, "Catan");
    }

    #[test]
    fn parses_thing_image_url() {
        let xml = r#"<?xml version="1.0"?>
<items>
  <item type="boardgame" id="13">
    <thumbnail>https://example.com/thumb.jpg</thumbnail>
    <image>https://example.com/big.jpg</image>
  </item>
</items>"#;
        assert_eq!(
            parse_image_url(xml),
            Some("https://example.com/big.jpg".into())
        );
    }

    #[test]
    fn empty_search_returns_none() {
        let xml = r#"<?xml version="1.0"?><items total="0"></items>"#;
        assert!(parse_search(xml).is_none());
    }
}
