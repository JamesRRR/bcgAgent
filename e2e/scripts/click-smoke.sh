#!/usr/bin/env bash
# Click-driven smoke test against the REAL packaged Tauri app.
#
# This is the only test that exercises the real Tauri IPC dispatcher.
# Vitest mocks invoke; Playwright + HTTP shim parses with serde defaults.
# Both miss Tauri's argument-rename rules — this test catches them.
#
# Drives the bundled .app via macOS Accessibility (osascript + cliclick):
#   1. Wipe DB and games dir
#   2. Launch /Applications/攀达桌游.app
#   3. Click "添加桌游" → fill form → submit (game_create)
#   4. Click 书架 (games_list)
#   5. Click the game card (pages_list_by_game) — the originally failing path
#   6. Verify Handbook UI rendered without an error toast
#
# Exits 0 on success, non-zero on any failure.

set -uo pipefail

APP_PATH="/Applications/攀达桌游.app"
APP_DATA="$HOME/Library/Application Support/bcgAgent"

red()   { printf "\033[31m%s\033[0m\n" "$*" >&2; }
green() { printf "\033[32m%s\033[0m\n" "$*"; }
step()  { printf "\033[36m[click-smoke] %s\033[0m\n" "$*"; }
fail()  { red   "FAIL: $*"; cleanup; exit 1; }

cleanup() {
  pkill -f "$APP_PATH" 2>/dev/null || true
}
trap cleanup EXIT

if ! command -v cliclick >/dev/null; then
  fail "cliclick not installed (brew install cliclick)"
fi
if [ ! -d "$APP_PATH" ]; then
  fail "$APP_PATH not found — run: pnpm tauri:build && cp -R src-tauri/target/release/bundle/macos/攀达桌游.app /Applications/"
fi

# Reset state so we run from a clean shelf
step "wiping app data"
rm -f "$APP_DATA/db.sqlite"
rm -rf "$APP_DATA/games"
mkdir -p "$APP_DATA/games"

# Launch
step "launching app"
pkill -f "$APP_PATH" 2>/dev/null || true
sleep 0.5
open "$APP_PATH"

# Wait until the window is up
for i in $(seq 1 30); do
  if pgrep -f "$APP_PATH/Contents/MacOS/bcgagent" >/dev/null; then break; fi
  sleep 0.5
done
sleep 2  # give frontend a beat to render

# Helper: collect both AXStaticText values AND AXButton titles from the front
# window. Web-rendered button labels surface as the button's `title` attribute,
# not as separate static text.
collect_text() {
  osascript <<'EOF' 2>/dev/null
tell application "System Events"
  tell process "bcgagent"
    set out to ""
    try
      set els to entire contents of front window
      repeat with el in els
        try
          set r to role of el
          if r is "AXStaticText" then
            set v to value of el
            if v is not missing value and v is not "" then
              set out to out & v & "|"
            end if
          else if r is "AXButton" then
            set t to ""
            try
              set t to title of el
            end try
            if t is not missing value and t is not "" then
              set out to out & t & "|"
            end if
          end if
        end try
      end repeat
    end try
    return out
  end tell
end tell
EOF
}

# Helper: get position of a button by exact title (for cliclick coords)
btn_center() {
  local title="$1"
  osascript 2>/dev/null <<EOF
tell application "System Events"
  tell process "bcgagent"
    set out to ""
    set els to entire contents of front window
    repeat with el in els
      try
        if role of el is "AXButton" and title of el is "$title" then
          set p to position of el
          set sz to size of el
          set cx to (item 1 of p) + ((item 1 of sz) div 2)
          set cy to (item 2 of p) + ((item 2 of sz) div 2)
          set out to (cx as text) & "," & (cy as text)
          exit repeat
        end if
      end try
    end repeat
    return out
  end tell
end tell
EOF
}

assert_contains() {
  local text="$1"; local needle="$2"; local label="$3"
  if [[ "$text" == *"$needle"* ]]; then
    green "  ✓ $label"
  else
    fail "$label — expected to find '$needle' in: $text"
  fi
}

# 1) Empty shelf
step "verify empty shelf"
osascript -e 'tell application "System Events" to tell process "bcgagent" to set frontmost to true' >/dev/null
sleep 0.5
TEXT="$(collect_text)"
# Empty state shows the EmptyShelf SVG copy, NOT the title heading.
assert_contains "$TEXT" "桌游架空空如也" "empty-shelf copy rendered"

# 2) Click 添加桌游 (in empty state, the button is in the page body)
step "click 添加桌游"
COORDS="$(btn_center "添加桌游")"
[ -n "$COORDS" ] || fail "couldn't locate 添加桌游 button"
cliclick "c:$COORDS" >/dev/null
sleep 1
TEXT="$(collect_text)"
assert_contains "$TEXT" "新增桌游" "Add Game dialog opened"

# 3) Fill form via clipboard paste (works for Chinese)
step "fill name_zh"
osascript -e 'set the clipboard to "卡坦岛"' >/dev/null
osascript -e 'tell application "System Events" to keystroke "v" using command down' >/dev/null
sleep 0.3
osascript -e 'tell application "System Events" to keystroke tab' >/dev/null
sleep 0.2
osascript -e 'set the clipboard to "Catan"' >/dev/null
osascript -e 'tell application "System Events" to keystroke "v" using command down' >/dev/null
sleep 0.3
osascript -e 'tell application "System Events" to keystroke tab' >/dev/null
sleep 0.2
osascript -e 'set the clipboard to "Kosmos"' >/dev/null
osascript -e 'tell application "System Events" to keystroke "v" using command down' >/dev/null
sleep 0.3

# 4) Click 创建 — exercises game_create (snake_case args)
step "click 创建 (game_create command)"
COORDS="$(btn_center "创建")"
[ -n "$COORDS" ] || fail "couldn't locate 创建 button"
cliclick "c:$COORDS" >/dev/null
sleep 3
TEXT="$(collect_text)"
# We should be on Import page now
assert_contains "$TEXT" "导入规则书" "navigated to Import wizard after game_create"
# A regression on rename_all would surface as a toast like "missing required key"
if [[ "$TEXT" == *"missing required key"* ]] || [[ "$TEXT" == *"invalid args"* ]]; then
  fail "Tauri IPC arg-name regression: $TEXT"
fi

# 5) Click 书架 in sidebar — exercises games_list
step "click 桌游架 (games_list command)"
COORDS="$(btn_center "桌游架")"
[ -n "$COORDS" ] || fail "couldn't locate 桌游架 button"
cliclick "c:$COORDS" >/dev/null
sleep 2
TEXT="$(collect_text)"
assert_contains "$TEXT" "我的桌游架" "back on Library after games_list"
assert_contains "$TEXT" "卡坦岛" "new game card appears in shelf"

# 6) Click the 重命名 (rename) pencil — exercises game_rename
step "click 重命名 (game_rename command)"
COORDS="$(btn_center "重命名")"
[ -n "$COORDS" ] || fail "couldn't locate 重命名 button (rename pencil)"
cliclick "c:$COORDS" >/dev/null
sleep 1
TEXT="$(collect_text)"
assert_contains "$TEXT" "重命名桌游" "rename dialog opened"

# Dialog auto-selects on open, but React re-render can race the select() —
# do an explicit CMD+A so the existing text is replaced, not appended.
osascript -e 'tell application "System Events" to keystroke "a" using command down' >/dev/null
sleep 0.2
osascript -e 'set the clipboard to "卡坦岛改名"' >/dev/null
osascript -e 'tell application "System Events" to keystroke "v" using command down' >/dev/null
sleep 0.3

step "click 保存 (game_rename submit)"
COORDS="$(btn_center "保存")"
[ -n "$COORDS" ] || fail "couldn't locate 保存 button"
cliclick "c:$COORDS" >/dev/null
sleep 3
TEXT="$(collect_text)"
if [[ "$TEXT" == *"missing required key"* ]] || [[ "$TEXT" == *"invalid args"* ]]; then
  fail "Tauri IPC arg-name regression on game_rename: $TEXT"
fi
assert_contains "$TEXT" "卡坦岛改名" "shelf shows renamed game"

# 7) Click the renamed card — exercises pages_list_by_game (the originally broken path)
# AX tree can lag after React re-render; retry up to 5×
step "click 卡坦岛改名 card (pages_list_by_game command)"
COORDS=""
for i in 1 2 3 4 5; do
  COORDS="$(btn_center "卡坦岛改名")"
  [ -n "$COORDS" ] && break
  sleep 1
done
if [ -z "$COORDS" ]; then
  echo "DEBUG dump after rename: $TEXT" >&2
  fail "couldn't locate 卡坦岛改名 game card (AX cache stale?)"
fi
cliclick "c:$COORDS" >/dev/null
sleep 3
TEXT="$(collect_text)"
if [[ "$TEXT" == *"missing required key"* ]] || [[ "$TEXT" == *"invalid args"* ]]; then
  fail "Tauri IPC arg-name regression on pages_list_by_game: $TEXT"
fi
# Empty-handbook copy "本页暂无可用文字" or empty-state "添加页面" indicates we reached Handbook
if [[ "$TEXT" == *"本页暂无可用文字"* ]] || [[ "$TEXT" == *"添加页面"* ]]; then
  green "  ✓ Handbook rendered without IPC error"
else
  fail "Handbook page didn't render as expected: $TEXT"
fi

green ""
green "════════════════════════════════════════════════"
green "  click-smoke PASSED — Tauri IPC end-to-end OK"
green "════════════════════════════════════════════════"
exit 0
