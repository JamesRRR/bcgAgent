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
use super::prompt::PROMPT;

const ENDPOINT: &str =
    "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions";
const MODEL: &str = "qwen-vl-max-latest";
const MAX_EDGE: u32 = 1568;
const JPEG_QUALITY: u8 = 85;

static HTTP: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .expect("reqwest client build")
});

/// Read image, downscale if needed, return (bytes, mime).
fn load_and_prepare(path: &Path) -> AppResult<(Vec<u8>, &'static str)> {
    let raw = std::fs::read(path)?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();

    // Probe dimensions; if within budget and a recognized type, send original.
    let img = image::load_from_memory(&raw)?;
    let (w, h) = (img.width(), img.height());
    let longest = w.max(h);

    if longest <= MAX_EDGE {
        let mime = match ext.as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "png" => "image/png",
            "webp" => "image/webp",
            _ => "image/jpeg",
        };
        // If extension was unknown but we successfully decoded, re-encode to jpeg.
        if !matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp") {
            let mut buf = Cursor::new(Vec::<u8>::new());
            img.to_rgb8()
                .write_to(&mut buf, ImageFormat::Jpeg)?;
            return Ok((buf.into_inner(), "image/jpeg"));
        }
        return Ok((raw, mime));
    }

    // Downscale, preserving aspect ratio.
    let scale = MAX_EDGE as f32 / longest as f32;
    let nw = (w as f32 * scale).round().max(1.0) as u32;
    let nh = (h as f32 * scale).round().max(1.0) as u32;
    let resized = img.resize_exact(nw, nh, image::imageops::FilterType::Lanczos3);

    let mut buf = Vec::<u8>::new();
    let encoder =
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, JPEG_QUALITY);
    resized.to_rgb8().write_with_encoder(encoder)?;
    Ok((buf, "image/jpeg"))
}

/// Build the request body. Pure helper — no I/O — used by tests.
fn build_body(image_bytes: &[u8], mime: &str) -> Value {
    let b64 = STANDARD.encode(image_bytes);
    let data_url = format!("data:{};base64,{}", mime, b64);
    json!({
        "model": MODEL,
        "messages": [
            {"role": "system", "content": PROMPT},
            {"role": "user", "content": [
                {"type": "image_url", "image_url": {"url": data_url}},
                {"type": "text", "text": "请转写本页。"}
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
    let (bytes, mime) = load_and_prepare(image_path)?;
    let body = build_body(&bytes, mime);
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

        let (out, mime) = load_and_prepare(&tmp).unwrap();
        assert_eq!(mime, "image/jpeg");
        let decoded = image::load_from_memory(&out).unwrap();
        assert!(decoded.width().max(decoded.height()) <= MAX_EDGE);
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
