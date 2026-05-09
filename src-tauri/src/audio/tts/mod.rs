//! TTS provider abstraction.
//!
//! `TtsProvider::speak` accepts an `on_exit` callback fired exactly once when
//! playback finishes (naturally or via cancel) and returns a `SpeechHandle`
//! whose `cancel()` is idempotent. Concrete providers live in sibling
//! modules (`say.rs`, `elevenlabs.rs`) and are selected by `pick_provider`
//! based on app settings + secrets.

mod elevenlabs;
mod pcm_sink;
mod say;

use crate::error::AppResult;
use crate::secrets;
use crate::store::{settings as store_settings, Db};

pub use elevenlabs::ElevenLabsProvider;
pub use say::SayProvider;

/// Concrete provider trait. Returns a `SpeechHandle` whose `cancel()` is
/// idempotent and whose Drop also cancels.
pub trait TtsProvider: Send + Sync {
    fn name(&self) -> &'static str;

    fn speak(
        &self,
        text: &str,
        lang: &str,
        on_exit: Box<dyn FnOnce() + Send + 'static>,
    ) -> AppResult<SpeechHandle>;
}

/// Cancel half of a speech handle. Concrete providers implement this with
/// kill-by-pid (`SayProvider`) or kill-afplay-and-abort-http (`ElevenLabsProvider`).
pub(crate) trait CancelInner: Send + Sync {
    fn cancel(&self);
}

/// Type-erased speech handle. Owned by `AppState.tts` map; cloning is not
/// supported — there is at most one handle per `speak()` call.
pub struct SpeechHandle {
    inner: Option<Box<dyn CancelInner>>,
}

impl SpeechHandle {
    pub(crate) fn new<C: CancelInner + 'static>(c: C) -> Self {
        Self {
            inner: Some(Box::new(c)),
        }
    }

    /// Idempotent. Subsequent calls are no-ops.
    pub fn cancel(mut self) {
        if let Some(inner) = self.inner.take() {
            inner.cancel();
        }
    }
}

impl Drop for SpeechHandle {
    fn drop(&mut self) {
        if let Some(inner) = self.inner.take() {
            inner.cancel();
        }
    }
}

/// Settings keys for picking a provider.
///
/// `tts_provider` has three meaningful states:
///   - `""` / unset → **Auto**: prefer ElevenLabs when an API key is present, otherwise fall back to `SayProvider`.
///   - `"elevenlabs"` → **Force ElevenLabs**: still falls back to `SayProvider` if the key is missing.
///   - `"system"` → **Force system**: never call ElevenLabs even if a key is configured.
pub const SETTING_PROVIDER: &str = "tts_provider";
pub const SETTING_EL_VOICE_ID: &str = "tts_elevenlabs_voice_id";
/// Default voice when the user hasn't picked one. The maintainer's personal
/// cloned voice (multilingual v2). Override per-user via the Settings UI.
pub const DEFAULT_EL_VOICE_ID: &str = "LOL6aFvN7gBkc7zf1Co9";

/// Resolve the active provider from settings + secrets.
///
/// Backward compatibility rule: any failure path (missing setting, missing
/// key, empty value) falls back to `SayProvider`. The user's `say` voice
/// must keep working with no configuration.
pub fn pick_provider(db: &Db) -> Box<dyn TtsProvider> {
    let provider_name = store_settings::get(db, SETTING_PROVIDER)
        .ok()
        .flatten()
        .unwrap_or_default();

    if provider_name == "system" {
        return Box::new(SayProvider);
    }

    let want_elevenlabs = provider_name == "elevenlabs" || provider_name.is_empty();
    if want_elevenlabs {
        if let Ok(Some(api_key)) = secrets::get_secret("elevenlabs") {
            let voice_id = store_settings::get(db, SETTING_EL_VOICE_ID)
                .ok()
                .flatten()
                .unwrap_or_else(|| DEFAULT_EL_VOICE_ID.to_string());
            return Box::new(ElevenLabsProvider::new(api_key, voice_id));
        }
        if provider_name == "elevenlabs" {
            tracing::warn!("tts_provider=elevenlabs but no api key found; falling back to system");
        }
    }
    Box::new(SayProvider)
}

/// One-time bootstrap: if the user has no in-app ElevenLabs key but has one
/// in `~/.config/dev-secrets/elevenlabs.env`, copy `ELEVENLABS_API_KEY` into
/// the app's secret store and seed `tts_elevenlabs_voice_id` from
/// `ELEVENLABS_VOICE_ID` (only when the setting is empty). Idempotent and
/// best-effort — every error path is logged as `warn!` and swallowed so a
/// missing/invalid file never blocks launch.
pub fn bootstrap_from_dev_secrets(db: &Db) {
    if let Ok(Some(_)) = secrets::get_secret("elevenlabs") {
        return; // append-only: never overwrite an existing key
    }
    let Some(home) = dirs::home_dir() else {
        return;
    };
    let path = home.join(".config/dev-secrets/elevenlabs.env");
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
        Err(e) => {
            tracing::warn!("read {}: {e}", path.display());
            return;
        }
    };

    let mut api_key: Option<String> = None;
    let mut voice_id: Option<String> = None;
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let v = v.trim().trim_matches(|c| c == '"' || c == '\'');
        match k.trim() {
            "ELEVENLABS_API_KEY" if !v.is_empty() => api_key = Some(v.to_string()),
            "ELEVENLABS_VOICE_ID" if !v.is_empty() => voice_id = Some(v.to_string()),
            _ => {}
        }
    }

    if let Some(key) = api_key {
        if let Err(e) = secrets::set_secret("elevenlabs", &key) {
            tracing::warn!("seed elevenlabs key: {e}");
        } else {
            tracing::info!("seeded elevenlabs api key from dev-secrets");
        }
    }
    if let Some(vid) = voice_id {
        let existing = store_settings::get(db, SETTING_EL_VOICE_ID)
            .ok()
            .flatten()
            .unwrap_or_default();
        if existing.is_empty() {
            if let Err(e) = store_settings::set(db, SETTING_EL_VOICE_ID, &vid) {
                tracing::warn!("seed elevenlabs voice_id: {e}");
            }
        }
    }
}
