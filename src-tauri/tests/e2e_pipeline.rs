//! End-to-end integration test exercising the real pipeline:
//! Qwen-VL OCR → chunker → BGE/E5 embeddings → SQLite (vec + FTS5) → RAG
//! fusion → MiniMax streaming chat.
//!
//! Requires real network + valid keys at:
//!   ~/Library/Application Support/bcgAgent/secrets/{dashscope,minimax}.key
//! The first run downloads ~1.3 GB of embeddings model files into:
//!   ~/Library/Application Support/bcgAgent/models/bge-m3/
//!
//! Run with:
//!   cargo test --test e2e_pipeline -- --ignored --nocapture

use std::path::PathBuf;

use bcgagent_lib::commands::chunker::chunk_markdown;
use bcgagent_lib::embed;
use bcgagent_lib::llm::{self, Message, RetrievedChunk};
use bcgagent_lib::ocr;
use bcgagent_lib::store::{self, Db};

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/page1.jpg")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore]
async fn ingest_then_ask_real_pipeline() {
    // 1) OCR the fixture page.
    let img = fixture_path();
    assert!(img.exists(), "missing fixture {}", img.display());
    eprintln!("[1/8] OCR {} ...", img.display());
    let md = ocr::extract_markdown(&img).await.expect("OCR failed");
    eprintln!("OCR markdown ({} bytes):\n{}\n----", md.len(), md);
    assert!(md.contains('#'), "OCR result missing heading");

    // 2) Chunk the markdown.
    eprintln!("[2/8] chunk markdown");
    let chunks = chunk_markdown(&md);
    eprintln!("→ {} chunks", chunks.len());
    assert!(chunks.len() >= 2, "too few chunks");

    // 3) Embed chunks (first call downloads ~1.3GB; subsequent calls are local).
    eprintln!("[3/8] embed_batch — first run downloads model");
    let texts: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();
    let chunk_vecs = tokio::task::spawn_blocking(move || embed::embed_batch(&texts))
        .await
        .expect("blocking join")
        .expect("embed_batch failed");
    assert_eq!(chunk_vecs.len(), chunks.len());
    assert_eq!(chunk_vecs[0].len(), embed::dim(), "wrong embedding dim");
    eprintln!("→ embedded {} chunks at {}-d", chunk_vecs.len(), embed::dim());

    // 4) Open in-memory DB and insert game/page/chunks with embeddings.
    eprintln!("[4/8] store: in-memory DB + insert chunks with embeddings");
    let db = Db::open_in_memory().expect("open in-memory db");
    let game_id = store::games::insert_game(&db, "卡坦岛", Some("Catan"), None)
        .expect("insert game");
    let page_id =
        store::pages::insert_page(&db, &game_id, 1, "/tmp/page1.jpg", None)
            .expect("insert page");
    for (chunk, vec) in chunks.iter().zip(chunk_vecs.iter()) {
        store::chunks::insert_chunk_with_embedding(
            &db,
            &page_id,
            &game_id,
            chunk.heading_path.as_deref(),
            &chunk.content,
            chunk.token_count as i64,
            vec,
        )
        .expect("insert chunk");
    }

    // 5) Embed the user question.
    let question = "玩家掷出 7 点之后强盗怎么处理？";
    eprintln!("[5/8] embed question: {}", question);
    let q_vec = {
        let q = question.to_string();
        tokio::task::spawn_blocking(move || embed::embed_query(&q))
            .await
            .unwrap()
            .expect("embed_query")
    };

    // 6) Hybrid retrieval: vec_search + fts_search.
    eprintln!("[6/8] retrieve");
    let vec_hits =
        store::chunks::vec_search(&db, &q_vec, Some(&game_id), 20).expect("vec_search");
    let fts_hits = store::chunks::fts_search(&db, question, Some(&game_id), 20)
        .expect("fts_search");
    eprintln!(
        "vec_hits={}  fts_hits={}",
        vec_hits.len(),
        fts_hits.len()
    );
    let vec_ids: Vec<i64> = vec_hits.iter().map(|(id, _)| *id).collect();
    let fts_ids: Vec<i64> = fts_hits.iter().map(|(id, _)| *id).collect();

    // 7) RRF fusion → top 5; hydrate to RetrievedChunk.
    let fused = llm::rrf(&vec_ids, &fts_ids, 60, 5);
    assert!(!fused.is_empty(), "no chunks after fusion");
    eprintln!("[7/8] fused top {} chunks", fused.len());
    let retrieved: Vec<RetrievedChunk> = fused
        .iter()
        .map(|(cid, score)| {
            let chunk = store::chunks::get_chunk(&db, *cid)
                .expect("get_chunk")
                .expect("chunk exists");
            RetrievedChunk {
                chunk_id: *cid,
                game_name: "卡坦岛".into(),
                page_number: 1,
                heading_path: chunk.heading_path,
                content: chunk.content,
                fused_score: *score,
            }
        })
        .collect();
    for (i, c) in retrieved.iter().enumerate() {
        eprintln!(
            "  [{}] score={:.4} heading={:?} content={}",
            i,
            c.fused_score,
            c.heading_path.as_deref().unwrap_or(""),
            c.content.chars().take(60).collect::<String>()
        );
    }

    // 8) Build messages + stream MiniMax answer.
    eprintln!("[8/8] stream_chat MiniMax");
    let messages: Vec<Message> = llm::build_messages(question, &retrieved);
    let answer = llm::stream_chat(messages, |tok| {
        eprint!("{}", tok);
    })
    .await
    .expect("stream_chat failed");
    eprintln!("\n---\n{}\n---", answer);

    assert!(!answer.trim().is_empty(), "empty answer");
    // Soft assertion: we expect some grounding mention. Accept any of the key
    // terms from the source page so we don't over-fit the model's phrasing.
    let lower = answer.to_lowercase();
    let grounded = ["强盗", "robber", "沙漠", "资源", "7", "七"]
        .iter()
        .any(|kw| lower.contains(&kw.to_lowercase()));
    assert!(grounded, "answer doesn't reference source terms: {}", answer);
}
