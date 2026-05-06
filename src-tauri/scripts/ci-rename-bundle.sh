#!/usr/bin/env bash
# Post-build hook for CI: rename the bundled `.app` *inside the DMG* so users
# see the Chinese display name (攀达桌游) when dragging from the .dmg into
# /Applications.
#
# Why only the DMG: tauri-action strips non-ASCII characters from upload
# filenames, so artifact basenames must stay ASCII for `latest.json`
# signature lookup to work. The DMG's *filename* stays ASCII; only the .app
# folder *inside* the DMG is renamed. The updater tarball (.app.tar.gz) and
# its signature are left untouched — auto-updates work because Tauri's
# updater replaces files inside the existing .app bundle, regardless of the
# bundle's display name.

set -euo pipefail

TARGET="${TARGET:-aarch64-apple-darwin}"
DISPLAY_NAME="${DISPLAY_NAME:-攀达桌游}"
ASCII_NAME="bcgAgent"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TAURI_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
BUNDLE_DIR="$TAURI_DIR/target/$TARGET/release/bundle"
MACOS_DIR="$BUNDLE_DIR/macos"
DMG_DIR="$BUNDLE_DIR/dmg"

shopt -s nullglob
DMG_FILES=("$DMG_DIR"/*.dmg)
if [ ${#DMG_FILES[@]} -eq 0 ]; then
  echo "ci-rename-bundle: no DMG found in $DMG_DIR — skipping"
  exit 0
fi

OLD_DMG="${DMG_FILES[0]}"
DMG_BASENAME="$(basename "$OLD_DMG")"
echo "→ Repacking DMG: $DMG_BASENAME"

# Mount the existing DMG and copy out the .app, since tauri's bundler may
# have produced a fancier DMG layout (icon positions, background) that we
# don't want to lose.
MOUNT_DIR="$(mktemp -d)"
hdiutil attach "$OLD_DMG" -nobrowse -mountpoint "$MOUNT_DIR" >/dev/null

STAGE_DIR="$(mktemp -d)"
SRC_APP=""
for app in "$MOUNT_DIR"/*.app; do
  SRC_APP="$app"
  break
done
if [ -z "$SRC_APP" ]; then
  hdiutil detach "$MOUNT_DIR" -force >/dev/null || true
  echo "ci-rename-bundle: no .app inside mounted DMG — aborting"
  exit 1
fi

cp -R "$SRC_APP" "$STAGE_DIR/$DISPLAY_NAME.app"
ln -s /Applications "$STAGE_DIR/Applications"
hdiutil detach "$MOUNT_DIR" -force >/dev/null

TMP_DMG="$(mktemp -d)/$DMG_BASENAME"
hdiutil create \
  -volname "$DISPLAY_NAME" \
  -srcfolder "$STAGE_DIR" \
  -ov \
  -format UDZO \
  "$TMP_DMG" >/dev/null

# Re-sign DMG so Gatekeeper doesn't reject it. The .app inside is already
# notarized + stapled by Tauri's pipeline, so DMG-level notarization is not
# strictly required for first-launch.
if [ -n "${APPLE_SIGNING_IDENTITY:-}" ]; then
  echo "→ Re-signing DMG with Apple identity"
  codesign --force --sign "$APPLE_SIGNING_IDENTITY" --timestamp "$TMP_DMG"
fi

mv "$TMP_DMG" "$OLD_DMG"
rm -rf "$STAGE_DIR"
echo "✓ Repacked $OLD_DMG — .app inside is now $DISPLAY_NAME.app"
