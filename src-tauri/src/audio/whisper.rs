use std::io::ErrorKind;
use std::path::Path;
use std::process::Command;

use tokio::io::AsyncWriteExt;
use tokio_stream::StreamExt;

use crate::error::{AppError, AppResult};
use crate::paths;

const MODEL_URL: &str =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-large-v3-turbo-q5_0.bin";

const BREW_INSTALL_HINT: &str = "请先运行 `brew install whisper-cpp` 安装 whisper.cpp 命令行工具";

/// Ensure the whisper model is downloaded (idempotent). Reports progress via `tracing`.
pub async fn ensure_model() -> AppResult<()> {
    let model_path = paths::whisper_model_path();
    if model_path.exists() {
        return Ok(());
    }

    if let Some(parent) = model_path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }

    tracing::info!("downloading whisper model to {}", model_path.display());

    let resp = reqwest::get(MODEL_URL).await?.error_for_status()?;
    let total = resp.content_length();
    let tmp_path = model_path.with_extension("bin.partial");
    let mut file = tokio::fs::File::create(&tmp_path).await?;
    let mut stream = resp.bytes_stream();

    let mut downloaded: u64 = 0;
    let mut next_log: u64 = 10 * 1024 * 1024;

    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        file.write_all(&chunk).await?;
        downloaded += chunk.len() as u64;
        if downloaded >= next_log {
            match total {
                Some(t) => tracing::info!(
                    "whisper model: {} / {} MB",
                    downloaded / (1024 * 1024),
                    t / (1024 * 1024)
                ),
                None => tracing::info!("whisper model: {} MB", downloaded / (1024 * 1024)),
            }
            next_log += 10 * 1024 * 1024;
        }
    }

    file.flush().await?;
    drop(file);
    tokio::fs::rename(&tmp_path, &model_path).await?;
    tracing::info!("whisper model ready: {}", model_path.display());
    Ok(())
}

/// Transcribe a 16kHz mono WAV file to text. Returns the transcript.
/// `language_hint` is "auto" | "zh" | "en"; passed to whisper-cli's `-l`.
pub async fn transcribe(wav_path: &Path, language_hint: &str) -> AppResult<String> {
    ensure_model().await?;

    let model_path = paths::whisper_model_path();
    let wav_path = wav_path.to_path_buf();
    let language_hint = language_hint.to_string();

    tokio::task::spawn_blocking(move || run_whisper_cli(&model_path, &wav_path, &language_hint))
        .await
        .map_err(|e| AppError::Audio(format!("join error: {e}")))?
}

fn run_whisper_cli(model: &Path, wav: &Path, lang: &str) -> AppResult<String> {
    let tmp = tempfile::TempDir::new()?;
    let out_prefix = tmp.path().join("out");

    let spawn_result = Command::new("whisper-cli")
        .arg("-m")
        .arg(model)
        .arg("-f")
        .arg(wav)
        .arg("-l")
        .arg(lang)
        .arg("--no-timestamps")
        .arg("--output-txt")
        .arg("-of")
        .arg(&out_prefix)
        .output();

    let output = match spawn_result {
        Ok(o) => o,
        Err(e) if e.kind() == ErrorKind::NotFound => {
            return Err(AppError::Audio(BREW_INSTALL_HINT.to_string()));
        }
        Err(e) => return Err(AppError::Io(e)),
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Audio(format!(
            "whisper-cli exited with {}: {}",
            output.status, stderr
        )));
    }

    let txt_path = out_prefix.with_extension("txt");
    let text = std::fs::read_to_string(&txt_path)?;
    Ok(text.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore]
    async fn whisper_live_silent_wav() {
        let tmp = tempfile::TempDir::new().unwrap();
        let wav_path = tmp.path().join("silent.wav");

        let spec = hound::WavSpec {
            channels: 1,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        };
        let mut writer = hound::WavWriter::create(&wav_path, spec).unwrap();
        for _ in 0..16_000 {
            writer.write_sample(0i16).unwrap();
        }
        writer.finalize().unwrap();

        let result = transcribe(&wav_path, "auto").await;
        assert!(result.is_ok(), "transcribe failed: {:?}", result.err());
        let _text: String = result.unwrap();
    }
}
