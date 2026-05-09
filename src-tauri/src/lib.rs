use tracing_subscriber::EnvFilter;

use error::AppResult;

pub mod error;
pub mod events;
pub mod paths;
pub mod secrets;

pub mod audio;
pub mod cover;
pub mod embed;
pub mod extractors;
pub mod llm;
pub mod ocr;
pub mod research;
pub mod store;

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

    // Wave 4: snapshot the user's `db.sqlite` BEFORE running the Wave 1
    // backfill (which mutates rows). The check happens after Db::open() so
    // schema migrations have already run, but the additive ALTERs don't
    // change row data — only the backfill does. We probe the pending flag
    // before kicking off any data-touching pass.
    //
    // Best-effort: a backup failure is logged and ignored so a stuck
    // filesystem doesn't brick first launch.
    let db = store::Db::open().expect("open sqlite db");
    match db.wave1_backfill_pending() {
        Ok(true) => match db.backup_to_file() {
            Ok(Some(path)) => tracing::info!("Wave 1 backup saved: {}", path.display()),
            Ok(None) => tracing::info!("Wave 1 backup skipped: db file does not exist yet"),
            Err(e) => tracing::warn!("Wave 1 backup failed (continuing): {e}"),
        },
        Ok(false) => {}
        Err(e) => tracing::warn!("Wave 1 backfill probe failed (continuing): {e}"),
    }

    audio::tts::bootstrap_from_dev_secrets(&db);
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

                // Also kick off a background whisper-model download so the
                // first push-to-talk press doesn't block on a 1-2 min fetch.
                // No status emits — silent best-effort. ensure_model is
                // idempotent.
                std::thread::spawn(|| {
                    let rt = match tokio::runtime::Runtime::new() {
                        Ok(rt) => rt,
                        Err(e) => {
                            tracing::warn!("whisper warmup runtime: {e}");
                            return;
                        }
                    };
                    if let Err(e) = rt.block_on(audio::whisper::ensure_model()) {
                        tracing::warn!("whisper model warmup failed: {e}");
                    }
                });

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
            commands::games::game_delete,
            commands::pages::pages_list_by_game,
            commands::pages::page_get,
            commands::pages::page_illustrations_list,
            commands::pages::qa_list,
            commands::search::search_keyword,
            commands::search::search_semantic,
            commands::ingest::ingest_pages,
            commands::import_external::bgg_search,
            commands::import_external::import_from_bgg,
            commands::research::research_run,
            commands::research::cmd_explicit_research,
            commands::research::cmd_run_extractors,
            commands::research::cmd_endorse_chunk,
            commands::research::cmd_kb_diff,
            commands::ask::ask,
            commands::walkthrough::walkthrough_run,
            commands::walkthrough::walkthrough_get_cached,
            commands::walkthrough_session::walkthrough_session_start,
            commands::walkthrough_session::walkthrough_session_continue,
            commands::walkthrough_session::walkthrough_session_get,
            commands::walkthrough_session::walkthrough_session_reset,
            commands::audio::transcribe,
            commands::audio::transcribe_stream_start,
            commands::audio::transcribe_chunk,
            commands::audio::transcribe_finalize,
            commands::audio::transcribe_stream_cancel,
            commands::audio::mic_capture_start,
            commands::audio::mic_capture_stop,
            commands::audio::mic_capture_cancel,
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
