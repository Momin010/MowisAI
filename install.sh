#!/bin/bash
# convenience installer script for MowisAI workspace
set -euo pipefail

echo "==> installing system prerequisites (curl tar skopeo build tools)"
# apt might complain about unsigned repos; ignore errors so script continues
sudo apt update || true
sudo apt install -y curl tar skopeo build-essential pkg-config libssl-dev || true

echo "==> building Rust engine"
cd mowisai-engine
cargo build --release
cd ..

echo "==> installing bridge npm dependencies"
cd mowisai-bridge
npm install
cd ..

echo "==> done. You can now run './start.sh' to set up the rootfs and start the engine."