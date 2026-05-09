//! macOS `say(1)` TTS provider — the default, dependency-free fallback.

use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::error::{AppError, AppResult};

use super::{CancelInner, SpeechHandle, TtsProvider};

pub struct SayProvider;

impl TtsProvider for SayProvider {
    fn name(&self) -> &'static str {
        "system"
    }

    fn speak(
        &self,
        text: &str,
        lang: &str,
        on_exit: Box<dyn FnOnce() + Send + 'static>,
    ) -> AppResult<SpeechHandle> {
        let voice = match lang {
            "en" => "Samantha",
            "zh" => "Tingting",
            _ => "Tingting",
        };

        let mut child = Command::new("say")
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

        let pid = child.id();
        let cancelled = Arc::new(AtomicBool::new(false));

        std::thread::spawn(move || {
            let _ = child.wait();
            on_exit();
        });

        Ok(SpeechHandle::new(SayCancel { pid, cancelled }))
    }
}

struct SayCancel {
    pid: u32,
    cancelled: Arc<AtomicBool>,
}

impl CancelInner for SayCancel {
    fn cancel(&self) {
        if self.cancelled.swap(true, Ordering::SeqCst) {
            return;
        }
        let _ = Command::new("kill")
            .arg("-TERM")
            .arg(self.pid.to_string())
            .stderr(Stdio::null())
            .status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn say_provider_natural_exit_fires_on_exit() {
        let provider = SayProvider;
        let (tx, rx) = mpsc::channel();
        let _h = provider
            .speak(
                "",
                "zh",
                Box::new(move || {
                    let _ = tx.send(());
                }),
            )
            .expect("spawn");
        rx.recv_timeout(Duration::from_secs(5))
            .expect("on_exit fired");
    }

    #[test]
    fn say_provider_cancel_fires_on_exit() {
        let provider = SayProvider;
        let (tx, rx) = mpsc::channel();
        let h = provider
            .speak(
                "this sentence is quite long and should still be talking when we cancel",
                "en",
                Box::new(move || {
                    let _ = tx.send(());
                }),
            )
            .expect("spawn");
        h.cancel();
        rx.recv_timeout(Duration::from_secs(5))
            .expect("on_exit fired after cancel");
    }
}
