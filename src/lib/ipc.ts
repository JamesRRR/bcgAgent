import { invoke, listen } from "@/lib/transport";
import type { UnlistenFn } from "@tauri-apps/api/event";

// ----- Types (mirror serde structs in src-tauri) -----

export type Game = {
  id: string;
  name_zh: string;
  name_en: string | null;
  publisher: string | null;
  cover_path: string | null;
  page_count: number;
  created_at: number;
};

export type Page = {
  id: string;
  game_id: string;
  page_number: number;
  image_path: string;
  thumb_path: string | null;
  ocr_status: "pending" | "done" | "failed" | string;
  ocr_markdown: string | null;
  ocr_json: string | null;
  created_at: number;
};

export type QAHistory = {
  id: string;
  game_id: string | null;
  question: string;
  answer: string | null;
  audio_path: string | null;
  retrieved_chunk_ids: string | null;
  created_at: number;
};

export type SearchHit = {
  chunk_id: number;
  game_id: string;
  game_name: string;
  page_id: string;
  page_number: number;
  heading_path: string | null;
  content: string;
  score: number;
};

export type RetrievedChunk = {
  chunk_id: number;
  game_id: string;
  game_name: string;
  page_id: string;
  page_number: number;
  heading_path: string | null;
  content: string;
  fused_score: number;
};

// ----- Games -----

export const games = {
  list: () => invoke<Game[]>("games_list"),
  create: (name_zh: string, name_en?: string, publisher?: string) =>
    invoke<string>("game_create", {
      name_zh,
      name_en: name_en ?? null,
      publisher: publisher ?? null,
    }),
  get: (id: string) => invoke<Game | null>("game_get", { id }),
  setCover: (id: string, cover_path: string) =>
    invoke<void>("game_set_cover", { id, cover_path }),
  autoSetCover: (game_id: string) =>
    invoke<void>("game_auto_set_cover", { game_id }),
  setCoverFromFile: (game_id: string, src_path: string) =>
    invoke<string>("game_set_cover_from_file", { game_id, src_path }),
  rename: (id: string, name_zh: string, name_en?: string) =>
    invoke<void>("game_rename", {
      id,
      name_zh,
      name_en: name_en ?? null,
    }),
};

// ----- Pages -----

export type PageIllustration = {
  id: string;
  page_id: string;
  game_id: string;
  position: number;
  image_path: string;
  bbox_x1: number;
  bbox_y1: number;
  bbox_x2: number;
  bbox_y2: number;
  label: string | null;
  created_at: number;
};

export const pages = {
  listByGame: (game_id: string) =>
    invoke<Page[]>("pages_list_by_game", { game_id }),
  get: (id: string) => invoke<Page | null>("page_get", { id }),
  illustrations: (page_id: string) =>
    invoke<PageIllustration[]>("page_illustrations_list", { page_id }),
};

// ----- Search -----

export const search = {
  keyword: (query: string, game_id: string | null, k: number) =>
    invoke<SearchHit[]>("search_keyword", { query, game_id, k }),
  semantic: (query: string, game_id: string | null, k: number) =>
    invoke<SearchHit[]>("search_semantic", { query, game_id, k }),
};

// ----- Ingest -----
//
// IMPORTANT: register the listeners BEFORE calling `ingest.run`, otherwise
// you'll miss the early `page_started` events. Each listener returns an
// `UnlistenFn` — call it on cleanup.

export type IngestPageStarted = { page_id: string; page_number: number };
export type IngestPageDone = {
  page_id: string;
  page_number: number;
  chunk_count: number;
};
export type IngestPageFailed = {
  page_id: string;
  page_number: number;
  error: string;
};
export type IngestDone = {
  game_id: string;
  succeeded: number;
  failed: number;
};

export const ingest = {
  run: (game_id: string, image_paths: string[]) =>
    invoke<void>("ingest_pages", { game_id, image_paths }),
  onPageStarted: (cb: (e: IngestPageStarted) => void): Promise<UnlistenFn> =>
    listen<IngestPageStarted>("ingest:page_started", (e) => cb(e.payload)),
  onPageDone: (cb: (e: IngestPageDone) => void): Promise<UnlistenFn> =>
    listen<IngestPageDone>("ingest:page_done", (e) => cb(e.payload)),
  onPageFailed: (cb: (e: IngestPageFailed) => void): Promise<UnlistenFn> =>
    listen<IngestPageFailed>("ingest:page_failed", (e) => cb(e.payload)),
  onDone: (cb: (e: IngestDone) => void): Promise<UnlistenFn> =>
    listen<IngestDone>("ingest:done", (e) => cb(e.payload)),
};

// ----- Ask (RAG streaming) -----

export type AskDone = { qa_id: string };

export const ask = {
  run: (question: string, game_id: string | null) =>
    invoke<string>("ask", { question, game_id }),
  onCitations: (cb: (chunks: RetrievedChunk[]) => void): Promise<UnlistenFn> =>
    listen<RetrievedChunk[]>("ask:citations", (e) => cb(e.payload)),
  onToken: (cb: (token: string) => void): Promise<UnlistenFn> =>
    listen<string>("ask:token", (e) => cb(e.payload)),
  onDone: (cb: (e: AskDone) => void): Promise<UnlistenFn> =>
    listen<AskDone>("ask:done", (e) => cb(e.payload)),
};

// ----- Walkthrough (beginner tutorial generation) -----

export type WalkthroughDone = { game_id: string };

export const walkthrough = {
  run: (game_id: string) => invoke<string>("walkthrough_run", { game_id }),
  onToken: (cb: (token: string) => void): Promise<UnlistenFn> =>
    listen<string>("walkthrough:token", (e) => cb(e.payload)),
  onDone: (cb: (e: WalkthroughDone) => void): Promise<UnlistenFn> =>
    listen<WalkthroughDone>("walkthrough:done", (e) => cb(e.payload)),
};

// ----- Audio -----

export type LangHint = "auto" | "zh" | "en";

export const audio = {
  transcribe: (wav: Uint8Array, lang_hint: LangHint) =>
    invoke<string>("transcribe", {
      wav_bytes: Array.from(wav),
      lang_hint,
    }),
  speak: (text: string, lang: "zh" | "en") =>
    invoke<string>("speak", { text, lang }),
  speakCancel: (handle_id: string) =>
    invoke<void>("speak_cancel", { handle_id }),
};

// ----- Settings -----

export type SecretName = "dashscope" | "minimax";

export const settings = {
  getSecret: (name: SecretName) =>
    invoke<string | null>("settings_get_secret", { name }),
  setSecret: (name: SecretName, value: string) =>
    invoke<void>("settings_set_secret", { name, value }),
  get: (key: string) => invoke<string | null>("settings_get", { key }),
  set: (key: string, value: string) =>
    invoke<void>("settings_set", { key, value }),
};

// ----- Q&A history -----

export const qa = {
  list: (game_id: string | null, limit: number) =>
    invoke<QAHistory[]>("qa_list", { game_id, limit }),
};
