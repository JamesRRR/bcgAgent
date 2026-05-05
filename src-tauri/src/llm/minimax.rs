use crate::error::{AppError, AppResult};
use crate::secrets::minimax_key;
use eventsource_stream::Eventsource;
use futures::StreamExt;
use once_cell::sync::Lazy;
use std::time::Duration;

const ENDPOINT: &str = "https://api.minimaxi.com/v1/text/chatcompletion_v2";
const MODEL: &str = "MiniMax-M2";

static HTTP: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .expect("build reqwest client")
});

#[derive(Clone, serde::Serialize, serde::Deserialize, Debug)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(serde::Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<Choice>,
    #[serde(default)]
    base_resp: Option<BaseResp>,
}

#[derive(serde::Deserialize)]
struct BaseResp {
    status_code: i32,
    #[serde(default)]
    status_msg: String,
}

#[derive(serde::Deserialize)]
struct Choice {
    #[serde(default)]
    delta: Delta,
}

#[derive(serde::Deserialize, Default)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
}

pub(crate) fn build_request_body(messages: &[Message]) -> serde_json::Value {
    serde_json::json!({
        "model": MODEL,
        "stream": true,
        "messages": messages,
    })
}

/// Stream chat completion. Calls `on_token` for each delta token chunk.
/// Returns the full concatenated assistant message on success.
pub async fn stream_chat<F>(messages: Vec<Message>, mut on_token: F) -> AppResult<String>
where
    F: FnMut(&str) + Send,
{
    let key = minimax_key()?;
    let body = build_request_body(&messages);

    let resp = HTTP
        .post(ENDPOINT)
        .bearer_auth(&key)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let text = resp.text().await.unwrap_or_default();
        return Err(AppError::Llm(format!("status {}: {}", status, text)));
    }

    let mut buf = String::new();
    let mut stream = resp.bytes_stream().eventsource();

    while let Some(event) = stream.next().await {
        match event {
            Ok(ev) => {
                let data = ev.data;
                if data == "[DONE]" {
                    return Ok(buf);
                }
                if data.is_empty() {
                    continue;
                }
                match serde_json::from_str::<StreamChunk>(&data) {
                    Ok(chunk) => {
                        if let Some(b) = &chunk.base_resp {
                            if b.status_code != 0 {
                                return Err(AppError::Llm(format!(
                                    "minimax {}: {}",
                                    b.status_code, b.status_msg
                                )));
                            }
                        }
                        if let Some(choice) = chunk.choices.into_iter().next() {
                            if let Some(content) = choice.delta.content {
                                if !content.is_empty() {
                                    on_token(&content);
                                    buf.push_str(&content);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("minimax: skip malformed chunk: {} ({})", e, data);
                    }
                }
            }
            Err(e) => {
                tracing::warn!("minimax: stream truncated: {}", e);
                return Ok(buf);
            }
        }
    }

    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_request_body_shape() {
        let msgs = vec![
            Message { role: "system".into(), content: "sys".into() },
            Message { role: "user".into(), content: "hi".into() },
        ];
        let body = build_request_body(&msgs);
        assert_eq!(body["model"], MODEL);
        assert_eq!(body["stream"], true);
        let arr = body["messages"].as_array().expect("messages array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["role"], "system");
        assert_eq!(arr[0]["content"], "sys");
        assert_eq!(arr[1]["role"], "user");
        assert_eq!(arr[1]["content"], "hi");
    }

    #[tokio::test]
    #[ignore]
    async fn live_ping() {
        let msgs = vec![Message {
            role: "user".into(),
            content: "你好".into(),
        }];
        let out = stream_chat(msgs, |t| {
            print!("{}", t);
        })
        .await
        .expect("live call");
        println!("\n--- full ---\n{}", out);
        assert!(!out.is_empty());
    }
}
