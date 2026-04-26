#!/usr/bin/env bash
# build_musl_agentd.sh — Compile agentd as a fully static binary using musl libc.
#
# Why musl?  The Alpine Linux image contains no glibc.  A musl-linked binary is
# entirely self-contained and runs inside Alpine (or any bare Linux kernel)
# without any shared library dependencies.
#
# Prerequisites (Ubuntu/Debian host):
#   sudo apt-get install musl-tools
#   rustup target add x86_64-unknown-linux-musl
#
# macOS host:
#   brew install FiloSottile/musl-cross/musl-cross
#   export CC_x86_64_unknown_linux_musl=x86_64-linux-musl-gcc
#   export CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER=x86_64-linux-musl-gcc
#   rustup target add x86_64-unknown-linux-musl
#
# Windows host:
#   Use WSL2 and follow the Ubuntu instructions above.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET="x86_64-unknown-linux-musl"
OUTPUT="$REPO_ROOT/target/$TARGET/release/agentd"

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
info()  { printf '\033[1;34m[build_musl]\033[0m %s\n' "$*"; }
error() { printf '\033[1;31m[build_musl] ERROR:\033[0m %s\n' "$*" >&2; }
die()   { error "$*"; exit 1; }

# ---------------------------------------------------------------------------
# Preflight checks
# ---------------------------------------------------------------------------
info "Checking for rustup..."
command -v rustup >/dev/null 2>&1 || die "rustup not found. Install from https://rustup.rs/"

info "Checking for cargo..."
command -v cargo >/dev/null 2>&1 || die "cargo not found. Install from https://rustup.rs/"

# Ensure the musl target is installed (idempotent).
info "Ensuring musl target '$TARGET' is installed..."
rustup target add "$TARGET"

# On Linux we need the musl-gcc wrapper from musl-tools.  Give a helpful hint
# if it is absent rather than letting the linker step fail silently.
if [[ "$(uname -s)" == "Linux" ]]; then
    if ! command -v musl-gcc >/dev/null 2>&1; then
        die "musl-gcc not found.  Install it with: sudo apt-get install musl-tools"
    fi
    info "musl-gcc found: $(command -v musl-gcc)"
fi

# ---------------------------------------------------------------------------
# Build
# ---------------------------------------------------------------------------
info "Building static agentd binary (release, $TARGET)..."
info "Working directory: $REPO_ROOT"

cd "$REPO_ROOT"

# RUSTFLAGS='-C target-feature=+crt-static' is redundant for the musl target
# (musl links statically by default) but makes the intent explicit and ensures
# any C dependencies pulled in via build scripts are also linked statically.
RUSTFLAGS='-C target-feature=+crt-static' \
    cargo build \
        --release \
        --target "$TARGET" \
        -p agentd

# ---------------------------------------------------------------------------
# Verify & report
# ---------------------------------------------------------------------------
if [[ ! -f "$OUTPUT" ]]; then
    die "Expected output binary not found at: $OUTPUT"
fi

BINARY_SIZE=$(du -sh "$OUTPUT" | cut -f1)
info "Build succeeded."
info "Static binary : $OUTPUT"
info "Binary size   : $BINARY_SIZE"

# Confirm the binary has no dynamic library dependencies (ldd should say
# "not a dynamic executable" or similar).
if command -v ldd >/dev/null 2>&1; then
    info "Checking dynamic dependencies (should be empty)..."
    ldd "$OUTPUT" 2>&1 || true   # ldd exits non-zero for static binaries — that is fine
fi

# SHA256 for integrity verification before copying into the Docker image.
if command -v sha256sum >/dev/null 2>&1; then
    SHA=$(sha256sum "$OUTPUT" | awk '{print $1}')
elif command -v shasum >/dev/null 2>&1; then
    SHA=$(shasum -a 256 "$OUTPUT" | awk '{print $1}')
else
    SHA="(sha256sum not available)"
fi
info "SHA256        : $SHA"

echo ""
echo "Next step: run scripts/build_alpine_image.sh to embed this binary in the QEMU/WSL2 image."
