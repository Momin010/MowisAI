#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEST_FINAL="${SCRIPT_DIR}/alpine-minirootfs-x86_64.tar.gz"

if [[ -f "$DEST_FINAL" ]]; then
    echo "Alpine rootfs already present, skipping download."
    exit 0
fi

echo "Fetching latest version metadata..."
# This finds the exact current version string (e.g., 3.23.4)
LATEST_VER=$(curl -s https://alpinelinux.org | \
             grep -m 1 "version:" | awk '{print $2}')

if [[ -z "$LATEST_VER" ]]; then
    echo "Error: Could not determine latest Alpine version."
    exit 1
fi

FILENAME="alpine-minirootfs-${LATEST_VER}-x86_64.tar.gz"
ALPINE_URL="https://alpinelinux.org{FILENAME}"

echo "Downloading Alpine ${LATEST_VER} mini-rootfs..."
# Download to a temporary file first, then rename to your generic DEST name
curl -fL --progress-bar -o "$DEST_FINAL" "$ALPINE_URL"

echo "Done: Saved as $DEST_FINAL"
