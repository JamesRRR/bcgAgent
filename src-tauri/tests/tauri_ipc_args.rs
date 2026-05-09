//! Tauri-runtime smoke test that catches camelCase/snake_case mismatches
//! between JS-side invoke args and Rust command parameters.
//!
//! Tauri 2 defaults to `rename_all = "camelCase"` for command params. Our
//! JS layer (and HTTP shim) sends snake_case. Without an explicit
//! `#[tauri::command(rename_all = "snake_case")]` on every command, the
//! IPC dispatcher returns "missing required key xxxId" — and that error
//! never reaches our HTTP shim or Vitest mocks.
//!
//! This test exercises the real Tauri invoke pipeline against a few
//! representative parameterized commands. If any are missing the
//! snake_case attribute, the assertion fails immediately.

use bcgagent_lib::commands::AppState;
use bcgagent_lib::commands::{games, pages, search};
use bcgagent_lib::store::Db;
use tauri::test::{get_ipc_response, mock_builder, mock_context, noop_assets, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::WebviewWindowBuilder;

fn make_request(cmd: &str, args: serde_json::Value) -> InvokeRequest {
    InvokeRequest {
        cmd: cmd.into(),
        callback: tauri::ipc::CallbackFn(0),
        error: tauri::ipc::CallbackFn(1),
        url: "http://tauri.localhost".parse().unwrap(),
        body: tauri::ipc::InvokeBody::Json(args),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    }
}

#[test]
fn snake_case_args_are_accepted() {
    let db = Db::open_in_memory().expect("in-memory db");
    let state = AppState::new(db);

    let app = mock_builder()
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            games::game_get,
            games::game_set_cover,
            pages::pages_list_by_game,
            pages::page_get,
            pages::qa_list,
            search::search_keyword,
        ])
        .build(mock_context(noop_assets()))
        .expect("build mock app");

    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("webview");

    // Each (cmd, snake_case args) pair MUST be accepted (no
    // "missing required key xxxId" error). The command may legitimately
    // succeed with an empty/null result (game doesn't exist) — that's fine.
    // We only fail if Tauri rejects the args before our handler runs.
    let cases: &[(&str, serde_json::Value)] = &[
        ("game_get", serde_json::json!({ "id": "no-such-game" })),
        (
            "game_set_cover",
            serde_json::json!({ "id": "no-such-game", "cover_path": "/tmp/x.png" }),
        ),
        (
            "pages_list_by_game",
            serde_json::json!({ "game_id": "no-such-game" }),
        ),
        ("page_get", serde_json::json!({ "id": "no-such-page" })),
        (
            "qa_list",
            serde_json::json!({ "game_id": null, "limit": 10 }),
        ),
        (
            "search_keyword",
            serde_json::json!({ "query": "x", "game_id": null, "k": 5 }),
        ),
    ];

    for (cmd, args) in cases {
        let resp = get_ipc_response(&webview, make_request(cmd, args.clone()));
        if let Err(e) = &resp {
            let msg = e.to_string();
            assert!(
                !msg.contains("missing required key"),
                "command `{cmd}` rejected snake_case args ({args}): {msg}"
            );
        }
    }
}
