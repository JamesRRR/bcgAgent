use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Game {
    pub id: String,
    pub name_zh: String,
    pub name_en: Option<String>,
    pub publisher: Option<String>,
    pub cover_path: Option<String>,
    pub page_count: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    pub id: String,
    pub game_id: String,
    pub page_number: i64,
    pub image_path: String,
    pub thumb_path: Option<String>,
    pub ocr_status: String,
    pub ocr_markdown: Option<String>,
    pub ocr_json: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: i64,
    pub page_id: String,
    pub game_id: String,
    pub heading_path: Option<String>,
    pub content: String,
    pub token_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QAHistory {
    pub id: String,
    pub game_id: Option<String>,
    pub question: String,
    pub answer: Option<String>,
    pub audio_path: Option<String>,
    pub retrieved_chunk_ids: Option<String>,
    pub created_at: i64,
}
