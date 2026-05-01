#!/usr/bin/env bash
set -euo pipefail

# Move to the script's directory so paths are consistent
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Target name required by your tauri.conf.json
DEST="alpine-minirootfs-x86_64.tar.gz"

if [[ -f "$DEST" ]]; then
    echo "alpine-minirootfs-x86_64.tar.gz already present, skipping download."
    exit 0
fi

# Hardcoding the current newest version to bypass parsing errors
# As of May 1, 2026, the newest stable version is 3.23.4
ALPINE_VER="3.23.4"
ALPINE_URL="https://alpinelinux.org{ALPINE_VER}-x86_64.tar.gz"

echo "Downloading Alpine ${ALPINE_VER} mini-rootfs..."
curl -fL --progress-bar -o "$DEST" "$ALPINE_URL"

echo "Done: Saved to $(pwd)/$DEST"
