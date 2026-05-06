# bcgAgent — Agent Surface

Macos Tauri 2 app for boardgame rule books: photo → OCR → RAG → voice/text Q&A. Chinese-first.

## Quick facts

- **Bundle ID**: `com.bcgagent.app`
- **Display name**: 攀达桌游
- **Repo**: https://github.com/JamesRRR/bcgAgent (private)
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
3. GitHub Action `.github/workflows/release.yml` runs on `macos-14` (ARM) + `macos-13` (Intel).
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

## Hard invariants

- **Private repo + auto-updater**: GitHub Releases for private repos require auth on the `latest.json` endpoint. Either make the repo public OR ship a thin proxy that re-serves `latest.json`/`.dmg` with the GitHub PAT injected. **Currently the configured endpoint will 404 for end users** until this is resolved.
- **Three version bumps**: `package.json`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json` must agree, otherwise updater detection fails.
- **Signing identity in conf is `"-"`**: ad-hoc local sign. CI overrides via `APPLE_SIGNING_IDENTITY` env var; do not hardcode the real identity in the conf (it's machine-specific).
- **Updater key is irreplaceable**: lose `~/.config/dev-secrets/tauri-updater/bcgAgent/private.key` and existing installs can never auto-update again — they would require manual re-download.
- **MAS sandbox incompatible**: this app uses Homebrew (`whisper-cpp`), writes outside sandbox, downloads ~1.9GB of models — Mac App Store path is closed without major rework. Stick with direct distribution.

## Self-update rule

When you change a CLI flag, schema, workflow, or invariant above, update this file in the same commit.
