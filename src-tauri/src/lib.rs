use tracing_subscriber::EnvFilter;

use error::AppResult;

pub mod paths;
pub mod error;
pub mod events;
pub mod secrets;

pub mod store;
pub mod ocr;
pub mod embed;
pub mod llm;
pub mod audio;
pub mod cover;

pub mod commands;

#[cfg(feature = "test-server")]
pub mod test_server;

#[tauri::command(rename_all = "snake_case")]
fn ping() -> &'static str {
    "pong"
}

pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    paths::ensure_layout().expect("create app data layout");
    let db = store::Db::open().expect("open sqlite db");
    let state = commands::AppState::new(db);

    tauri::Builder::default()
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(state)
        .setup(|app| {
            // Pre-download the embedding model on launch and broadcast progress
            // hints, so a slow first-run never looks like the app is frozen.
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                use serde::Serialize;
                use tauri::Emitter;
                #[derive(Serialize, Clone)]
                #[serde(rename_all = "snake_case")]
                struct Status<'a> {
                    phase: &'a str,
                    bytes: u64,
                    total: u64,
                    message: Option<String>,
                }
                let emit = |phase: &str, bytes: u64, message: Option<String>| {
                    let _ = handle.emit(
                        "app:model_status",
                        Status {
                            phase,
                            bytes,
                            total: embed::MODEL_TOTAL_BYTES,
                            message,
                        },
                    );
                };

                if embed::is_ready() {
                    emit("ready", embed::cache_size_bytes(), None);
                    return;
                }
                emit("downloading", embed::cache_size_bytes(), None);

                // Fire off the download on a second thread so we can keep
                // emitting heartbeat progress every 2s from this one.
                let (tx, rx) = std::sync::mpsc::channel::<AppResult<()>>();
                std::thread::spawn(move || {
                    let _ = tx.send(embed::warm_up());
                });
                loop {
                    match rx.recv_timeout(std::time::Duration::from_secs(2)) {
                        Ok(Ok(())) => {
                            emit("ready", embed::cache_size_bytes(), None);
                            return;
                        }
                        Ok(Err(e)) => {
                            emit("error", embed::cache_size_bytes(), Some(format!("{e}")));
                            return;
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            emit("downloading", embed::cache_size_bytes(), None);
                        }
                        Err(_) => return,
                    }
                }
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ping,
            commands::games::games_list,
            commands::games::game_create,
            commands::games::game_get,
            commands::games::game_set_cover,
            commands::games::game_auto_set_cover,
            commands::games::game_set_cover_from_file,
            commands::games::game_rename,
            commands::pages::pages_list_by_game,
            commands::pages::page_get,
            commands::pages::page_illustrations_list,
            commands::pages::qa_list,
            commands::search::search_keyword,
            commands::search::search_semantic,
            commands::ingest::ingest_pages,
            commands::ask::ask,
            commands::walkthrough::walkthrough_run,
            commands::audio::transcribe,
            commands::audio::speak,
            commands::audio::speak_cancel,
            commands::settings::settings_get_secret,
            commands::settings::settings_set_secret,
            commands::settings::settings_get,
            commands::settings::settings_set,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
