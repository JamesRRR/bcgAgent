CREATE TABLE IF NOT EXISTS games (
    id          TEXT PRIMARY KEY,
    name_zh     TEXT NOT NULL,
    name_en     TEXT,
    publisher   TEXT,
    cover_path  TEXT,
    page_count  INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS pages (
    id            TEXT PRIMARY KEY,
    game_id       TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    page_number   INTEGER NOT NULL,
    image_path    TEXT NOT NULL,
    thumb_path    TEXT,
    ocr_status    TEXT NOT NULL DEFAULT 'pending',
    ocr_markdown  TEXT,
    ocr_json      TEXT,
    created_at    INTEGER NOT NULL,
    UNIQUE (game_id, page_number)
);

CREATE TABLE IF NOT EXISTS chunks (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    page_id       TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    game_id       TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    heading_path  TEXT,
    content       TEXT NOT NULL,
    token_count   INTEGER NOT NULL
);

CREATE VIRTUAL TABLE IF NOT EXISTS chunks_vec USING vec0(
    embedding float[1024]
);

CREATE VIRTUAL TABLE IF NOT EXISTS chunks_fts USING fts5(
    tokens,
    heading_path,
    tokenize = "unicode61 remove_diacritics 2"
);

CREATE TABLE IF NOT EXISTS qa_history (
    id                   TEXT PRIMARY KEY,
    game_id              TEXT,
    question             TEXT NOT NULL,
    answer               TEXT,
    audio_path           TEXT,
    retrieved_chunk_ids  TEXT,
    created_at           INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
    key    TEXT PRIMARY KEY,
    value  TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_pages_game ON pages(game_id);
CREATE INDEX IF NOT EXISTS idx_chunks_game ON chunks(game_id);
CREATE INDEX IF NOT EXISTS idx_qa_game ON qa_history(game_id);

CREATE TABLE IF NOT EXISTS page_illustrations (
    id          TEXT PRIMARY KEY,
    page_id     TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    game_id     TEXT NOT NULL REFERENCES games(id) ON DELETE CASCADE,
    position    INTEGER NOT NULL,
    image_path  TEXT NOT NULL,
    bbox_x1     INTEGER NOT NULL,
    bbox_y1     INTEGER NOT NULL,
    bbox_x2     INTEGER NOT NULL,
    bbox_y2     INTEGER NOT NULL,
    label       TEXT,
    created_at  INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_illustrations_page ON page_illustrations(page_id);
CREATE INDEX IF NOT EXISTS idx_illustrations_game ON page_illustrations(game_id);
