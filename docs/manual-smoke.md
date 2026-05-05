# Manual smoke checklist · 手动验收

The Vitest suite (`pnpm test`) covers UI flows with mocked IPC. This checklist
covers the parts that **cannot** be automated: real Qwen-VL OCR, real MiniMax
Q&A, real microphone input, and the actual Tauri runtime.

## Prerequisites · 前置条件

1. **API keys**
   - DashScope (Qwen-VL): https://dashscope.console.aliyun.com/
   - MiniMax: https://platform.minimaxi.com/

   Drop both into your app data dir (Settings page does this for you):
   ```
   ~/Library/Application Support/bcgAgent/secrets/dashscope.key
   ~/Library/Application Support/bcgAgent/secrets/minimax.key
   ```

2. **Whisper.cpp** (only needed for voice questions)
   ```
   brew install whisper-cpp
   ```
   The model (~570MB) auto-downloads on first transcribe call.

3. **One sample handbook page**
   Take a clear photo of a real board-game rulebook page (Catan, Wingspan,
   Ark Nova — whatever you have). Save as JPG/PNG.

## Launch · 启动

```bash
cd ~/Projects/bcgAgent
pnpm tauri dev
```

Or open the built app:
```
open src-tauri/target/release/bundle/macos/桌游规则助手.app
```

## Test sequence · 测试步骤

### 1. Empty shelf
- App opens, sidebar visible (Library / Import / Ask / Settings).
- Library page shows the meeple-with-? illustration and the empty-shelf copy.
- ✅ Pass if Chinese text renders cleanly with no font fallback boxes.

### 2. Settings
- Click **Settings** in sidebar.
- Paste DashScope key into the first field, MiniMax into the second.
- Toggle the "show / hide" eye icon — confirms it's saved (not echoed).
- Click **保存 / Save** — toast: "已保存 / Saved".
- Toggle dark mode — page repaints in dark cream/ink palette. Toggle back.

### 3. Add a game + import 1 page
- Library → click **添加桌游 / Add Game**.
- Type Chinese name (e.g. `卡坦岛`), English `Catan`, publisher `Kosmos`.
- Click **创建 / Confirm** → routes to Import wizard with the game pre-selected.
- Click the dropzone, pick your sample image, click **开始导入 / Start**.
- Watch the page card status:
  - blue spinner (OCR running, calls Qwen-VL — ~3-10s)
  - green check (chunks generated, embedded, stored)
- On completion, app navigates to Handbook viewer.

### 4. Handbook viewer
- See the OCR'd Markdown rendered in the middle pane.
- Headings appear in the TOC sidebar.
- Right pane shows the original photo thumbnail; click → fullscreen modal; Esc closes.
- Type a Chinese keyword from the page into the search bar — TOC switches to hit list.
- Click a hit → middle pane scrolls to that page.

### 5. Text Q&A
- Sidebar → **问规则 / Ask**.
- Game filter chip top-left should default to your imported game.
- Type a question matching the page (e.g., `强盗怎么移动？`).
- Click **发送 / Send**.
- Citations appear first, tokens stream in. Answer cites `[卡坦岛 p.X]`.
- The new Q&A appears in the right history sidebar.

### 6. Voice Q&A
- On the Ask page, **press and hold** the round mic button. Idle = breathing
  outline; recording = pulsing red ring.
- Speak a question (Chinese or English). Release the button.
- Mic spinner appears while whisper.cpp transcribes (~1-3s).
- Transcript appears in the input AND auto-submits.
- Answer streams back as before.
- Click the volume icon top-right to enable TTS; ask another question — the
  answer is read aloud in the matching language.

### 7. Multi-game + global ask
- Add a second game, import a page, return to Ask.
- Switch filter chip to **全部 / All**.
- Ask a question that only one game's handbook can answer — confirm citations
  reference the correct game.

### Troubleshooting

| Symptom | Likely cause | Fix |
|---|---|---|
| Toast "MissingKey" | Key file empty/missing | Re-enter in Settings |
| Voice button → "麦克风权限被拒绝" | macOS blocked mic | System Settings → Privacy → Microphone → enable for the bundle |
| Transcribe error mentions whisper-cli | Not installed | `brew install whisper-cpp` |
| OCR returns empty markdown | Image unreadable | Try better lit / higher-res photo |
| App fails to launch from .app bundle | Unsigned by Apple | Right-click → Open (one-time bypass) |

## Backup / wipe

- All user data lives under `~/Library/Application Support/bcgAgent/`.
- Wipe: `rm -rf ~/Library/Application\ Support/bcgAgent`. Re-launch app to
  recreate the layout.
