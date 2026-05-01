#!/usr/bin/env bash
set -euo pipefail

# Ensure we are in the right directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

DEST_FINAL="alpine-minirootfs-x86_64.tar.gz"

if [[ -f "$DEST_FINAL" ]]; then
    echo "Alpine rootfs already present, skipping download."
    exit 0
fi

echo "Fetching latest version metadata..."

# Get the version using a simpler pattern that works better in Git Bash
LATEST_VER=$(curl -sL https://alpinelinux.org | grep "version:" | head -n 1 | sed 's/[^0-9.]//g')

if [[ -z "$LATEST_VER" ]]; then
    echo "Error: Could not determine latest Alpine version. Check internet connection or URL."
    exit 1
fi

FILENAME="alpine-minirootfs-${LATEST_VER}-x86_64.tar.gz"
ALPINE_URL="https://alpinelinux.org{FILENAME}"

echo "Downloading Alpine ${LATEST_VER} mini-rootfs from ${ALPINE_URL}"
curl -fL --progress-bar -o "$DEST_FINAL" "$ALPINE_URL"

echo "Done: Saved as $DEST_FINAL"
