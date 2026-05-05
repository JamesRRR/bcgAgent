use tracing_subscriber::EnvFilter;

pub mod paths;
pub mod error;
pub mod secrets;

pub mod store;
pub mod ocr;
pub mod embed;
pub mod llm;
pub mod audio;

pub mod commands;

#[tauri::command]
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
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            ping,
            commands::games::games_list,
            commands::games::game_create,
            commands::games::game_get,
            commands::games::game_set_cover,
            commands::pages::pages_list_by_game,
            commands::pages::page_get,
            commands::pages::qa_list,
            commands::search::search_keyword,
            commands::search::search_semantic,
            commands::ingest::ingest_pages,
            commands::ask::ask,
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
