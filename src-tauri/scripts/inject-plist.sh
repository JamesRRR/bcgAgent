#!/usr/bin/env bash
# Patches Info.plist of the bundled .app(s) with custom keys (Tauri 2 has no
# built-in merge), then re-packs the DMG so the distributable carries the
# patched plist too.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TAURI_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

MIC_DESC='桌游规则助手需要使用麦克风，让你用语音提问规则。Microphone access is used to ask questions by voice.'

shopt -s nullglob
APPS=("$TAURI_DIR"/target/release/bundle/macos/*.app)
if [ ${#APPS[@]} -eq 0 ]; then
  echo "inject-plist: no .app bundles to patch (skipping)"
  exit 0
fi

for APP in "${APPS[@]}"; do
  PLIST="$APP/Contents/Info.plist"
  if [ ! -f "$PLIST" ]; then
    echo "inject-plist: $PLIST missing — skipping"
    continue
  fi
  if /usr/libexec/PlistBuddy -c "Print :NSMicrophoneUsageDescription" "$PLIST" >/dev/null 2>&1; then
    /usr/libexec/PlistBuddy -c "Set :NSMicrophoneUsageDescription $MIC_DESC" "$PLIST"
  else
    /usr/libexec/PlistBuddy -c "Add :NSMicrophoneUsageDescription string $MIC_DESC" "$PLIST"
  fi
  echo "inject-plist: patched $PLIST"
done

# Repack any DMGs alongside the patched .apps so the distributable carries
# the fresh Info.plist. We rebuild a plain hdiutil image (no fancy DMG layout
# — Tauri's `create-dmg` script is regenerated each build, so re-invoking it
# here would also work but adds complexity; this gets a working .dmg).
DMGS=("$TAURI_DIR"/target/release/bundle/dmg/*.dmg)
if [ ${#DMGS[@]} -eq 0 ]; then
  exit 0
fi

for OLD_DMG in "${DMGS[@]}"; do
  NAME="$(basename "$OLD_DMG" .dmg)"
  STAGE_DIR="$(mktemp -d)"
  for APP in "${APPS[@]}"; do
    cp -R "$APP" "$STAGE_DIR/"
  done
  ln -s /Applications "$STAGE_DIR/Applications"

  TMP_DMG="$(mktemp -d)/${NAME}.dmg"
  rm -f "$OLD_DMG"
  hdiutil create -volname "$NAME" -srcfolder "$STAGE_DIR" -ov -format UDZO "$TMP_DMG" >/dev/null
  mv "$TMP_DMG" "$OLD_DMG"
  rm -rf "$STAGE_DIR"
  echo "inject-plist: repacked $OLD_DMG"
done
