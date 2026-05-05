use std::io::Cursor;
use std::path::Path;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use image::ImageFormat;
use once_cell::sync::Lazy;
use reqwest::{Client, StatusCode};
use serde_json::{Value, json};

use crate::error::{AppError, AppResult};
use crate::secrets;
use super::prompt::{GROUNDED_PROMPT, PROMPT};

#[derive(Debug, Clone)]
pub struct Illustration {
    /// Bbox in the **original** stored image's pixel space (not the
    /// possibly-downscaled copy we sent to the model). Coordinates are
    /// already clamped to the image and ordered so that x1 < x2 and y1 < y2.
    pub x1: u32,
    pub y1: u32,
    pub x2: u32,
    pub y2: u32,
    pub label: Option<String>,
}

const ENDPOINT: &str =
    "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions";
const MODEL: &str = "qwen-vl-max-latest";
const MAX_EDGE: u32 = 1568;
const JPEG_QUALITY: u8 = 85;

static HTTP: Lazy<Client> = Lazy::new(|| {
    // Grounded extraction returns markdown + JSON bboxes — responses can run
    // 30-90s on complex pages with many illustrations. 60s was too tight and
    // caused page-8-style hangs that exhausted retries silently.
    Client::builder()
        .timeout(Duration::from_secs(120))
        .build()
        .expect("reqwest client build")
});

struct PreparedImage {
    bytes: Vec<u8>,
    mime: &'static str,
    /// Dimensions of the bytes we are SENDING to the model. Bboxes returned
    /// by the model live in this coordinate space.
    sent_w: u32,
    sent_h: u32,
    /// Dimensions of the image as stored on disk (the canonical photo we
    /// will crop from later). May equal sent_w/h if no downscale was needed.
    orig_w: u32,
    orig_h: u32,
}

/// Read image, downscale if needed, return prepared bytes plus both original
/// and sent dimensions so the caller can scale bboxes back to the original.
fn load_and_prepare(path: &Path) -> AppResult<PreparedImage> {
    let raw = std::fs::read(path)?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    let img = image::load_from_memory(&raw)?;
    let (orig_w, orig_h) = (img.width(), img.height());
    let longest = orig_w.max(orig_h);

    if longest <= MAX_EDGE {
        let mime = match ext.as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "webp" => "image/webp",
            _ => "image/jpeg",
        };
        if !matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp") {
            let mut buf = Cursor::new(Vec::<u8>::new());
            img.to_rgb8().write_to(&mut buf, ImageFormat::Jpeg)?;
            return Ok(PreparedImage {
                bytes: buf.into_inner(),
                mime: "image/jpeg",
                sent_w: orig_w,
                sent_h: orig_h,
                orig_w,
                orig_h,
            });
        }
        return Ok(PreparedImage {
            bytes: raw,
            mime,
            sent_w: orig_w,
            sent_h: orig_h,
            orig_w,
            orig_h,
        });
    }

    let scale = MAX_EDGE as f32 / longest as f32;
    let nw = (orig_w as f32 * scale).round().max(1.0) as u32;
    let nh = (orig_h as f32 * scale).round().max(1.0) as u32;
    let resized = img.resize_exact(nw, nh, image::imageops::FilterType::Lanczos3);

    let mut buf = Vec::<u8>::new();
    let encoder =
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, JPEG_QUALITY);
    resized.to_rgb8().write_with_encoder(encoder)?;
    Ok(PreparedImage {
        bytes: buf,
        mime: "image/jpeg",
        sent_w: nw,
        sent_h: nh,
        orig_w,
        orig_h,
    })
}

/// Build the request body. Pure helper — no I/O — used by tests.
fn build_body(image_bytes: &[u8], mime: &str) -> Value {
    build_body_with_prompt(image_bytes, mime, PROMPT, "请转写本页。")
}

fn build_body_with_prompt(
    image_bytes: &[u8],
    mime: &str,
    system_prompt: &str,
    user_text: &str,
) -> Value {
    let b64 = STANDARD.encode(image_bytes);
    let data_url = format!("data:{};base64,{}", mime, b64);
    json!({
        "model": MODEL,
        "messages": [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": [
                {"type": "image_url", "image_url": {"url": data_url}},
                {"type": "text", "text": user_text}
            ]}
        ]
    })
}

/// Strip a leading triple-backtick fence (and trailing one) if Qwen wrapped output.
fn strip_fence(s: &str) -> String {
    let t = s.trim();
    if let Some(rest) = t.strip_prefix("```") {
        // drop optional language tag on the first line
        let after_lang = match rest.find('\n') {
            Some(i) => &rest[i + 1..],
            None => rest,
        };
        let body = after_lang.trim_end();
        let body = body.strip_suffix("```").unwrap_or(body);
        return body.trim().to_string();
    }
    t.to_string()
}

pub async fn extract_markdown(image_path: &Path) -> AppResult<String> {
    let prepared = load_and_prepare(image_path)?;
    let body = build_body(&prepared.bytes, prepared.mime);
    let key = secrets::dashscope_key()?;

    tracing::info!(url = ENDPOINT, model = MODEL, "qwen-vl ocr request");

    let mut delay_ms: u64 = 1000;
    let mut last_err: Option<AppError> = None;

    for attempt in 1..=3u32 {
        let resp = HTTP
            .post(ENDPOINT)
            .bearer_auth(&key)
            .json(&body)
            .send()
            .await;

        match resp {
            Ok(r) => {
                let status = r.status();
                if status.is_success() {
                    let v: Value = r.json().await?;
                    let content = v
                        .get("choices")
                        .and_then(|c| c.get(0))
                        .and_then(|c| c.get("message"))
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_string();
                    if content.trim().is_empty() {
                        return Err(AppError::Ocr("empty response".into()));
                    }
                    return Ok(strip_fence(&content));
                }

                let retryable = status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS;
                let body_text = r.text().await.unwrap_or_default();
                if !retryable {
                    return Err(AppError::Ocr(format!(
                        "dashscope {}: {}",
                        status, body_text
                    )));
                }
                tracing::warn!(
                    attempt,
                    %status,
                    "qwen-vl transient error, retrying"
                );
                last_err = Some(AppError::Ocr(format!(
                    "dashscope {}: {}",
                    status, body_text
                )));
            }
            Err(e) => {
                tracing::warn!(attempt, error = %e, "qwen-vl network error, retrying");
                last_err = Some(AppError::Http(e));
            }
        }

        if attempt < 3 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            delay_ms *= 2;
        }
    }

    Err(last_err.unwrap_or_else(|| AppError::Ocr("retry exhausted".into())))
}

/// Combined OCR + illustration grounding in a single Qwen-VL call.
/// The model returns JSON of the shape `{"markdown":..., "illustrations":[...]}`.
/// Bboxes are scaled from the (possibly downscaled) image we sent back into
/// the original image's pixel space, ready to crop directly from the stored
/// page photo. On any parse failure we fall back to plain `extract_markdown`
/// so a flaky JSON response never breaks ingestion.
pub async fn extract_grounded(
    image_path: &Path,
) -> AppResult<(String, Vec<Illustration>)> {
    let prepared = load_and_prepare(image_path)?;
    let body = build_body_with_prompt(
        &prepared.bytes,
        prepared.mime,
        GROUNDED_PROMPT,
        "请同时输出 Markdown 与插图边界框。",
    );
    let key = secrets::dashscope_key()?;

    tracing::info!(url = ENDPOINT, model = MODEL, "qwen-vl grounded request");

    let mut delay_ms: u64 = 1000;
    let mut last_err: Option<AppError> = None;

    for attempt in 1..=3u32 {
        let resp = HTTP
            .post(ENDPOINT)
            .bearer_auth(&key)
            .json(&body)
            .send()
            .await;

        match resp {
            Ok(r) => {
                let status = r.status();
                if status.is_success() {
                    let v: Value = r.json().await?;
                    let content = v
                        .get("choices")
                        .and_then(|c| c.get(0))
                        .and_then(|c| c.get("message"))
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_string();
                    if content.trim().is_empty() {
                        return Err(AppError::Ocr("empty response".into()));
                    }
                    return Ok(parse_grounded_response(
                        &content,
                        prepared.sent_w,
                        prepared.sent_h,
                        prepared.orig_w,
                        prepared.orig_h,
                    ));
                }

                let retryable =
                    status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS;
                let body_text = r.text().await.unwrap_or_default();
                if !retryable {
                    return Err(AppError::Ocr(format!(
                        "dashscope {}: {}",
                        status, body_text
                    )));
                }
                tracing::warn!(attempt, %status, "qwen-vl grounded transient error");
                last_err =
                    Some(AppError::Ocr(format!("dashscope {}: {}", status, body_text)));
            }
            Err(e) => {
                tracing::warn!(attempt, error = %e, "qwen-vl grounded network error");
                last_err = Some(AppError::Http(e));
            }
        }

        if attempt < 3 {
            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
            delay_ms *= 2;
        }
    }

    Err(last_err.unwrap_or_else(|| AppError::Ocr("grounded retry exhausted".into())))
}

/// Pull the JSON object from the model's reply (it may or may not be fenced),
/// extract markdown + illustrations, then scale every bbox from the model's
/// coordinate space (sent_w × sent_h) into the stored image's coordinate
/// space (orig_w × orig_h). Returns markdown + valid bboxes only — anything
/// degenerate, out-of-bounds, or trivially small (<0.5% of image) is dropped.
fn parse_grounded_response(
    content: &str,
    sent_w: u32,
    sent_h: u32,
    orig_w: u32,
    orig_h: u32,
) -> (String, Vec<Illustration>) {
    let json_text = extract_json_object(content);
    let parsed: Option<Value> = json_text.and_then(|t| serde_json::from_str(&t).ok());

    let Some(obj) = parsed else {
        // Couldn't parse — treat the whole thing as plain markdown.
        return (strip_fence(content), Vec::new());
    };

    let markdown = obj
        .get("markdown")
        .and_then(|m| m.as_str())
        .unwrap_or("")
        .to_string();
    let markdown = if markdown.trim().is_empty() {
        strip_fence(content)
    } else {
        markdown
    };

    let scale_x = orig_w as f32 / sent_w.max(1) as f32;
    let scale_y = orig_h as f32 / sent_h.max(1) as f32;
    let min_area = (orig_w as u64 * orig_h as u64) / 200; // 0.5% of image area

    let mut illustrations = Vec::new();
    if let Some(arr) = obj.get("illustrations").and_then(|i| i.as_array()) {
        for item in arr {
            let bbox = item
                .get("bbox_2d")
                .or_else(|| item.get("bbox"))
                .and_then(|b| b.as_array());
            let Some(bbox) = bbox else { continue };
            if bbox.len() < 4 {
                continue;
            }
            let to_u = |v: &Value| v.as_f64().map(|f| f.max(0.0));
            let (x1, y1, x2, y2) =
                match (to_u(&bbox[0]), to_u(&bbox[1]), to_u(&bbox[2]), to_u(&bbox[3])) {
                    (Some(a), Some(b), Some(c), Some(d)) => (a, b, c, d),
                    _ => continue,
                };
            // Scale and clamp to original image bounds.
            let x1 = ((x1 as f32) * scale_x).round().max(0.0) as u32;
            let y1 = ((y1 as f32) * scale_y).round().max(0.0) as u32;
            let x2 = ((x2 as f32) * scale_x).round().max(0.0) as u32;
            let y2 = ((y2 as f32) * scale_y).round().max(0.0) as u32;
            let (x1, x2) = (x1.min(x2), x1.max(x2));
            let (y1, y2) = (y1.min(y2), y1.max(y2));
            let x2 = x2.min(orig_w);
            let y2 = y2.min(orig_h);
            if x2 <= x1 || y2 <= y1 {
                continue;
            }
            let area = (x2 - x1) as u64 * (y2 - y1) as u64;
            if area < min_area {
                continue;
            }
            let label = item
                .get("label")
                .and_then(|l| l.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
            illustrations.push(Illustration {
                x1,
                y1,
                x2,
                y2,
                label,
            });
        }
    }

    (markdown, illustrations)
}

/// Extract the first balanced `{ ... }` JSON object from a string, even when
/// surrounded by stray prose, code fences, or trailing commentary that some
/// models emit despite our "do not explain" instruction.
fn extract_json_object(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let start = bytes.iter().position(|&b| b == b'{')?;
    let mut depth = 0i32;
    let mut in_string = false;
    let mut escape = false;
    for (i, &b) in bytes[start..].iter().enumerate() {
        if in_string {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
            }
            continue;
        }
        match b {
            b'"' => in_string = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start..start + i + 1].to_string());
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::DynamicImage;
    use std::io::Write;

    fn tiny_png_bytes() -> Vec<u8> {
        let img = DynamicImage::new_rgb8(4, 4);
        let mut buf = Cursor::new(Vec::<u8>::new());
        img.write_to(&mut buf, ImageFormat::Png).unwrap();
        buf.into_inner()
    }

    #[test]
    fn build_body_contains_model_prompt_and_data_url() {
        let bytes = tiny_png_bytes();
        let body = build_body(&bytes, "image/png");
        let s = serde_json::to_string(&body).unwrap();
        assert!(s.contains("qwen-vl-max-latest"), "missing model");
        assert!(s.contains("OCR"), "missing prompt fragment");
        assert!(
            s.contains("data:image/png;base64,"),
            "missing data url prefix"
        );
        // The user-facing instruction is included.
        assert!(s.contains("请转写本页"));
    }

    #[test]
    fn extract_json_object_handles_prose_around() {
        let s = "blah blah\n```json\n{\"a\": 1}\n```\nthen extra text";
        let j = extract_json_object(s).unwrap();
        assert_eq!(j, "{\"a\": 1}");
    }

    #[test]
    fn extract_json_object_balances_nested_braces_and_strings() {
        let s = r#"{"markdown": "text with {curly} and \"quotes\"", "illustrations": [{"bbox_2d":[1,2,3,4]}]}"#;
        let j = extract_json_object(s).unwrap();
        assert!(j.contains("bbox_2d"));
        let parsed: serde_json::Value = serde_json::from_str(&j).unwrap();
        assert_eq!(parsed["illustrations"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn parse_grounded_response_scales_bboxes_back_to_original() {
        // Model saw a 1568x1176 image; original was 4000x3000. Scale 4000/1568 ≈ 2.55.
        let body = "{\"markdown\":\"# Title\",\"illustrations\":[\
            {\"bbox_2d\":[100, 200, 500, 800], \"label\":\"meeple\"}\
        ]}";
        let (md, ill) = parse_grounded_response(body, 1568, 1176, 4000, 3000);
        assert!(md.contains("# Title"));
        assert_eq!(ill.len(), 1);
        let i = &ill[0];
        // Sanity-check scaled coords are within 1px of expected.
        let expect_x1 = (100.0_f32 * 4000.0 / 1568.0).round() as u32;
        let expect_y2 = (800.0_f32 * 3000.0 / 1176.0).round() as u32;
        assert_eq!(i.x1, expect_x1);
        assert_eq!(i.y2, expect_y2);
        assert_eq!(i.label.as_deref(), Some("meeple"));
    }

    #[test]
    fn parse_grounded_response_drops_tiny_and_invalid_boxes() {
        // 100x100 input image, min_area = 100*100/200 = 50.
        // - (0,0,1,1): area 1 — dropped (below min_area)
        // - (100,100,50,50): degenerate after we swap into normalized order;
        //   becomes (50,50,100,100) — kept, area 2500
        // - (10,10,1000,1000): clamped to (10,10,100,100) — kept, area 8100
        // - (98,98,99,99): area 1 — dropped
        let body = "{\"markdown\":\"x\",\"illustrations\":[\
            {\"bbox_2d\":[0, 0, 1, 1]},\
            {\"bbox_2d\":[100, 100, 50, 50]},\
            {\"bbox_2d\":[10, 10, 1000, 1000]},\
            {\"bbox_2d\":[98, 98, 99, 99]}\
        ]}";
        let (_, ill) = parse_grounded_response(body, 100, 100, 100, 100);
        assert_eq!(ill.len(), 2, "tiny boxes should be filtered");
        assert!(ill.iter().any(|i| i.x2 == 100 && i.y2 == 100));
    }

    #[test]
    fn parse_grounded_response_falls_back_to_plain_markdown() {
        let body = "this is not json at all";
        let (md, ill) = parse_grounded_response(body, 100, 100, 100, 100);
        assert_eq!(md, "this is not json at all");
        assert!(ill.is_empty());
    }

    #[test]
    fn strip_fence_removes_wrapping() {
        let s = "```markdown\n# Title\n\ntext\n```";
        assert_eq!(strip_fence(s), "# Title\n\ntext");
        let s2 = "no fence here";
        assert_eq!(strip_fence(s2), "no fence here");
    }

    #[test]
    fn load_and_prepare_downscales_oversized_image() {
        // Build a 2000x1000 png in a temp file; expect longest edge <= MAX_EDGE.
        let tmp = std::env::temp_dir().join("bcg_ocr_test_big.png");
        let img = DynamicImage::new_rgb8(2000, 1000);
        let mut f = std::fs::File::create(&tmp).unwrap();
        let mut bytes = Cursor::new(Vec::<u8>::new());
        img.write_to(&mut bytes, ImageFormat::Png).unwrap();
        f.write_all(&bytes.into_inner()).unwrap();

        let prep = load_and_prepare(&tmp).unwrap();
        assert_eq!(prep.mime, "image/jpeg");
        let decoded = image::load_from_memory(&prep.bytes).unwrap();
        assert!(decoded.width().max(decoded.height()) <= MAX_EDGE);
        assert_eq!(prep.orig_w, 2000);
        assert_eq!(prep.orig_h, 1000);
        assert!(prep.sent_w <= MAX_EDGE);
        let _ = std::fs::remove_file(&tmp);
    }

    /// Live end-to-end test. Requires `DASHSCOPE_KEY` env or a key file, plus
    /// a real handbook page at `tests/fixtures/page1.jpg`. Run with:
    ///   cargo test -p bcgagent --lib ocr -- --ignored --nocapture
    #[ignore]
    #[tokio::test]
    async fn live_extract_markdown() {
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/page1.jpg");
        assert!(
            fixture.exists(),
            "drop a real page at {}",
            fixture.display()
        );
        // If DASHSCOPE_KEY env var is set, write it through the secrets path
        // is not necessary — secrets::dashscope_key() reads from disk only.
        // Users who set the env var instead should expose it via the secrets
        // file. We rely on secrets::dashscope_key() here.
        let md = extract_markdown(&fixture).await.expect("ocr ok");
        eprintln!("--- markdown ---\n{}\n----------------", md);
        assert!(!md.trim().is_empty());
    }
}
