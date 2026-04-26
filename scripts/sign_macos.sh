#!/usr/bin/env bash
set -euo pipefail

# ---------------------------------------------------------------------------
# sign_macos.sh — Code-signs the .app bundle and optionally notarizes the DMG
#
# Usage:
#   bash scripts/sign_macos.sh [path/to/MowisAI.app]
#
# Environment variables (required for notarization; not needed for ad-hoc):
#   MACOS_SIGNING_IDENTITY  — Developer ID Application: ... (default: "-" for ad-hoc)
#   APPLE_ID                — Apple ID email used for notarization
#   TEAM_ID                 — Apple Developer team ID (10-char alphanumeric)
#   APP_PASSWORD            — App-specific password for notarytool
#
# Ad-hoc signing ("-") is useful for local testing; the resulting binary runs
# only on the machine it was signed on (Gatekeeper will block distribution).
# ---------------------------------------------------------------------------

APP="${1:-dist/MowisAI.app}"
DMG="dist/MowisAI.dmg"
IDENTITY="${MACOS_SIGNING_IDENTITY:-"-"}"

# ---------------------------------------------------------------------------
# Dependency checks
# ---------------------------------------------------------------------------
echo "Checking dependencies..."

if ! command -v codesign &>/dev/null; then
  echo "ERROR: codesign not found. This script must run on macOS." >&2
  exit 1
fi

if [[ ! -d "$APP" ]]; then
  echo "ERROR: App bundle not found at '${APP}'." >&2
  echo "       Run 'bash scripts/bundle_macos.sh' first." >&2
  exit 1
fi

# ---------------------------------------------------------------------------
# Code signing
# ---------------------------------------------------------------------------
echo "Signing '${APP}' with identity '${IDENTITY}'..."

codesign \
  --deep \
  --force \
  --options runtime \
  --sign "$IDENTITY" \
  "$APP"

echo "Signed: ${APP}"

# Verify the signature
echo "Verifying signature..."
codesign --verify --verbose=2 "$APP"
echo "Signature OK."

# ---------------------------------------------------------------------------
# Notarization (only when a real Developer ID identity is provided)
# ---------------------------------------------------------------------------
if [[ "$IDENTITY" == "-" ]]; then
  echo ""
  echo "Ad-hoc signing complete. Skipping notarization (identity is '-')."
  echo "To notarize, set MACOS_SIGNING_IDENTITY, APPLE_ID, TEAM_ID, and APP_PASSWORD."
  exit 0
fi

# Ensure the DMG exists before attempting notarization
if [[ ! -f "$DMG" ]]; then
  echo "ERROR: DMG not found at '${DMG}'. Cannot notarize without a DMG." >&2
  echo "       Run 'bash scripts/bundle_macos.sh' after signing to build the DMG." >&2
  exit 1
fi

# Validate required env vars for notarization
for var in APPLE_ID TEAM_ID APP_PASSWORD; do
  if [[ -z "${!var:-}" ]]; then
    echo "ERROR: Environment variable '${var}' is required for notarization." >&2
    exit 1
  fi
done

echo ""
echo "Submitting '${DMG}' for notarization (this may take a few minutes)..."

xcrun notarytool submit "$DMG" \
  --apple-id  "$APPLE_ID" \
  --team-id   "$TEAM_ID" \
  --password  "$APP_PASSWORD" \
  --wait

echo "Notarization complete."

# Staple the notarization ticket so the app works offline
echo "Stapling notarization ticket to ${APP}..."
xcrun stapler staple "$APP"
echo "Stapled."
