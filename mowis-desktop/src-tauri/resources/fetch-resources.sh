#!/usr/bin/env bash
# Download build-time resources that are too large to commit to git.
# Run this script once before `tauri build` on a CI/CD machine or dev box.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

ALPINE_VERSION="3.21.3"
ALPINE_ROOTFS="alpine-minirootfs-${ALPINE_VERSION}-x86_64.tar.gz"
ALPINE_URL="https://dl-cdn.alpinelinux.org/alpine/v${ALPINE_VERSION%%.*}.${ALPINE_VERSION#*.}/releases/x86_64/alpine-minirootfs-${ALPINE_VERSION}-x86_64.tar.gz"
DEST="${SCRIPT_DIR}/alpine-minirootfs-x86_64.tar.gz"

if [[ -f "$DEST" ]]; then
    echo "alpine-minirootfs-x86_64.tar.gz already present, skipping download."
    exit 0
fi

echo "Downloading Alpine ${ALPINE_VERSION} mini-rootfs (~3.5 MB)..."
curl -fL --progress-bar -o "$DEST" "$ALPINE_URL"
echo "Done: $DEST"
