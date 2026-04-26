#!/usr/bin/env bash
# release_assets.sh — Upload MowisAI disk image assets to a GitHub release.
#
# Usage:
#   ./scripts/release_assets.sh [<version-tag>]
#
# Arguments:
#   version-tag   The release tag to upload to (e.g. v0.1.0).
#                 If omitted, the most recent git tag is used.
#
# Prerequisites:
#   - gh (GitHub CLI) installed and authenticated: gh auth login
#   - dist/agentd-alpine.qcow2 and dist/agentd-alpine.tar must exist.
#     Build them first with: scripts/build_alpine_image.sh
#
# The --clobber flag allows re-uploading assets to an existing release
# (e.g. when iterating on a pre-release) without failing.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_DIR="$REPO_ROOT/dist"
QCOW2_ASSET="$DIST_DIR/agentd-alpine.qcow2"
TAR_ASSET="$DIST_DIR/agentd-alpine.tar"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
info()    { printf '\033[1;34m[release]\033[0m %s\n' "$*"; }
success() { printf '\033[1;32m[release]\033[0m %s\n' "$*"; }
error()   { printf '\033[1;31m[release] ERROR:\033[0m %s\n' "$*" >&2; }
die()     { error "$*"; exit 1; }

# ---------------------------------------------------------------------------
# Resolve version tag
# ---------------------------------------------------------------------------
if [[ $# -ge 1 && -n "${1:-}" ]]; then
    VERSION="$1"
else
    info "No version tag supplied — detecting from git tags..."
    if ! VERSION=$(git -C "$REPO_ROOT" describe --tags --abbrev=0 2>/dev/null); then
        die "No git tags found and no version argument provided.  Usage: $0 <version-tag>"
    fi
fi

info "Target release tag: $VERSION"

# ---------------------------------------------------------------------------
# Preflight checks
# ---------------------------------------------------------------------------
info "Checking prerequisites..."

if ! command -v gh >/dev/null 2>&1; then
    cat >&2 <<'MSG'
[release] ERROR: gh (GitHub CLI) not found.
  macOS : brew install gh
  Ubuntu: sudo apt-get install gh  (or https://cli.github.com)
  Windows: winget install GitHub.cli
MSG
    exit 1
fi

# Confirm the user is authenticated.
if ! gh auth status >/dev/null 2>&1; then
    die "Not authenticated with GitHub CLI.  Run: gh auth login"
fi

info "gh version: $(gh --version | head -1)"

# Confirm both asset files exist.
if [[ ! -f "$QCOW2_ASSET" ]]; then
    die "qcow2 asset not found: $QCOW2_ASSET\n       Run scripts/build_alpine_image.sh first."
fi

if [[ ! -f "$TAR_ASSET" ]]; then
    die "tar asset not found: $TAR_ASSET\n       Run scripts/build_alpine_image.sh first."
fi

# ---------------------------------------------------------------------------
# Verify the release exists on GitHub (do not create it automatically — that
# is the developer's responsibility so release notes are written by a human).
# ---------------------------------------------------------------------------
info "Verifying release '$VERSION' exists on GitHub..."
if ! gh release view "$VERSION" >/dev/null 2>&1; then
    cat >&2 <<MSG
[release] ERROR: GitHub release '$VERSION' does not exist.
  Create it first (with release notes) via the GitHub UI or:
    gh release create "$VERSION" --title "$VERSION" --notes "Release $VERSION"
MSG
    exit 1
fi

# ---------------------------------------------------------------------------
# Compute checksums for display
# ---------------------------------------------------------------------------
sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        echo "(sha256sum not available)"
    fi
}

QCOW2_SIZE="$(du -sh "$QCOW2_ASSET" | cut -f1)"
TAR_SIZE="$(du -sh "$TAR_ASSET" | cut -f1)"
QCOW2_SHA="$(sha256_of "$QCOW2_ASSET")"
TAR_SHA="$(sha256_of "$TAR_ASSET")"

info "Assets to upload:"
printf '  %-36s  %s  SHA256: %s\n' "agentd-alpine.qcow2" "$QCOW2_SIZE" "$QCOW2_SHA"
printf '  %-36s  %s  SHA256: %s\n' "agentd-alpine.tar"   "$TAR_SIZE"   "$TAR_SHA"

# ---------------------------------------------------------------------------
# Upload
# ---------------------------------------------------------------------------
info "Uploading assets to release '$VERSION'..."

gh release upload "$VERSION" \
    "$QCOW2_ASSET" \
    "$TAR_ASSET" \
    --clobber

# ---------------------------------------------------------------------------
# Done
# ---------------------------------------------------------------------------
echo ""
success "Assets uploaded to release $VERSION."
echo ""
info "Release URL:"
gh release view "$VERSION" --json url --jq '.url'
echo ""
info "Checksums for release notes / SBOM:"
printf '  SHA256 (agentd-alpine.qcow2): %s\n' "$QCOW2_SHA"
printf '  SHA256 (agentd-alpine.tar)  : %s\n' "$TAR_SHA"
