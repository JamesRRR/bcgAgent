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

#[derive(Debug, Clone, serde::Serialize)]
pub struct BggMatch {
    pub id: u32,
    pub name: String,
    pub year: Option<u32>,
}

/// Full metadata fetched from `/xmlapi2/thing?id=N`. The `description` is the
/// long-form human-written summary BGG hosts; for many popular games this is
/// effectively a rules outline. `mechanics` and `categories` are short tags.
#[derive(Debug, Clone, serde::Serialize)]
pub struct BggThing {
    pub id: u32,
    pub primary_name: String,
    pub year: Option<u32>,
    pub description: String,
    pub min_players: Option<u32>,
    pub max_players: Option<u32>,
    pub playing_time: Option<u32>,
    pub image_url: Option<String>,
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
    let all = search_many(query).await?;
    Ok(all.into_iter().next())
}

/// Search BGG by game name. Returns up to 10 results so the user can
/// disambiguate between editions / similarly-named games.
pub async fn search_many(query: &str) -> AppResult<Vec<BggMatch>> {
    if query.trim().is_empty() {
        return Ok(vec![]);
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
        return Ok(vec![]);
    }
    let body = resp
        .text()
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("bgg search body: {e}")))?;
    let mut all = parse_search_all(&body);
    all.truncate(10);
    Ok(all)
}

fn parse_search_all(xml: &str) -> Vec<BggMatch> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut out: Vec<BggMatch> = Vec::new();
    let mut current_id: Option<u32> = None;
    let mut current_name: Option<String> = None;
    let mut current_year: Option<u32> = None;
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
                } else if name_bytes == b"yearpublished" {
                    for attr in e.attributes().flatten() {
                        if attr.key.as_ref() == b"value" {
                            if let Ok(s) = attr.unescape_value() {
                                current_year = s.parse().ok();
                            }
                        }
                    }
                }
            }
            Ok(Event::End(e)) if e.name().as_ref() == b"item" => {
                if let (Some(id), Some(name)) = (current_id, current_name.clone()) {
                    out.push(BggMatch {
                        id,
                        name,
                        year: current_year,
                    });
                }
                current_id = None;
                current_name = None;
                current_year = None;
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
    out
}

/// Fetch full metadata + description for a BGG id. Used by the "Use BGG
/// rules" import path. Returns `None` if BGG returns no item.
pub async fn fetch_thing_full(bgg_id: u32) -> AppResult<Option<BggThing>> {
    let client = http_client()?;
    let resp = client
        .get(THING_URL)
        .query(&[("id", bgg_id.to_string().as_str())])
        .send()
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("bgg thing: {e}")))?;
    if !resp.status().is_success() {
        return Ok(None);
    }
    let body = resp
        .text()
        .await
        .map_err(|e| AppError::Other(anyhow::anyhow!("bgg thing body: {e}")))?;
    Ok(parse_thing_full(&body))
}

fn parse_thing_full(xml: &str) -> Option<BggThing> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut id: Option<u32> = None;
    let mut primary_name: Option<String> = None;
    let mut year: Option<u32> = None;
    let mut description = String::new();
    let mut min_players: Option<u32> = None;
    let mut max_players: Option<u32> = None;
    let mut playing_time: Option<u32> = None;
    let mut image_url: Option<String> = None;

    let mut in_description = false;
    let mut in_image = false;
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Empty(e)) | Ok(Event::Start(e)) => {
                let tag = e.name();
                let name_bytes = tag.as_ref();
                match name_bytes {
                    b"item" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"id" {
                                if let Ok(s) = attr.unescape_value() {
                                    id = s.parse().ok();
                                }
                            }
                        }
                    }
                    b"name" => {
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
                        if is_primary && primary_name.is_none() {
                            primary_name = value;
                        }
                    }
                    b"yearpublished" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"value" {
                                if let Ok(s) = attr.unescape_value() {
                                    year = s.parse().ok();
                                }
                            }
                        }
                    }
                    b"minplayers" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"value" {
                                if let Ok(s) = attr.unescape_value() {
                                    min_players = s.parse().ok();
                                }
                            }
                        }
                    }
                    b"maxplayers" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"value" {
                                if let Ok(s) = attr.unescape_value() {
                                    max_players = s.parse().ok();
                                }
                            }
                        }
                    }
                    b"playingtime" => {
                        for attr in e.attributes().flatten() {
                            if attr.key.as_ref() == b"value" {
                                if let Ok(s) = attr.unescape_value() {
                                    playing_time = s.parse().ok();
                                }
                            }
                        }
                    }
                    b"description" => {
                        in_description = true;
                    }
                    b"image" => {
                        in_image = true;
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name_bytes = e.name();
                if name_bytes.as_ref() == b"description" {
                    in_description = false;
                } else if name_bytes.as_ref() == b"image" {
                    in_image = false;
                }
            }
            Ok(Event::Text(t)) => {
                if in_description {
                    if let Ok(s) = t.unescape() {
                        description.push_str(&s);
                    }
                } else if in_image {
                    if let Ok(s) = t.unescape() {
                        let url = s.trim().to_string();
                        if !url.is_empty() && image_url.is_none() {
                            image_url = Some(url);
                        }
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

    let id = id?;
    Some(BggThing {
        id,
        primary_name: primary_name.unwrap_or_default(),
        year,
        description: clean_bgg_description(&description),
        min_players,
        max_players,
        playing_time,
        image_url,
    })
}

/// BGG escapes some HTML entities (`&#10;` for newline, `&amp;` etc.) and
/// uses `&amp;mdash;` everywhere. This brings the description back to
/// readable plain text. We deliberately keep paragraph breaks.
pub fn clean_bgg_description(s: &str) -> String {
    let s = s
        .replace("&amp;mdash;", "—")
        .replace("&amp;ndash;", "–")
        .replace("&amp;hellip;", "…")
        .replace("&amp;ldquo;", "“")
        .replace("&amp;rdquo;", "”")
        .replace("&amp;lsquo;", "‘")
        .replace("&amp;rsquo;", "’")
        .replace("&amp;quot;", "\"")
        .replace("&amp;amp;", "&")
        .replace("&#10;", "\n")
        .replace("&#13;", "")
        .replace("&amp;#10;", "\n");
    // Collapse runs of >2 newlines into double-newline.
    let mut out = String::with_capacity(s.len());
    let mut nl_run = 0;
    for c in s.chars() {
        if c == '\n' {
            nl_run += 1;
            if nl_run <= 2 {
                out.push(c);
            }
        } else {
            nl_run = 0;
            out.push(c);
        }
    }
    out.trim().to_string()
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
        let all = parse_search_all(xml);
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].id, 13);
        assert_eq!(all[0].name, "Catan");
        assert_eq!(all[0].year, Some(1995));
    }

    #[test]
    fn parses_thing_full_metadata_and_description() {
        let xml = r#"<?xml version="1.0"?>
<items>
  <item type="boardgame" id="266192">
    <thumbnail>https://example.com/thumb.jpg</thumbnail>
    <image>https://example.com/big.jpg</image>
    <name type="primary" sortindex="1" value="Wingspan"/>
    <name type="alternate" value="翼展"/>
    <description>Wingspan is a relaxing, award-winning strategy card game.&#10;&#10;Players are bird enthusiasts.</description>
    <yearpublished value="2019"/>
    <minplayers value="1"/>
    <maxplayers value="5"/>
    <playingtime value="70"/>
  </item>
</items>"#;
        let t = parse_thing_full(xml).unwrap();
        assert_eq!(t.id, 266192);
        assert_eq!(t.primary_name, "Wingspan");
        assert_eq!(t.year, Some(2019));
        assert_eq!(t.min_players, Some(1));
        assert_eq!(t.max_players, Some(5));
        assert_eq!(t.playing_time, Some(70));
        assert!(t.description.contains("Wingspan is a relaxing"));
        assert!(t.description.contains("\n"));
        assert_eq!(t.image_url.as_deref(), Some("https://example.com/big.jpg"));
    }

    #[test]
    fn cleans_bgg_description_entities() {
        let raw = "Players are heroes&#10;&#10;Roll dice &amp;amp; battle &amp;mdash; fun!";
        let cleaned = clean_bgg_description(raw);
        assert!(cleaned.contains("\n\n"));
        assert!(cleaned.contains("&"));
        assert!(cleaned.contains("—"));
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
        assert!(parse_search_all(xml).is_empty());
    }
}
