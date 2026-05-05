use std::process::{Child, Command};

use crate::error::{AppError, AppResult};

/// Speak the given text. `lang` selects the voice:
/// "zh" -> "Tingting", "en" -> "Samantha", anything else -> "Tingting".
/// Returns a `SpeechHandle` that can be `.cancel()`-ed; dropping also cancels.
pub fn speak(text: &str, lang: &str) -> AppResult<SpeechHandle> {
    let voice = match lang {
        "en" => "Samantha",
        "zh" => "Tingting",
        _ => "Tingting",
    };

    let child = Command::new("say")
        .arg("-v")
        .arg(voice)
        .arg("-r")
        .arg("200")
        .arg(text)
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                AppError::Audio("`say` 命令不存在（需 macOS 系统）".to_string())
            }
            _ => AppError::Io(e),
        })?;

    Ok(SpeechHandle {
        child: Some(child),
    })
}

pub struct SpeechHandle {
    child: Option<Child>,
}

impl SpeechHandle {
    pub fn cancel(mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    pub fn wait(mut self) -> AppResult<()> {
        let mut child = self
            .child
            .take()
            .ok_or_else(|| AppError::Audio("speech handle already consumed".to_string()))?;
        let status = child.wait()?;
        if !status.success() {
            return Err(AppError::Audio(format!("`say` exited with {status}")));
        }
        Ok(())
    }
}

impl Drop for SpeechHandle {
    fn drop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tts_smoke_empty_text() {
        let handle = speak("", "zh").expect("spawn say");
        handle.wait().expect("say exited cleanly");
    }
}
