#!/usr/bin/env bash
# Download build-time resources that are too large to commit to git.
# Run this script once before `tauri build` on a CI/CD machine or dev box.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEST="${SCRIPT_DIR}/alpine-minirootfs-x86_64.tar.gz"

if [[ -f "$DEST" ]]; then
    echo "alpine-minirootfs-x86_64.tar.gz already present, skipping download."
    exit 0
fi

# Use latest-stable to avoid hardcoding version numbers that eventually 404
# This automatically gets the current stable release (3.23.4 as of May 2026)
ALPINE_URL="https://dl-cdn.alpinelinux.org/alpine/latest-stable/releases/x86_64/alpine-minirootfs-x86_64.tar.gz"

echo "Downloading Alpine latest-stable mini-rootfs (~3.5 MB)..."
curl -fL --progress-bar -o "$DEST" "$ALPINE_URL"
echo "Done: $DEST"
