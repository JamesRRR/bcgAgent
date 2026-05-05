---
name: "桌游规则助手"
description: "拍照导入桌游规则书、语音/文字提问，本地 RAG + 中文优先 OCR/LLM"
category: "macos-app"
status: "active"
icon: "dices"
version: "0.1.0"
tech:
  - "Tauri 2"
  - "Rust"
  - "React 18"
  - "TypeScript"
  - "SQLite + sqlite-vec + FTS5"
  - "fastembed (multilingual-E5)"
  - "Qwen-VL (DashScope)"
  - "MiniMax-M2"
  - "whisper.cpp"
launch:
  type: "app"
  command: "open -a 桌游规则助手"
tags:
  - "boardgame"
  - "rag"
  - "voice"
  - "ocr"
  - "chinese-first"
created: "2026-05-05"
updated: "2026-05-05"
---

# bcgAgent · 桌游规则助手

一个 macOS 桌面应用：拍照导入桌游规则书每一页，自动 OCR 成结构化电子手册，再用语音或文字按规则提问，回答附带页码引用。中文优先。

## 启动

直接打开 `/Applications/桌游规则助手.app`，或在 Launchr 里点击启动。

首次使用前请在 **设置** 中填写：

- **DashScope (Qwen-VL) API Key** — 用于 OCR
- **MiniMax API Key** — 用于回答规则提问

也可以直接写入文件：

```
~/Library/Application Support/bcgAgent/secrets/dashscope.key
~/Library/Application Support/bcgAgent/secrets/minimax.key
```

第一次导入时会下载约 1.3 GB 的本地嵌入模型；第一次语音提问时会下载约 570 MB 的 whisper 模型。语音功能需要：

```
brew install whisper-cpp
```

## 主要功能

| 模块 | 说明 |
|---|---|
| 📚 书架 | 按桌游分组的电子手册库，悬停倾斜动效，自动生成封面 |
| 📥 导入 | 拖入图片 → Qwen-VL OCR 转结构化 Markdown → BGE/E5 嵌入 → SQLite 持久化 |
| 📖 规则书 | 分页阅读，TOC 侧栏，关键字高亮，原图缩放 |
| 🎤 问规则 | 语音/文字提问，混合检索（向量 + FTS5+jieba），MiniMax 流式回答 + 页码引用，可选朗读 |
| ⚙️ 设置 | API Key、TTS 语言、检索条数 K、深色模式 |

## 数据位置

```
~/Library/Application Support/bcgAgent/
├── db.sqlite                            # SQLite，含 vec0 + FTS5
├── games/<game_id>/{pages,thumbs}/      # 原始图片 + 缩略图
├── audio/qa/                            # 语音问题录音
├── secrets/{dashscope,minimax}.key      # 0600 权限
└── models/{bge-m3,whisper}/             # 模型缓存
```

整目录可拷贝迁移。

## 开发

```bash
cd ~/Projects/bcgAgent
pnpm install
pnpm tauri dev          # 开发模式
pnpm test               # Vitest UI 交互测试 (10 项)
pnpm exec playwright test   # 端到端真实点击测试 (Playwright)
cd src-tauri && cargo test --lib                                # 后端单元测试
cargo test --test e2e_pipeline -- --ignored --nocapture         # 真实 OCR + LLM + 嵌入端到端
cargo test --test voice_roundtrip -- --ignored --nocapture      # 真实 STT/TTS 往返
```

## 构建分发

```bash
pnpm tauri:build        # 注意冒号 — 会自动打 Info.plist 麦克风权限并重打包 DMG
```

产物：`src-tauri/target/release/bundle/dmg/bcgAgent_0.1.0_aarch64.dmg`
