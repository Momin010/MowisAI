#!/bin/bash

# MowisAI Container Entry Script
# This script "enters" the container like WSL with a green prompt

set -e

ROOTFS_DIR="./rootfs"
ENGINE_DIR="$(dirname "$0")"

# Colors
GREEN='\033[32m'
BLUE='\033[34m'
CYAN='\033[36m'
RESET='\033[0m'

echo -e "${GREEN}➜  MowisAI Container Shell${RESET}"
echo -e "${CYAN}   Starting interactive session...${RESET}"
echo ""

# Check if rootfs exists
if [ ! -d "$ROOTFS_DIR" ]; then
    echo -e "${GREEN}➜  Rootfs not found. Setting up Alpine Linux...${RESET}"
    bash "$ENGINE_DIR/setup_rootfs.sh"
fi

# Build the engine if needed
if [ ! -f "$ENGINE_DIR/target/debug/mowisai-engine" ] && [ ! -f "$ENGINE_DIR/target/release/mowisai-engine" ]; then
    echo -e "${GREEN}➜  Building MowisAI Engine...${RESET}"
    cd "$ENGINE_DIR" && cargo build
fi

# Run the interactive shell
cd "$ENGINE_DIR"
exec cargo run --bin mowisai-shell 2>/dev/null || cargo run -- --interactive
