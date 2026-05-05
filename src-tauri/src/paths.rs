use std::path::PathBuf;

/// `~/Library/Application Support/bcgAgent/`
pub fn app_data_dir() -> PathBuf {
    dirs::data_dir()
        .expect("no data dir")
        .join("bcgAgent")
}

pub fn db_path() -> PathBuf {
    app_data_dir().join("db.sqlite")
}

pub fn games_dir() -> PathBuf {
    app_data_dir().join("games")
}

pub fn audio_dir() -> PathBuf {
    app_data_dir().join("audio").join("qa")
}

pub fn secrets_dir() -> PathBuf {
    app_data_dir().join("secrets")
}

pub fn models_dir() -> PathBuf {
    app_data_dir().join("models")
}

pub fn bge_m3_dir() -> PathBuf {
    models_dir().join("bge-m3")
}

pub fn whisper_dir() -> PathBuf {
    models_dir().join("whisper")
}

pub fn whisper_model_path() -> PathBuf {
    whisper_dir().join("ggml-large-v3-turbo-q5_0.bin")
}

pub fn ensure_layout() -> std::io::Result<()> {
    for d in [
        app_data_dir(),
        games_dir(),
        audio_dir(),
        secrets_dir(),
        models_dir(),
        bge_m3_dir(),
        whisper_dir(),
    ] {
        std::fs::create_dir_all(d)?;
    }
    Ok(())
}
