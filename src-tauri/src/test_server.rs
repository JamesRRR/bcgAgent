//! HTTP shim that exposes every Tauri command over `POST /api/<cmd>` plus
//! `GET /api/events` for SSE. Used by the browser-driven Playwright suite.
//!
//! Compiled only with `--features test-server`. Reuses the production
//! `AppState` + command helpers verbatim. The transport differs but every
//! Rust path the user cares about (DB, OCR, embeddings, RAG, whisper) runs
//! exactly as in the Tauri build.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{Multipart, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
    Json, Router,
};
use futures::stream::Stream;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::broadcast;
use tower_http::cors::{Any, CorsLayer};
use tracing_subscriber::EnvFilter;
use uuid::Uuid;

use crate::commands::{ask::run_ask, ingest::run_ingest, AppState};
use crate::events::EventSink;
use crate::store::{
    chunks as store_chunks, games as store_games, pages as store_pages, qa as store_qa,
    settings as store_settings, Db, Game, Page, QAHistory,
};
use crate::{audio as audio_mod, embed, paths, secrets};

#[derive(Debug, Clone, Serialize)]
struct SsePayload {
    kind: String,
    data: Value,
}

#[derive(Clone)]
struct AppCtx {
    state: Arc<AppState>,
    tx: broadcast::Sender<SsePayload>,
    upload_dir: PathBuf,
}

impl AppCtx {
    fn sink(&self) -> EventSink {
        let tx = self.tx.clone();
        Arc::new(move |event: &str, payload: Value| {
            let _ = tx.send(SsePayload {
                kind: event.to_string(),
                data: payload,
            });
        })
    }
}

fn err500<E: std::fmt::Display>(e: E) -> (StatusCode, String) {
    (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
}

// ---------- games ----------

async fn games_list(State(ctx): State<AppCtx>) -> Result<Json<Vec<Game>>, (StatusCode, String)> {
    let db = ctx.state.db.clone();
    let res = tokio::task::spawn_blocking(move || store_games::list_games(&db))
        .await
        .map_err(err500)?
        .map_err(err500)?;
    Ok(Json(res))
}

#[derive(Deserialize)]
struct GameCreateBody {
    name_zh: String,
    name_en: Option<String>,
    publisher: Option<String>,
}

async fn game_create(
    State(ctx): State<AppCtx>,
    Json(body): Json<GameCreateBody>,
) -> Result<Json<String>, (StatusCode, String)> {
    let db = ctx.state.db.clone();
    let res = tokio::task::spawn_blocking(move || {
        store_games::insert_game(
            &db,
            &body.name_zh,
            body.name_en.as_deref(),
            body.publisher.as_deref(),
        )
    })
    .await
    .map_err(err500)?
    .map_err(err500)?;
    Ok(Json(res))
}

#[derive(Deserialize)]
struct IdBody {
    id: String,
}

async fn game_get(
    State(ctx): State<AppCtx>,
    Json(body): Json<IdBody>,
) -> Result<Json<Option<Game>>, (StatusCode, String)> {
    let db = ctx.state.db.clone();
    let res = tokio::task::spawn_blocking(move || store_games::get_game(&db, &body.id))
        .await
        .map_err(err500)?
        .map_err(err500)?;
    Ok(Json(res))
}

#[derive(Deserialize)]
struct GameSetCoverBody {
    id: String,
    cover_path: String,
}

async fn game_set_cover(
    State(ctx): State<AppCtx>,
    Json(body): Json<GameSetCoverBody>,
) -> Result<Json<()>, (StatusCode, String)> {
    let db = ctx.state.db.clone();
    tokio::task::spawn_blocking(move || store_games::set_cover(&db, &body.id, &body.cover_path))
        .await
        .map_err(err500)?
        .map_err(err500)?;
    Ok(Json(()))
}

#[derive(Deserialize)]
struct GameRenameBody {
    id: String,
    name_zh: String,
    name_en: Option<String>,
}

async fn game_rename(
    State(ctx): State<AppCtx>,
    Json(body): Json<GameRenameBody>,
) -> Result<Json<()>, (StatusCode, String)> {
    let db = ctx.state.db.clone();
    tokio::task::spawn_blocking(move || {
        store_games::update_name(&db, &body.id, &body.name_zh, body.name_en.as_deref())
    })
    .await
    .map_err(err500)?
    .map_err(err500)?;
    Ok(Json(()))
}

// ---------- pages ----------

#[derive(Deserialize)]
struct GameIdBody {
    game_id: String,
}

async fn pages_list_by_game(
    State(ctx): State<AppCtx>,
    Json(body): Json<GameIdBody>,
) -> Result<Json<Vec<Page>>, (StatusCode, String)> {
    let db = ctx.state.db.clone();
    let res = tokio::task::spawn_blocking(move || {
        store_pages::list_pages_by_game(&db, &body.game_id)
    })
    .await
    .map_err(err500)?
    .map_err(err500)?;
    Ok(Json(res))
}

async fn page_get(
    State(ctx): State<AppCtx>,
    Json(body): Json<IdBody>,
) -> Result<Json<Option<Page>>, (StatusCode, String)> {
    let db = ctx.state.db.clone();
    let res = tokio::task::spawn_blocking(move || store_pages::get_page(&db, &body.id))
        .await
        .map_err(err500)?
        .map_err(err500)?;
    Ok(Json(res))
}

#[derive(Deserialize)]
struct QaListBody {
    game_id: Option<String>,
    limit: i64,
}

async fn qa_list(
    State(ctx): State<AppCtx>,
    Json(body): Json<QaListBody>,
) -> Result<Json<Vec<QAHistory>>, (StatusCode, String)> {
    let db = ctx.state.db.clone();
    let res = tokio::task::spawn_blocking(move || {
        store_qa::list_qa(&db, body.game_id.as_deref(), body.limit.max(0) as usize)
    })
    .await
    .map_err(err500)?
    .map_err(err500)?;
    Ok(Json(res))
}

// ---------- search ----------

#[derive(Serialize)]
struct SearchHit {
    chunk_id: i64,
    game_id: String,
    game_name: String,
    page_id: String,
    page_number: i64,
    heading_path: Option<String>,
    content: String,
    score: f32,
}

fn hydrate(db: &Db, hits: Vec<(i64, f32)>) -> anyhow::Result<Vec<SearchHit>> {
    let mut out = Vec::with_capacity(hits.len());
    for (id, score) in hits {
        let chunk = match store_chunks::get_chunk(db, id)? {
            Some(c) => c,
            None => continue,
        };
        let page = match store_pages::get_page(db, &chunk.page_id)? {
            Some(p) => p,
            None => continue,
        };
        let game = match store_games::get_game(db, &chunk.game_id)? {
            Some(g) => g,
            None => continue,
        };
        out.push(SearchHit {
            chunk_id: chunk.id,
            game_id: chunk.game_id.clone(),
            game_name: game.name_zh,
            page_id: page.id,
            page_number: page.page_number,
            heading_path: chunk.heading_path,
            content: chunk.content,
            score,
        });
    }
    Ok(out)
}

#[derive(Deserialize)]
struct SearchBody {
    query: String,
    game_id: Option<String>,
    k: usize,
}

async fn search_keyword(
    State(ctx): State<AppCtx>,
    Json(body): Json<SearchBody>,
) -> Result<Json<Vec<SearchHit>>, (StatusCode, String)> {
    let db = ctx.state.db.clone();
    let res = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<SearchHit>> {
        let raw = store_chunks::fts_search(&db, &body.query, body.game_id.as_deref(), body.k)?;
        hydrate(&db, raw)
    })
    .await
    .map_err(err500)?
    .map_err(err500)?;
    Ok(Json(res))
}

async fn search_semantic(
    State(ctx): State<AppCtx>,
    Json(body): Json<SearchBody>,
) -> Result<Json<Vec<SearchHit>>, (StatusCode, String)> {
    let db = ctx.state.db.clone();
    let res = tokio::task::spawn_blocking(move || -> anyhow::Result<Vec<SearchHit>> {
        let qv = embed::embed_query(&body.query)?;
        let raw = store_chunks::vec_search(&db, &qv, body.game_id.as_deref(), body.k)?;
        hydrate(&db, raw)
    })
    .await
    .map_err(err500)?
    .map_err(err500)?;
    Ok(Json(res))
}

// ---------- ingest + ask (events go through the SSE channel) ----------

#[derive(Deserialize)]
struct IngestBody {
    game_id: String,
    image_paths: Vec<String>,
}

async fn ingest_pages(
    State(ctx): State<AppCtx>,
    Json(body): Json<IngestBody>,
) -> Result<Json<()>, (StatusCode, String)> {
    let sink = ctx.sink();
    run_ingest(&ctx.state, sink, body.game_id, body.image_paths)
        .await
        .map_err(err500)?;
    Ok(Json(()))
}

#[derive(Deserialize)]
struct AskBody {
    question: String,
    game_id: Option<String>,
}

async fn ask(
    State(ctx): State<AppCtx>,
    Json(body): Json<AskBody>,
) -> Result<Json<String>, (StatusCode, String)> {
    let sink = ctx.sink();
    let qa_id = run_ask(&ctx.state, sink, body.question, body.game_id)
        .await
        .map_err(err500)?;
    Ok(Json(qa_id))
}

// ---------- audio ----------

#[derive(Deserialize)]
struct TranscribeBody {
    wav_bytes: Vec<u8>,
    lang_hint: String,
}

async fn transcribe(
    Json(body): Json<TranscribeBody>,
) -> Result<Json<String>, (StatusCode, String)> {
    use std::io::Write;
    if body.wav_bytes.is_empty() {
        return Err((StatusCode::BAD_REQUEST, "empty wav payload".into()));
    }
    let mut tmp = tempfile::Builder::new()
        .prefix("bcg-stt-")
        .suffix(".wav")
        .tempfile()
        .map_err(err500)?;
    tmp.write_all(&body.wav_bytes).map_err(err500)?;
    tmp.flush().map_err(err500)?;
    let (_f, path) = tmp.keep().map_err(err500)?;
    let res = audio_mod::transcribe(&path, &body.lang_hint)
        .await
        .map_err(err500)?;
    let _ = std::fs::remove_file(&path);
    Ok(Json(res))
}

#[derive(Deserialize)]
struct SpeakBody {
    text: String,
    lang: String,
}

async fn speak(
    State(ctx): State<AppCtx>,
    Json(body): Json<SpeakBody>,
) -> Result<Json<String>, (StatusCode, String)> {
    let handle = audio_mod::speak(&body.text, &body.lang).map_err(err500)?;
    let id = Uuid::new_v4().to_string();
    ctx.state.tts.lock().insert(id.clone(), handle);
    Ok(Json(id))
}

#[derive(Deserialize)]
struct SpeakCancelBody {
    handle_id: String,
}

async fn speak_cancel(
    State(ctx): State<AppCtx>,
    Json(body): Json<SpeakCancelBody>,
) -> Result<Json<()>, (StatusCode, String)> {
    let popped = ctx.state.tts.lock().remove(&body.handle_id);
    if let Some(h) = popped {
        h.cancel();
    }
    Ok(Json(()))
}

// ---------- settings ----------

#[derive(Deserialize)]
struct SecretGetBody {
    name: String,
}

async fn settings_get_secret(
    Json(body): Json<SecretGetBody>,
) -> Result<Json<Option<String>>, (StatusCode, String)> {
    let res = secrets::get_secret(&body.name).map_err(err500)?;
    Ok(Json(res))
}

#[derive(Deserialize)]
struct SecretSetBody {
    name: String,
    value: String,
}

async fn settings_set_secret(
    Json(body): Json<SecretSetBody>,
) -> Result<Json<()>, (StatusCode, String)> {
    secrets::set_secret(&body.name, &body.value).map_err(err500)?;
    Ok(Json(()))
}

#[derive(Deserialize)]
struct KvKeyBody {
    key: String,
}

async fn settings_get(
    State(ctx): State<AppCtx>,
    Json(body): Json<KvKeyBody>,
) -> Result<Json<Option<String>>, (StatusCode, String)> {
    let db = ctx.state.db.clone();
    let res = tokio::task::spawn_blocking(move || store_settings::get(&db, &body.key))
        .await
        .map_err(err500)?
        .map_err(err500)?;
    Ok(Json(res))
}

#[derive(Deserialize)]
struct KvSetBody {
    key: String,
    value: String,
}

async fn settings_set(
    State(ctx): State<AppCtx>,
    Json(body): Json<KvSetBody>,
) -> Result<Json<()>, (StatusCode, String)> {
    let db = ctx.state.db.clone();
    tokio::task::spawn_blocking(move || store_settings::set(&db, &body.key, &body.value))
        .await
        .map_err(err500)?
        .map_err(err500)?;
    Ok(Json(()))
}

// ---------- upload ----------

async fn upload_image(
    State(ctx): State<AppCtx>,
    mut mp: Multipart,
) -> Result<Json<Value>, (StatusCode, String)> {
    while let Some(field) = mp
        .next_field()
        .await
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?
    {
        let name = field.file_name().map(|s| s.to_string()).unwrap_or_default();
        let bytes = field
            .bytes()
            .await
            .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))?;
        let ext = std::path::Path::new(&name)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("png")
            .to_ascii_lowercase();
        let id = Uuid::new_v4();
        let dst = ctx.upload_dir.join(format!("{id}.{ext}"));
        std::fs::write(&dst, &bytes).map_err(err500)?;
        return Ok(Json(serde_json::json!({ "path": dst.to_string_lossy() })));
    }
    Err((StatusCode::BAD_REQUEST, "no file in multipart".into()))
}

// ---------- SSE ----------

async fn events(
    State(ctx): State<AppCtx>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    let mut rx = ctx.tx.subscribe();
    let stream = async_stream::stream! {
        loop {
            match rx.recv().await {
                Ok(payload) => {
                    let json = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".into());
                    yield Ok(Event::default().event(payload.kind.clone()).data(json));
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

// ---------- test reset ----------

/// Wipe DB + per-game asset dirs in place. Models are preserved (expensive).
async fn test_reset(State(ctx): State<AppCtx>) -> Result<Json<()>, (StatusCode, String)> {
    let games = paths::games_dir();
    let _ = std::fs::remove_dir_all(&games);
    paths::ensure_layout().map_err(err500)?;
    // Wipe rows in dependency order; we reuse the existing connection so the
    // sqlite-vec virtual tables stay registered.
    let db = ctx.state.db.clone();
    tokio::task::spawn_blocking(move || -> rusqlite::Result<()> {
        let conn = db.lock();
        conn.execute_batch(
            "DELETE FROM chunks_fts;
             DELETE FROM chunks_vec;
             DELETE FROM chunks;
             DELETE FROM pages;
             DELETE FROM qa_history;
             DELETE FROM games;
             DELETE FROM settings;",
        )?;
        Ok(())
    })
    .await
    .map_err(err500)?
    .map_err(err500)?;
    ctx.state.tts.lock().clear();
    Ok(Json(()))
}

// ---------- entry point ----------

pub fn run_test_server() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio rt");
    rt.block_on(async {
        paths::ensure_layout().expect("ensure_layout");
        let db = Db::open().expect("open db");
        let state = Arc::new(AppState {
            db,
            tts: Mutex::new(HashMap::new()),
        });

        let upload_dir = std::env::temp_dir()
            .join(format!("bcgagent-uploads-{}", std::process::id()));
        std::fs::create_dir_all(&upload_dir).expect("create upload dir");

        let (tx, _) = broadcast::channel::<SsePayload>(1024);
        let ctx = AppCtx {
            state,
            tx,
            upload_dir,
        };

        let cors = CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any);

        let app = Router::new()
            .route("/api/games_list", post(games_list))
            .route("/api/game_create", post(game_create))
            .route("/api/game_get", post(game_get))
            .route("/api/game_set_cover", post(game_set_cover))
            .route("/api/game_rename", post(game_rename))
            .route("/api/pages_list_by_game", post(pages_list_by_game))
            .route("/api/page_get", post(page_get))
            .route("/api/qa_list", post(qa_list))
            .route("/api/search_keyword", post(search_keyword))
            .route("/api/search_semantic", post(search_semantic))
            .route("/api/ingest_pages", post(ingest_pages))
            .route("/api/ask", post(ask))
            .route("/api/transcribe", post(transcribe))
            .route("/api/speak", post(speak))
            .route("/api/speak_cancel", post(speak_cancel))
            .route("/api/settings_get_secret", post(settings_get_secret))
            .route("/api/settings_set_secret", post(settings_set_secret))
            .route("/api/settings_get", post(settings_get))
            .route("/api/settings_set", post(settings_set))
            .route("/api/upload_image", post(upload_image))
            .route("/api/events", get(events))
            .route("/api/__test/reset", post(test_reset))
            .route("/api/health", get(|| async { "ok" }))
            .with_state(ctx)
            .layer(cors);

        let port: u16 = std::env::var("BCGAGENT_TEST_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1421);
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        tracing::info!("test-server listening on {addr}");
        let listener = tokio::net::TcpListener::bind(addr).await.expect("bind");
        axum::serve(listener, app.into_make_service())
            .await
            .expect("serve");
    });
}
