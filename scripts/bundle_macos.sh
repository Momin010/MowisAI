#!/usr/bin/env bash
set -euo pipefail

# ---------------------------------------------------------------------------
# bundle_macos.sh — Creates a .app bundle and .dmg installer for MowisAI
# ---------------------------------------------------------------------------

BINARY_SRC="target/release/mowisai"
APP_NAME="MowisAI"
BUNDLE_ID="com.mowisai.app"
VERSION="0.1.0"
DIST_DIR="dist"
APP_BUNDLE="${DIST_DIR}/${APP_NAME}.app"
DMG_PATH="${DIST_DIR}/${APP_NAME}.dmg"

# ---------------------------------------------------------------------------
# Dependency checks
# ---------------------------------------------------------------------------
echo "Checking dependencies..."

if ! command -v hdiutil &>/dev/null; then
  echo "ERROR: hdiutil not found. This script must run on macOS." >&2
  exit 1
fi

if [[ ! -f "$BINARY_SRC" ]]; then
  echo "ERROR: Binary not found at '${BINARY_SRC}'." >&2
  echo "       Run 'cargo build --release -p mowis-gui' first." >&2
  exit 1
fi

# ---------------------------------------------------------------------------
# Create bundle directory structure
# ---------------------------------------------------------------------------
echo "Creating .app bundle at ${APP_BUNDLE}..."

MACOS_DIR="${APP_BUNDLE}/Contents/MacOS"
RESOURCES_DIR="${APP_BUNDLE}/Contents/Resources"

rm -rf "$APP_BUNDLE"
mkdir -p "$MACOS_DIR"
mkdir -p "$RESOURCES_DIR"

# ---------------------------------------------------------------------------
# Copy binary
# ---------------------------------------------------------------------------
echo "Copying binary..."
cp "$BINARY_SRC" "${MACOS_DIR}/mowisai"
chmod +x "${MACOS_DIR}/mowisai"

# ---------------------------------------------------------------------------
# Icon — copy if present, otherwise write a placeholder note
# ---------------------------------------------------------------------------
ICON_SRC="assets/AppIcon.icns"
if [[ -f "$ICON_SRC" ]]; then
  echo "Copying icon from ${ICON_SRC}..."
  cp "$ICON_SRC" "${RESOURCES_DIR}/AppIcon.icns"
else
  echo "WARNING: No icon found at '${ICON_SRC}'. Skipping icon (bundle will use default macOS icon)."
  # Create a zero-byte placeholder so the Resources dir is non-empty and
  # Info.plist CFBundleIconFile key still resolves gracefully.
  touch "${RESOURCES_DIR}/AppIcon.icns"
fi

# ---------------------------------------------------------------------------
# Write Info.plist
# ---------------------------------------------------------------------------
echo "Writing Info.plist..."
cat > "${APP_BUNDLE}/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleExecutable</key><string>mowisai</string>
  <key>CFBundleIdentifier</key><string>${BUNDLE_ID}</string>
  <key>CFBundleName</key><string>${APP_NAME}</string>
  <key>CFBundleVersion</key><string>${VERSION}</string>
  <key>CFBundleShortVersionString</key><string>${VERSION}</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSMinimumSystemVersion</key><string>10.15</string>
</dict>
</plist>
PLIST

# ---------------------------------------------------------------------------
# Create DMG
# ---------------------------------------------------------------------------
echo "Creating DMG at ${DMG_PATH}..."

# Remove stale DMG if present
rm -f "$DMG_PATH"

hdiutil create \
  -volname "${APP_NAME}" \
  -srcfolder "${APP_BUNDLE}" \
  -ov \
  -format UDZO \
  "$DMG_PATH"

# ---------------------------------------------------------------------------
# Print results
# ---------------------------------------------------------------------------
echo ""
echo "Build complete."
echo "  .app bundle : ${APP_BUNDLE}"
echo "  .dmg path   : ${DMG_PATH}"
echo ""
echo "SHA256:"
if command -v shasum &>/dev/null; then
  shasum -a 256 "$DMG_PATH"
elif command -v sha256sum &>/dev/null; then
  sha256sum "$DMG_PATH"
else
  echo "WARNING: No sha256 tool found; skipping checksum." >&2
fi
