#!/usr/bin/env bash
set -euo pipefail

# Build static agentd binaries for Alpine Linux VMs
# Produces x86_64 and aarch64 musl-linked binaries with no dynamic dependencies

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
OUTPUT_DIR="$PROJECT_ROOT/target/static"

echo "==> Building static agentd binaries"
echo "    Project root: $PROJECT_ROOT"
echo "    Output dir: $OUTPUT_DIR"

mkdir -p "$OUTPUT_DIR"

# Install musl targets if not already installed
echo "==> Installing musl targets"
rustup target add x86_64-unknown-linux-musl
rustup target add aarch64-unknown-linux-musl

# Build x86_64 static binary
echo "==> Building x86_64-unknown-linux-musl"
cd "$PROJECT_ROOT/agentd"
RUSTFLAGS="-C target-feature=+crt-static" \
    cargo build \
    --release \
    --target x86_64-unknown-linux-musl \
    --bin agentd

cp "$PROJECT_ROOT/target/x86_64-unknown-linux-musl/release/agentd" \
   "$OUTPUT_DIR/agentd-x86_64"

echo "==> Verifying x86_64 binary has no dynamic dependencies"
if command -v ldd &> /dev/null; then
    if ldd "$OUTPUT_DIR/agentd-x86_64" 2>&1 | grep -q "not a dynamic executable"; then
        echo "    ✓ x86_64 binary is statically linked"
    else
        echo "    ✗ x86_64 binary has dynamic dependencies:"
        ldd "$OUTPUT_DIR/agentd-x86_64" || true
        exit 1
    fi
else
    echo "    ⚠ ldd not available, skipping dynamic dependency check"
fi

# Build aarch64 static binary (for Apple Silicon)
echo "==> Building aarch64-unknown-linux-musl"
RUSTFLAGS="-C target-feature=+crt-static" \
    cargo build \
    --release \
    --target aarch64-unknown-linux-musl \
    --bin agentd

cp "$PROJECT_ROOT/target/aarch64-unknown-linux-musl/release/agentd" \
   "$OUTPUT_DIR/agentd-aarch64"

echo "==> Verifying aarch64 binary has no dynamic dependencies"
if command -v ldd &> /dev/null; then
    if ldd "$OUTPUT_DIR/agentd-aarch64" 2>&1 | grep -q "not a dynamic executable"; then
        echo "    ✓ aarch64 binary is statically linked"
    else
        echo "    ✗ aarch64 binary has dynamic dependencies:"
        ldd "$OUTPUT_DIR/agentd-aarch64" || true
        exit 1
    fi
else
    echo "    ⚠ ldd not available, skipping dynamic dependency check"
fi

# Display binary sizes
echo "==> Binary sizes:"
ls -lh "$OUTPUT_DIR"/agentd-*

echo "==> Static binaries built successfully"
echo "    x86_64: $OUTPUT_DIR/agentd-x86_64"
echo "    aarch64: $OUTPUT_DIR/agentd-aarch64"
