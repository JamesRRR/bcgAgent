# Knowledge-Base Research & Provenance — Design

**Status:** approved 2026-05-09  
**Owner:** jr  
**Scope:** Backend + Frontend changes to grow the bcgAgent knowledge base from multiple sources, lazily, on demand, with full provenance and Chinese-canonical storage.

## Goal

Today's KB is built from one input (user photos of a rulebook) plus a post-hoc BGG enrichment. It cannot answer rule clarifications, component-specific questions, or setup-help questions when the rulebook is silent.

The upgraded KB grows along two loops:

1. **Import-time loop** (eager) — same as today, plus a tiny seed research pass after import.
2. **Ask-time loop** (lazy, question-driven) — when retrieval confidence is low, the system fans out to research connectors (BGG forums, web), fetches & translates results to Chinese, ingests them as ordinary chunks tagged with provenance, then answers.

Every chunk carries `(source_kind, source_url, trust_tier, official, confidence)`. Citations are rendered with trust badges so users can tell publisher rules from a forum thread.

## Non-goals

- Strategy/tutorial content (deliberately deferred — the question types we're solving are *clarifications, components, setup*).
- Synthesizing answers offline; research output enters the KB as ordinary chunks and is answered by the existing RAG path.
- Local-LLM extraction; cloud (Qwen-VL + MiniMax) only for v1.

## Architecture

```
IMPORT-TIME LOOP (eager, explicit)              ASK-TIME LOOP (lazy, question-driven)
─────────────────────────────────               ──────────────────────────────────
User photos ─► Qwen-VL OCR                      Q ─► hybrid retrieve (vec + FTS)
BGG descr   ─► XMLAPI                              │
BGG forums  ─► top-N seed                          ▼
BGG gallery ─► captions                         confidence ≥ τ ?
        │                                          │ yes → answer + citations
        ▼                                          │ no
   chunks{provenance fields}                       ▼
        │                                       RESEARCH PASS
        ▼                                          │
   structured extractors                           ├─► bgg_forum (XMLAPI)
   (components, faq, setup)                        ├─► web_search (Brave, optional)
                                                   ├─► url_fetch (readability)
                                                   ▼
                                                EXTRACT: translate → Chinese
                                                   │
                                                   ▼
                                                PERSIST as chunks + structured rows
                                                   │
                                                   ▼
                                                ANSWER with badges
```

## Provenance spine

Every `chunks` row carries:

| field | values | source of truth |
|---|---|---|
| `source_kind` | `photo_ocr`, `bgg_description`, `bgg_forum`, `bgg_geeklist`, `reddit`, `publisher_faq`, `web`, `extracted_component`, `extracted_faq`, `extracted_setup` | set by the producer |
| `source_url` | original URL (null for user photos) | producer |
| `trust_tier` | `publisher` / `designer` / `community` / `unverified` | connector default, optionally upgraded by domain allowlist |
| `official` | `1` if publisher/designer tier, else `0` | derived |
| `confidence` | 0..1, decayed by user thumb-down | extractor |
| `fetched_at` | unix ts | producer |
| `endorsed` | nullable bool, set by user feedback | UI |
| `content_lang` | `zh` (canonical) | extractor |
| `content_orig` | English raw text, retained for audit | extractor |

## Storage schema

Additive ALTERs on `chunks`:

```sql
ALTER TABLE chunks ADD COLUMN source_kind   TEXT NOT NULL DEFAULT 'photo_ocr';
ALTER TABLE chunks ADD COLUMN source_url    TEXT;
ALTER TABLE chunks ADD COLUMN trust_tier    TEXT NOT NULL DEFAULT 'publisher';
ALTER TABLE chunks ADD COLUMN official      INTEGER NOT NULL DEFAULT 1;
ALTER TABLE chunks ADD COLUMN confidence    REAL NOT NULL DEFAULT 1.0;
ALTER TABLE chunks ADD COLUMN fetched_at    INTEGER;
ALTER TABLE chunks ADD COLUMN endorsed      INTEGER;
ALTER TABLE chunks ADD COLUMN content_lang  TEXT NOT NULL DEFAULT 'zh';
ALTER TABLE chunks ADD COLUMN content_orig  TEXT;
CREATE INDEX IF NOT EXISTS idx_chunks_game_tier ON chunks(game_id, trust_tier, official);
```

New tables (`components`, `faq_pairs`, `setup_steps`, `research_events`, `web_cache`, `research_budget`) — schemas as specified in design Section 3 of the brainstorm transcript (replicated below):

```sql
CREATE TABLE components (
  id INTEGER PRIMARY KEY, game_id INTEGER NOT NULL,
  name_zh TEXT NOT NULL, category TEXT,
  effect_zh TEXT, source_kind TEXT NOT NULL, source_url TEXT,
  page_id INTEGER, bbox_json TEXT, illustration_id INTEGER,
  trust_tier TEXT NOT NULL, confidence REAL NOT NULL,
  created_at INTEGER NOT NULL
);
CREATE INDEX idx_components_game ON components(game_id);

CREATE TABLE faq_pairs (
  id INTEGER PRIMARY KEY, game_id INTEGER NOT NULL,
  question_zh TEXT NOT NULL, answer_zh TEXT NOT NULL,
  source_kind TEXT NOT NULL, source_url TEXT,
  trust_tier TEXT NOT NULL, official INTEGER NOT NULL,
  confidence REAL NOT NULL, fetched_at INTEGER NOT NULL
);
CREATE INDEX idx_faq_game ON faq_pairs(game_id);

CREATE TABLE setup_steps (
  id INTEGER PRIMARY KEY, game_id INTEGER NOT NULL,
  step_no INTEGER NOT NULL, player_count TEXT,
  text_zh TEXT NOT NULL, component_ids TEXT,
  source_kind TEXT NOT NULL, source_url TEXT,
  page_id INTEGER, trust_tier TEXT NOT NULL, confidence REAL NOT NULL
);
CREATE INDEX idx_setup_game ON setup_steps(game_id, step_no);

CREATE TABLE research_events (
  id INTEGER PRIMARY KEY, game_id INTEGER NOT NULL,
  trigger TEXT NOT NULL, query TEXT NOT NULL, query_normalized TEXT NOT NULL,
  hits_json TEXT NOT NULL, chunks_added INTEGER NOT NULL,
  cost_estimate REAL, created_at INTEGER NOT NULL
);
CREATE INDEX idx_research_game ON research_events(game_id, query_normalized);

CREATE TABLE web_cache (
  url TEXT PRIMARY KEY, status INTEGER, fetched_at INTEGER NOT NULL,
  content_md TEXT, content_zh TEXT, etag TEXT, expires_at INTEGER NOT NULL
);

CREATE TABLE research_budget (
  game_id INTEGER NOT NULL, date_utc TEXT NOT NULL,
  events INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (game_id, date_utc)
);
```

Backfill: existing `chunks` rows from photo OCR get `source_kind='photo_ocr'`, `trust_tier='publisher'`. Rows in `game_external_refs` are migrated into `chunks` with appropriate `source_kind` (`bgg_description`, `bgg_forum`, etc.) and tagged `community` for forum content, `publisher` for description.

## Research connectors

```rust
#[async_trait]
pub trait ResearchConnector: Send + Sync {
    fn id(&self) -> &'static str;
    fn default_tier(&self) -> TrustTier;
    async fn search(&self, ctx: &GameCtx, query: &str) -> Result<Vec<ResearchHit>>;
}

pub struct ResearchHit {
    pub url: String,
    pub title: String,
    pub snippet: String,
    pub source_kind: String,
    pub trust_tier: TrustTier,
}
```

v1 connectors: `bgg_forum`, `url_fetch`, `web_search` (Brave, optional via API key in Settings).
v1.5 (deferred): `reddit`.

Rate limits:
- BGG: 1 req/sec (existing)
- Brave: respect their RPS; cache result lists by `(normalized_query, game_id)` for 7 days
- URL fetches cached for 7 days in `web_cache`
- Per-game cap: 20 research events / day

## Research pipeline (ask-time)

```
Q (low-confidence) ─► query rewriter (cheap MiniMax call)
                       │
                       ▼ rewritten query
                  ┌────┴────┬─────────┐
                  ▼         ▼         ▼
              bgg_forum  web_search  (reddit, v1.5)
                  └────┬────┴─────────┘
                       ▼ ranked hits (top-K, dedupe by URL)
                       │
                       ▼
                  url_fetch top-3 (parallel, with web_cache)
                       │
                       ▼
                  translate → Chinese (MiniMax)
                       │
                       ▼
                  chunker + BGE-M3 embed (existing path)
                       │
                       ▼
                  insert chunks with provenance
                       │
                       ▼
                  re-run hybrid retrieve on the original Q
                       │
                       ▼
                  LLM answer with citations + badges
```

Synchronous, with overall 8-second timeout. If timeout fires, answer with whatever was already in the KB plus a one-line note "(没有找到外部资料，仅使用本地手册作答)".

## Structured extractors

Run as background batch jobs after import (Stage 2 in the architecture). Each takes the game's existing chunks as input, calls MiniMax with a structured-output prompt in Chinese, writes to its dedicated table, then re-chunks the structured output back into `chunks` with `source_kind = extracted_*` so it participates in retrieval.

- **Components extractor** — input: rulebook markdown + `page_illustrations` rows. Output: rows in `components` table (name_zh, category, effect_zh, page_id, bbox_json).
- **FAQ extractor** — input: BGG forum threads (already fetched). Output: rows in `faq_pairs` (question_zh, answer_zh, source_url).
- **Setup extractor** — input: rulebook sections matching "Setup" / "设置" headings. Output: ordered rows in `setup_steps` (step_no, player_count, text_zh, component_ids).

Idempotent: re-running clears the game's rows in each table before re-extracting.

## Retrieval & ask-time changes

- Confidence score: `0.6 * top_cosine + 0.3 * fts_rank_normalized + 0.1 * tier_weight`. Threshold τ = 0.45.
- Tier weights: publisher 1.0, designer 0.9, community 0.7, unverified 0.5.
- User endorsement: `endorsed=1` adds +0.1 to confidence at retrieval time (not embedding); `endorsed=0` subtracts 0.2.
- New commands:
  - `cmd_explicit_research(game_id, query)` — bypass confidence check, force research.
  - `cmd_endorse_chunk(chunk_id, up: bool)` — flip the `endorsed` flag.

## UX

- **Import** — unchanged primary flow. After Stage 1 finishes, fire seed crawl (`{game} setup` + `{game} rules clarifications`) as a backgrounded research event with a quiet banner "补充资料中…" (dismissible).
- **Ask** — each citation chip gets a leading badge: 📕 publisher · 🎨 designer · 💬 community · 🌐 unverified. Click chip → opens `source_url` if present, else jumps to rulebook page.
- **New `🔍 搜索网络` button** on AskBar — forces a research pass on the current question regardless of confidence.
- **Answer caption** — when at least one cited chunk has `source_kind` other than `photo_ocr` / `bgg_description`, show "本回答引用 N 条社区资料 + M 条官方规则".
- **Settings** — new section "知识扩展":
  - Brave Search API key (optional)
  - Per-day research budget (default 20)
  - "包含非官方来源" toggle (default ON)

## Cost ceiling

Per research event, approximately:
- 1 cheap MiniMax call (query rewrite, ~50 output tokens)
- 1 Brave search request (if key set)
- 2–3 URL fetches (no LLM)
- 1 MiniMax translation call (~500 output tokens)
- 1 BGE-M3 embed batch (local, free)
- 1 normal RAG answer call (already paid)

Daily budget cap (20 events / game / day) gives a safe upper bound.

## Verification (acceptance criteria)

E2E test, run on a real game with real keys, after the implementation lands:

1. **Snapshot baseline**: pick a real game already in the user's library. Record current KB state via a small `kb_dump` harness: chunks per source_kind, total chunks, total content size, FAQ count (will be 0), components count (0), setup_steps count (0).
2. **Re-run import on same game** with the new pipeline. Wait for seed crawl + extractors to settle.
3. **Snapshot new state** with the same harness.
4. **Ask 5 representative questions** spanning the three target types: 2 rule clarifications, 2 component-lookups, 1 setup question. For each: confirm at least one citation has a non-rulebook source_kind for clarification questions; confirm structured tables are hit for component/setup questions.
5. **Diff report** — print:
   - chunks added by source_kind
   - components / faq_pairs / setup_steps populated
   - total research events fired during the test
   - any chunks that went `unverified` (and the URLs)

Pass criteria: ≥1 chunk added per non-photo source kind, ≥3 components extracted, ≥1 FAQ pair, ≥1 setup step row, all 5 questions answered with at least one citation.

## File layout (planned)

```
src-tauri/src/
  research/
    mod.rs               # public ResearchConnector trait, orchestrator, budget
    connectors/
      bgg_forum.rs
      url_fetch.rs
      web_search.rs       # Brave
    pipeline.rs           # existing — extend with new extractors
  extractors/
    mod.rs
    components.rs
    faq.rs
    setup.rs
  store/
    chunks.rs             # add provenance fields
    components.rs         # new
    faq_pairs.rs          # new
    setup_steps.rs        # new
    research.rs           # research_events, web_cache, budget
  commands/
    research.rs           # extend with explicit_research, endorse_chunk
src/components/ask/
  CitationChip.tsx        # add tier badge
  AskBar.tsx              # add 🔍 button
src/components/IngestProvider.tsx  # seed-crawl banner
src/pages/Settings.tsx    # 知识扩展 section
```

## Migration safety

- All ALTERs are additive; old data backfills with sensible defaults.
- Backup `db.sqlite` to `db.sqlite.bak.{ts}` before first run on the user's data dir.
- Migration runs once-per-version, recorded in `kv_meta.schema_version`.
- E2E test creates a temp data dir + temp game; never mutates user library.
