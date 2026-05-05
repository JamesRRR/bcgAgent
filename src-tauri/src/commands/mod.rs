use std::collections::HashMap;

use parking_lot::Mutex;

use crate::audio::SpeechHandle;
use crate::store::Db;

pub mod ask;
pub mod audio;
pub mod chunker;
pub mod games;
pub mod ingest;
pub mod pages;
pub mod search;
pub mod settings;

/// Global Tauri-managed state. The `db` is cloneable (Arc<Mutex<Connection>>);
/// `tts` keeps live `SpeechHandle`s keyed by uuid so the UI can cancel them.
pub struct AppState {
    pub db: Db,
    pub tts: Mutex<HashMap<String, SpeechHandle>>,
}

impl AppState {
    pub fn new(db: Db) -> Self {
        Self {
            db,
            tts: Mutex::new(HashMap::new()),
        }
    }
}
