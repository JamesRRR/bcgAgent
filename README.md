# bcgAgent · 桌游规则助手

一个 macOS 桌面应用，帮助你把纸质桌游规则书拍照导入、转成可搜索的电子手册，并用语音/文字提问规则。中文优先，兼容英文。

A macOS desktop app that turns photos of board-game rulebooks into searchable, voice-queryable knowledge bases. Chinese-first, English supported.

---

## 功能 · Features

- 📷 **拍照导入**：拖入规则书每一页的照片，自动 OCR 成结构化文本（保留章节、表格、图标说明）。
- 📚 **电子手册**：按桌游分组浏览，TOC 侧栏导航，关键字高亮。
- 🎤 **语音提问**：按住麦克风提问，回答按规则书原文给出，并附页码引用。可选朗读。
- 🔍 **混合检索**：语义搜索 (BGE-M3) + 关键字搜索 (FTS5+jieba)。
- 🎲 **趣味界面**：木头书架风格、自定义 SVG meeple 插画、暖色 cream 主题。

## 技术栈 · Stack

| 层 | 选型 |
|---|---|
| Shell | Tauri 2 + React 18 + TypeScript + Vite |
| OCR | DashScope `qwen-vl-max-latest`（阿里云）|
| Q&A LLM | MiniMax chat completions |
| STT | whisper.cpp (`ggml-large-v3-turbo-q5_0`) |
| TTS | macOS `say`（`Tingting` 中文 / `Samantha` 英文）|
| Embeddings | BGE-M3（fastembed-rs，本地，1024 维）|
| Storage | SQLite + sqlite-vec + FTS5 (jieba 分词) |

## 准备 · Prerequisites

- macOS（Apple Silicon 推荐）
- Node 20+, pnpm
- Rust（`rustup` 安装即可）
- Xcode Command Line Tools

```bash
xcode-select --install
brew install pnpm rustup-init
rustup-init -y
```

## API Keys

把密钥放到（首次运行后会自动创建目录）：

```
~/Library/Application Support/bcgAgent/secrets/dashscope.key   # 单行，DashScope API Key
~/Library/Application Support/bcgAgent/secrets/minimax.key     # 单行，MiniMax API Key
```

也可以在应用内 Settings 页面填写。

## 开发 · Development

```bash
pnpm install
pnpm tauri dev
```

首次启动会下载 BGE-M3 (~440MB) 和 Whisper 模型 (~570MB) 到 `~/Library/Application Support/bcgAgent/models/`。请保持网络畅通。

## 构建 · Build

```bash
pnpm tauri build
```

产物在 `src-tauri/target/release/bundle/dmg/`。

## 数据位置 · Data location

```
~/Library/Application Support/bcgAgent/
├── db.sqlite
├── games/<game_id>/pages/<n>.jpg
├── games/<game_id>/thumbs/<n>.webp
├── audio/qa/<qa_id>.wav
├── secrets/{dashscope,minimax}.key
└── models/
    ├── bge-m3/
    └── whisper/ggml-large-v3-turbo-q5_0.bin
```

整目录可拷贝迁移。

## 协议 · License

个人项目，未公开发布。Personal project; not for redistribution.
