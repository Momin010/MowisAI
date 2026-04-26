#!/usr/bin/env bash
set -euo pipefail

# ---------------------------------------------------------------------------
# build_macos.sh — Master build script for the MowisAI macOS release
#
# Steps:
#   1. cargo build --release -p mowis-gui
#   2. bash scripts/bundle_macos.sh  (creates .app + .dmg)
#   3. (Optional) bash scripts/sign_macos.sh  (set MACOS_SIGNING_IDENTITY to enable)
#
# Usage:
#   bash scripts/build_macos.sh
#
# Set MACOS_SIGNING_IDENTITY to sign the bundle after packaging:
#   MACOS_SIGNING_IDENTITY="Developer ID Application: Acme Corp (XXXXXXXXXX)" \
#     bash scripts/build_macos.sh
# ---------------------------------------------------------------------------

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Run everything from the repo root so relative paths (target/, dist/, etc.) work
cd "$REPO_ROOT"

# ---------------------------------------------------------------------------
# Dependency checks
# ---------------------------------------------------------------------------
echo "Checking dependencies..."

if ! command -v cargo &>/dev/null; then
  echo "ERROR: cargo not found. Install Rust from https://rustup.rs/" >&2
  exit 1
fi

if ! command -v hdiutil &>/dev/null; then
  echo "ERROR: hdiutil not found. This script must run on macOS." >&2
  exit 1
fi

# ---------------------------------------------------------------------------
# Step 1: Compile
# ---------------------------------------------------------------------------
echo ""
echo "==> Building mowisai for macOS (release)..."
cargo build --release -p mowis-gui
echo "Build succeeded."

# ---------------------------------------------------------------------------
# Step 2: Bundle (.app + .dmg)
# ---------------------------------------------------------------------------
echo ""
echo "==> Creating .app bundle and .dmg installer..."
bash "${SCRIPT_DIR}/bundle_macos.sh"

# ---------------------------------------------------------------------------
# Step 3: Optional code signing
# ---------------------------------------------------------------------------
if [[ -n "${MACOS_SIGNING_IDENTITY:-}" ]]; then
  echo ""
  echo "==> Signing bundle (MACOS_SIGNING_IDENTITY is set)..."
  bash "${SCRIPT_DIR}/sign_macos.sh" dist/MowisAI.app
else
  echo ""
  echo "Skipping code signing (MACOS_SIGNING_IDENTITY not set)."
  echo "To sign, run: MACOS_SIGNING_IDENTITY=\"-\" bash scripts/sign_macos.sh"
fi

# ---------------------------------------------------------------------------
# Done
# ---------------------------------------------------------------------------
echo ""
echo "==> Done: dist/MowisAI.dmg"
