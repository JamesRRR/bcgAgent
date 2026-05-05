use thiserror::Error;

#[derive(Error, Debug)]
pub enum AppError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),

    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("http: {0}")]
    Http(#[from] reqwest::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),

    #[error("image: {0}")]
    Image(#[from] image::ImageError),

    #[error("missing api key: {0}")]
    MissingKey(&'static str),

    #[error("ocr: {0}")]
    Ocr(String),

    #[error("llm: {0}")]
    Llm(String),

    #[error("embed: {0}")]
    Embed(String),

    #[error("audio: {0}")]
    Audio(String),

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

pub type AppResult<T> = Result<T, AppError>;

impl serde::Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}
