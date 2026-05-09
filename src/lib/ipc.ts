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

export type ResearchSummary = {
  bgg_id: number | null;
  description_added: boolean;
  forum_threads_added: number;
  gallery_captions_added: number;
  illustrations_captioned: number;
  chunks_added: number;
};

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
  delete: (id: string) => invoke<void>("game_delete", { id }),
  researchRun: (game_id: string) =>
    invoke<ResearchSummary>("research_run", { game_id }),
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
  token: string | null;
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

// ----- External (BGG) import -----

export type BggMatch = { id: number; name: string; year: number | null };
export type BggImportResult = {
  game_id: string;
  page_count: number;
  chunk_count: number;
};

export const bgg = {
  search: (query: string) => invoke<BggMatch[]>("bgg_search", { query }),
  importFromBgg: (
    bgg_id: number,
    name_zh_override: string | null,
    existing_game_id: string | null,
  ) =>
    invoke<BggImportResult>("import_from_bgg", {
      bgg_id,
      name_zh_override,
      existing_game_id,
    }),
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
  getCached: (game_id: string) =>
    invoke<string | null>("walkthrough_get_cached", { game_id }),
  onToken: (cb: (token: string) => void): Promise<UnlistenFn> =>
    listen<string>("walkthrough:token", (e) => cb(e.payload)),
  onDone: (cb: (e: WalkthroughDone) => void): Promise<UnlistenFn> =>
    listen<WalkthroughDone>("walkthrough:done", (e) => cb(e.payload)),
};

// ----- Walkthrough session (conversational beginner mode) -----

export type WalkthroughTurn = {
  turn_no: number;
  role: "agent" | "user" | string;
  kind: string;
  content: string;
  created_at: number;
};

export type WalkthroughSession = {
  session_id: string;
  game_id: string;
  phase: string;
  created_at: number;
  updated_at: number;
};

export type WalkthroughSessionView = {
  session: WalkthroughSession;
  turns: WalkthroughTurn[];
};

export type WalkthroughSessionToken = {
  session_id: string;
  token: string;
};

export type WalkthroughSessionDone = {
  session_id: string;
  turn_no: number;
  phase: string;
  full_content: string;
};

export const walkthroughSession = {
  start: (game_id: string) =>
    invoke<WalkthroughSessionView>("walkthrough_session_start", { game_id }),
  continue_: (session_id: string, user_kind: string, user_text: string) =>
    invoke<void>("walkthrough_session_continue", {
      session_id,
      user_kind,
      user_text,
    }),
  get: (game_id: string) =>
    invoke<WalkthroughSessionView | null>("walkthrough_session_get", { game_id }),
  reset: (game_id: string) =>
    invoke<void>("walkthrough_session_reset", { game_id }),
  onToken: (cb: (e: WalkthroughSessionToken) => void): Promise<UnlistenFn> =>
    listen<WalkthroughSessionToken>("walkthrough_session:token", (e) =>
      cb(e.payload),
    ),
  onDone: (cb: (e: WalkthroughSessionDone) => void): Promise<UnlistenFn> =>
    listen<WalkthroughSessionDone>("walkthrough_session:done", (e) =>
      cb(e.payload),
    ),
};

// ----- Audio -----

export type LangHint = "auto" | "zh" | "en";

export type TtsDone = { handle_id: string };

export type TranscribePartial = {
  session_id: string;
  text: string;
  duration_ms: number;
};

export const audio = {
  transcribe: (wav: Uint8Array, lang_hint: LangHint) =>
    invoke<string>("transcribe", {
      wav_bytes: Array.from(wav),
      lang_hint,
    }),
  transcribeStreamStart: (session_id: string, lang_hint: LangHint) =>
    invoke<void>("transcribe_stream_start", { session_id, lang_hint }),
  transcribeChunk: (session_id: string, wav: Uint8Array) =>
    invoke<void>("transcribe_chunk", {
      session_id,
      wav_bytes: Array.from(wav),
    }),
  transcribeFinalize: (session_id: string) =>
    invoke<string>("transcribe_finalize", { session_id }),
  transcribeStreamCancel: (session_id: string) =>
    invoke<void>("transcribe_stream_cancel", { session_id }),
  /** Native push-to-talk (cpal) — bypasses WKWebView's broken getUserMedia. */
  micCaptureStart: (session_id: string, lang_hint: LangHint) =>
    invoke<void>("mic_capture_start", { session_id, lang_hint }),
  micCaptureStop: (session_id: string) =>
    invoke<string>("mic_capture_stop", { session_id }),
  micCaptureCancel: (session_id: string) =>
    invoke<void>("mic_capture_cancel", { session_id }),
  onTranscribePartial: (
    cb: (e: TranscribePartial) => void,
  ): Promise<UnlistenFn> =>
    listen<TranscribePartial>("transcribe:partial", (e) => cb(e.payload)),
  speak: (text: string, lang: "zh" | "en") =>
    invoke<string>("speak", { text, lang }),
  speakCancel: (handle_id: string) =>
    invoke<void>("speak_cancel", { handle_id }),
  onTtsDone: (cb: (e: TtsDone) => void): Promise<UnlistenFn> =>
    listen<TtsDone>("tts:done", (e) => cb(e.payload)),
};

// ----- Settings -----

export type SecretName = "dashscope" | "minimax" | "elevenlabs";

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
