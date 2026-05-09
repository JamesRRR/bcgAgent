#!/usr/bin/env bash
# Click-test for the 新手向导 (Beginner Mode) flow against the running app.
# Drives whichever bcgagent window is frontmost. Does NOT wipe data.
#
# Verifies the user-reported bug: clicking 生成向导 silently does nothing.
# After this test, we should see EITHER streaming text OR a visible error
# alert — never an unchanged empty state.
#
# Prereqs: cliclick, an imported game in the library.

set -uo pipefail

PROC="${PROC:-bcgagent}"
DEV_LOG="${DEV_LOG:-/private/tmp/claude-501/-Users-bingyanren-Projects-bcgAgent/af35451c-42f5-4513-af3d-174ee6b707fe/tasks/b2bl6w0ld.output}"

red()   { printf "\033[31m%s\033[0m\n" "$*" >&2; }
green() { printf "\033[32m%s\033[0m\n" "$*"; }
step()  { printf "\033[36m[walkthrough-test] %s\033[0m\n" "$*"; }
fail()  { red "FAIL: $*"; exit 1; }

if ! command -v cliclick >/dev/null; then
  fail "cliclick not installed"
fi

osascript -e "tell application \"System Events\" to tell process \"$PROC\" to set frontmost to true" >/dev/null
sleep 1

collect_text() {
  osascript <<EOF 2>/dev/null
tell application "System Events"
  tell process "$PROC"
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
              set out to out & "[btn]" & t & "|"
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

btn_center() {
  local title="$1"
  osascript 2>/dev/null <<EOF
tell application "System Events"
  tell process "$PROC"
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

# 1. Make sure we're on the Library page first.
step "navigate to Library"
COORDS="$(btn_center "桌游架")"
[ -n "$COORDS" ] && cliclick "c:$COORDS" >/dev/null && sleep 1

step "snapshot library"
TEXT="$(collect_text)"
echo "${TEXT}" | tr '|' '\n' | grep -v '^$' | head -30

# 2. Find any game name and click it.
step "click first game card"
# The first card title is whichever game name appears as a button after 添加桌游.
GAME_BTN=$(echo "$TEXT" | tr '|' '\n' | grep '^\[btn\]' | grep -v '^\[btn\]添加桌游' | grep -v '^\[btn\]桌游架' | grep -v '^\[btn\]导入' | grep -v '^\[btn\]问规则' | grep -v '^\[btn\]设置' | head -1 | sed 's/^\[btn\]//')
[ -n "$GAME_BTN" ] || fail "no game found in library — import one first"
echo "  picking: $GAME_BTN"
COORDS="$(btn_center "$GAME_BTN")"
[ -n "$COORDS" ] || fail "couldn't get coords for $GAME_BTN"
cliclick "c:$COORDS" >/dev/null
sleep 2

# 3. We're now on the Handbook page. Click the 新手向导 header button.
step "click 新手向导 in handbook header"
COORDS="$(btn_center "新手向导")"
[ -n "$COORDS" ] || fail "couldn't find 新手向导 button on handbook header"
cliclick "c:$COORDS" >/dev/null
sleep 2

# 4. We should now be on the Walkthrough page. Snapshot.
step "snapshot walkthrough page (before click)"
TEXT_BEFORE="$(collect_text)"
echo "${TEXT_BEFORE}" | tr '|' '\n' | grep -v '^$' | head -20

# 5. Click 生成向导 — the bug repro.
step "click 生成向导"
COORDS="$(btn_center "生成向导")"
[ -n "$COORDS" ] || fail "couldn't find 生成向导 button"
cliclick "c:$COORDS" >/dev/null

# 6. Wait up to 30s for either streaming text or a visible error.
step "wait for streaming text or error (up to 30s)"
for i in $(seq 1 30); do
  sleep 1
  TEXT_AFTER="$(collect_text)"
  # Check for streaming-state indicators or content.
  if [[ "$TEXT_AFTER" == *"生成中"* ]]; then
    green "  ✓ streaming state detected after ${i}s"
    STREAMING_SAW=1
    break
  fi
  if [[ "$TEXT_AFTER" == *"## 游戏目标"* ]] || [[ "$TEXT_AFTER" == *"游戏目标"* ]]; then
    green "  ✓ generated content detected after ${i}s"
    STREAMING_SAW=1
    break
  fi
  if [[ "$TEXT_AFTER" == *"出错"* ]] || [[ "$TEXT_AFTER" == *"error"* ]] || [[ "$TEXT_AFTER" == *"failed"* ]]; then
    green "  ✓ error message surfaced after ${i}s (visible to user)"
    STREAMING_SAW=1
    break
  fi
done

if [ -z "${STREAMING_SAW:-}" ]; then
  red "FAIL: no state change after 30s — bug repro confirmed"
  echo "BEFORE:"
  echo "${TEXT_BEFORE}" | tr '|' '\n' | grep -v '^$'
  echo "AFTER:"
  echo "${TEXT_AFTER}" | tr '|' '\n' | grep -v '^$'
  echo "DEV LOG TAIL:"
  tail -30 "$DEV_LOG" 2>/dev/null || echo "(no dev log)"
  exit 1
fi

# 7. Wait for completion (or settled state) and capture final content.
step "wait for completion (up to 60s)"
for i in $(seq 1 60); do
  sleep 1
  TEXT_FINAL="$(collect_text)"
  if [[ "$TEXT_FINAL" != *"生成中"* ]] && [[ "$TEXT_FINAL" == *"游戏目标"* ]]; then
    green "  ✓ generation completed after ${i}s"
    break
  fi
  if [[ "$TEXT_FINAL" == *"出错"* ]]; then
    red "FAIL: generation errored: see screenshot for details"
    echo "${TEXT_FINAL}" | tr '|' '\n' | grep -v '^$' | head -40
    exit 1
  fi
done

green ""
green "════════════════════════════════════════════════"
green "  walkthrough-click-test PASSED"
green "════════════════════════════════════════════════"
echo "${TEXT_FINAL}" | tr '|' '\n' | grep -v '^$' | head -30
