#!/usr/bin/env bash
set -euo pipefail

# Move to the script's directory
cd "$(dirname "${BASH_SOURCE[0]}")"

# Target name required by your tauri.conf.json
DEST="alpine-minirootfs-x86_64.tar.gz"

if [[ -f "$DEST" ]]; then
    echo "File already exists, skipping."
    exit 0
fi

echo "Downloading Alpine 3.23.4..."

# Hardcoded direct link to prevent Windows variable errors
curl -fL --progress-bar -o "$DEST" "https://dl-cdn.alpinelinux.org/alpine/latest-stable/releases/x86_64/alpine-minirootfs-3.23.4-x86_64.tar.gz"

echo "Done!"
