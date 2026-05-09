# bcgAgent — Agent Surface

Macos Tauri 2 app for boardgame rule books: photo → OCR → RAG → voice/text Q&A. Chinese-first.

## Quick facts

- **Bundle ID**: `com.bcgagent.app`
- **Display name**: 攀达桌游
- **Repo**: https://github.com/JamesRRR/bcgAgent (public)
- **Stack**: Tauri 2 / Rust + React 18 / TypeScript / SQLite (sqlite-vec + FTS5) / fastembed / Qwen-VL / MiniMax-M2 / whisper.cpp
- **Data dir**: `~/Library/Application Support/bcgAgent/`
- **Launch**: `open -a 攀达桌游`

## Local commands

```bash
pnpm install
pnpm tauri:dev          # dev
pnpm tauri:build        # release build (.app + .dmg, Info.plist mic permission injected)
pnpm test               # vitest
pnpm exec playwright test
```

## Release workflow (GitHub Releases + auto-updater)

End-to-end:

1. Bump version in three places: `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`.
2. Commit, then `git tag vX.Y.Z && git push origin vX.Y.Z`.
3. GitHub Action `.github/workflows/release.yml` runs on `macos-14` (Apple Silicon only — Intel was dropped because runners queue indefinitely).
4. `tauri-action@v0` builds, signs (Apple), notarizes, signs the updater artifact (Tauri minisign), uploads to GitHub Releases.
5. Existing clients hit `https://github.com/JamesRRR/bcgAgent/releases/latest/download/latest.json` on launch (3s delay) and prompt user.

### One-time setup

Secrets live in `~/.config/dev-secrets/` (machine-local, `chmod 700`). Sync to GitHub:

```bash
~/.config/dev-secrets/sync-to-github.sh JamesRRR/bcgAgent bcgAgent
```

This pushes 8 secrets: 6 Apple (`APPLE_*`, `APPLE_CERTIFICATE` is base64 .p12), 2 Tauri updater (`TAURI_SIGNING_PRIVATE_KEY*`).

### Files involved

- `.github/workflows/release.yml` — CI workflow
- `src-tauri/tauri.conf.json` — updater pubkey + endpoint, bundle targets include `updater`
- `src-tauri/Cargo.toml` — `tauri-plugin-updater`, `tauri-plugin-process` deps
- `src-tauri/capabilities/default.json` — `updater:default`, `process:default`, `process:allow-restart`
- `src-tauri/src/lib.rs` — plugins registered on the Tauri builder
- `src/lib/updater.ts` — wraps `check()` + `downloadAndInstall()` + `relaunch()`
- `src/components/UpdaterBanner.tsx` — non-blocking bottom-right prompt
- `src/App.tsx` — mounts `<UpdaterBanner />`

## User-facing pages (sidebar nav order)

- `library` (桌游架) — game shelf, BGG/auto cover thumbnails
- `import` (导入) — drag-drop OCR pipeline; shows model warmup banner on cold start
- `walkthrough` (新手向导) — conversational beginner mode. The LLM coach issues one instruction at a time, waits for the player's "好了" or "我有问题" reply, then advances. State persists per game in `walkthrough_sessions` + `walkthrough_turns`. The old one-shot 6-section guide is still reachable via the 📖 button as a fallback / quick reference. **Hold Space** anywhere on the page (no input focused) to push-to-talk: live partial transcripts render into the question composer; release to auto-submit. Powered by streaming whisper-cli sessions.
- `ask` (问规则) — voice or text Q&A
- `settings` (设置) — API keys, language, retrieval K

Cross-page surfaces: `<UpdaterBanner>` (top-right toast on new release) and `<ModelStatusBanner>` (top banner during embedding-model download).

## Tauri events the UI listens for

- `app:model_status` — `{phase: "downloading"|"ready"|"error", bytes, total, message}` — emitted from app launch until `multilingual-e5-large` is fully cached
- `ingest:page_started` / `ingest:page_done` / `ingest:page_failed` / `ingest:done` — per-page OCR/embed progress and final summary
- `walkthrough:token` / `walkthrough:done` — streamed *one-shot* walkthrough generation tokens (legacy)
- `walkthrough_session:token` / `walkthrough_session:done` — streamed conversational walkthrough turns; payload includes `session_id`, `phase`, and `full_content`
- `tts:done` — `{handle_id}` — fires when an `audio.speak` call's process exits (naturally or via cancel). The frontend uses this to clear `speaking` state.
- `transcribe:partial` — `{session_id, text, duration_ms}` — emitted during a push-to-talk session as the cumulative whisper-cli pass completes. Frontend filters by its own `session_id`.
- `research:started` / `research:done` — `{game_id, summary?}` — emitted by import + manual `research_run`.

## Audio / TTS providers

`src-tauri/src/audio/tts/` exposes a `TtsProvider` trait with two impls:

- `SayProvider` — wraps macOS `say(1)`, no config needed.
- `ElevenLabsProvider` — `POST /v1/text-to-speech/{voice_id}/stream?output_format=pcm_22050&optimize_streaming_latency=2` with `model_id=eleven_multilingual_v2`. Streams 16-bit little-endian mono PCM directly into a `cpal` output sink (`pcm_sink.rs`); time-to-first-audio drops to roughly the network RTT plus the first 4 KiB instead of waiting for the full MP3 to buffer. Cancel drops the response (rustls aborts mid-body) and stops the cpal stream.

`pick_provider(db)` resolves `tts_provider` (settings key) into one of three modes:

- `""` / unset → **Auto**: prefer ElevenLabs when `secrets::get_secret("elevenlabs")` returns a key, otherwise fall back to `SayProvider`.
- `"elevenlabs"` → **Force ElevenLabs**, with the same `SayProvider` fallback if the key is missing.
- `"system"` → **Force `SayProvider`** even if a key is configured.

Voice id is `tts_elevenlabs_voice_id`; default `LOL6aFvN7gBkc7zf1Co9` (the maintainer's personal cloned voice — replace via Settings → ElevenLabs).

**Boot-time bootstrap**: on launch, if no `elevenlabs.key` secret is present in the per-app secret store, `audio::tts::bootstrap_from_dev_secrets(&db)` reads `~/.config/dev-secrets/elevenlabs.env` and seeds `ELEVENLABS_API_KEY` (always, when the local secret is empty) and `ELEVENLABS_VOICE_ID` (only when the `tts_elevenlabs_voice_id` setting is empty). Idempotent; never overwrites existing values; every error path is `tracing::warn!`-and-continue so a missing/invalid env file never blocks launch.

## External rules import (BGG)

Commands: `bgg_search(query) -> Vec<BggMatch>`, `import_from_bgg(bgg_id, name_zh_override?, existing_game_id?) -> {game_id, page_count, chunk_count}`.

Pulls `<description>` + metadata from `https://boardgamegeek.com/xmlapi2/thing?id=N`, splits into ~2000-char paragraph-bounded "pages", chunks + embeds. These pages have `image_path = ""` and `ocr_status = "external"` — renderers must guard against missing images.

## Knowledge research pass (auto-runs at end of every import)

Implemented in `src-tauri/src/research/`. Runs after ingest finishes (background task) and after BGG import. Idempotent — safe to re-run. Manual trigger: `research_run(game_id) -> ResearchSummary`.

Steps (each throttled 1 req/sec):

1. Resolve `games.bgg_id` (search by `name_zh` then `name_en`); persist on the row.
2. Fetch BGG `<description>` → write to `game_external_refs(kind='description')` + chunk + embed.
3. Walk the BGG forum list, pick top "Rules"/"Reviews"/"Strategy" forums, fetch top threads (max 5) → `kind='forum'` + chunk + embed.
4. Fetch one page (50) of the BGG image gallery; record captions → `kind='gallery'` + chunk + embed.
5. Crop every illustration on the game with no `description` set, send each through Qwen-VL with `CAPTION_PROMPT`, persist on `page_illustrations.description` + chunk + embed.

Tauri events: `research:started` / `research:done` (payload includes `ResearchSummary`). The walkthrough coach's rulebook context (`commands/walkthrough_session.rs`) appends the `page_illustrations` rows + the first 8 `game_external_refs` rows to its prompt.

## Schema additions (since 0.2.0)

- `games.bgg_id INTEGER NULL` — set once BGG resolution succeeds.
- `page_illustrations.description TEXT NULL` — Qwen-VL caption.
- `game_external_refs(id, game_id, source, kind, ext_id, title, content, url, fetched_at)` with `UNIQUE(game_id, source, kind, ext_id)`. `kind ∈ {description, forum, gallery}` for now.

All additive — bootstrapped via defensive ALTER + CREATE TABLE IF NOT EXISTS in `store/db.rs`. No standalone migration file.

## Inline illustrations (token-anchored)

Qwen-VL grounded OCR is asked to emit `![label](ill:N)` markdown image tags inline at the position each illustration belongs. The N maps 1-to-1 to the index in the JSON `illustrations` array, which carries the bbox. The post-processor stamps each `Illustration` with `token: "ill:N"` and that token is persisted in `page_illustrations.token`. The React `MarkdownView` accepts an `illustrations` prop (`{ "ill:0": { image_path, label } }`) and renders matching `<img src="ill:N">` tags as actual figures via `convertFileSrc`. URL transform whitelists `ill:` so react-markdown doesn't strip it.

## Cover sourcing pipeline (auto-runs at end of ingest)

1. BGG XML API search by `name_zh`, then `name_en` → download `<image>` → save `~/games/<id>/cover.jpg`
2. Fallback: copy first imported page's thumbnail to `~/games/<id>/cover.<ext>`
3. User override at any time via Library card hover overlay → `gameSetCoverFromFile`

Commands: `game_auto_set_cover(game_id)`, `game_set_cover_from_file(game_id, src_path)`. Backend: `src-tauri/src/cover/{bgg,auto}.rs`. BGG endpoint: `https://boardgamegeek.com/xmlapi2/{search,thing}`.

## Hard invariants

- **Private repo + auto-updater**: GitHub Releases for private repos require auth on the `latest.json` endpoint. Either make the repo public OR ship a thin proxy that re-serves `latest.json`/`.dmg` with the GitHub PAT injected. **Currently the configured endpoint will 404 for end users** until this is resolved.
- **Three version bumps**: `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` must agree, otherwise updater detection fails.
- **Signing identity in conf is `"-"`**: ad-hoc local sign. CI overrides via `APPLE_SIGNING_IDENTITY` env var; do not hardcode the real identity in the conf (it's machine-specific).
- **Updater key is irreplaceable**: lose `~/.config/dev-secrets/tauri-updater/bcgAgent/private.key` and existing installs can never auto-update again — they would require manual re-download.
- **MAS sandbox incompatible**: this app uses Homebrew (`whisper-cpp`), writes outside sandbox, downloads ~1.9GB of models — Mac App Store path is closed without major rework. Stick with direct distribution.

## Self-update rule

When you change a CLI flag, schema, workflow, or invariant above, update this file in the same commit.
