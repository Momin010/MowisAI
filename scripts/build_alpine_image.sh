#!/usr/bin/env bash
# build_alpine_image.sh — Build an Alpine Linux disk image containing the
# static agentd binary and a socat TCP bridge.
#
# Outputs (written to dist/):
#   agentd-alpine.qcow2   — QEMU disk image for macOS/Windows QEMU VMs
#   agentd-alpine.tar     — Filesystem tar for WSL2 import on Windows
#
# Prerequisites:
#   - Docker daemon running
#   - qemu-img in PATH  (macOS: brew install qemu)
#                        (Ubuntu: sudo apt-get install qemu-utils)
#   - The static agentd binary must already be built by build_musl_agentd.sh
#
# Usage:
#   ./scripts/build_alpine_image.sh [--skip-build]
#
# Flags:
#   --skip-build   Skip running build_musl_agentd.sh (use existing binary).

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SCRIPTS_DIR="$REPO_ROOT/scripts"
DIST_DIR="$REPO_ROOT/dist"
TARGET="x86_64-unknown-linux-musl"
AGENTD_BIN="$REPO_ROOT/target/$TARGET/release/agentd"
DOCKER_IMAGE="mowisai-agentd"
QCOW2_OUT="$DIST_DIR/agentd-alpine.qcow2"
TAR_OUT="$DIST_DIR/agentd-alpine.tar"
RAW_IMG="$DIST_DIR/agentd-alpine.raw"
MOUNT_DIR="$DIST_DIR/.mnt"
# Size of the raw ext4 image in MiB (adjust upward if agentd binary grows).
IMG_SIZE_MIB=128

SKIP_BUILD=0
for arg in "$@"; do
    case "$arg" in
        --skip-build) SKIP_BUILD=1 ;;
        *) printf 'Unknown flag: %s\n' "$arg" >&2; exit 1 ;;
    esac
done

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
info()    { printf '\033[1;34m[alpine-image]\033[0m %s\n' "$*"; }
success() { printf '\033[1;32m[alpine-image]\033[0m %s\n' "$*"; }
error()   { printf '\033[1;31m[alpine-image] ERROR:\033[0m %s\n' "$*" >&2; }
die()     { error "$*"; exit 1; }

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        echo "(sha256sum not available)"
    fi
}

cleanup() {
    # Unmount if still mounted (Linux only, best-effort).
    if [[ -d "$MOUNT_DIR" ]]; then
        if mount | grep -q "$MOUNT_DIR" 2>/dev/null; then
            sudo umount "$MOUNT_DIR" 2>/dev/null || true
        fi
        rmdir "$MOUNT_DIR" 2>/dev/null || true
    fi
    # Remove intermediate raw image.
    rm -f "$RAW_IMG"
}
trap cleanup EXIT

# ---------------------------------------------------------------------------
# Preflight: required tools
# ---------------------------------------------------------------------------
info "Checking required tools..."

if ! command -v docker >/dev/null 2>&1; then
    die "docker not found.  Install Docker Desktop (macOS/Windows) or Docker Engine (Linux)."
fi

if ! docker info >/dev/null 2>&1; then
    die "Docker daemon is not running.  Start Docker and retry."
fi

if ! command -v qemu-img >/dev/null 2>&1; then
    cat >&2 <<'MSG'
[alpine-image] ERROR: qemu-img not found.
  macOS : brew install qemu
  Ubuntu: sudo apt-get install qemu-utils
  Windows (WSL2): sudo apt-get install qemu-utils
MSG
    exit 1
fi

info "docker  : $(docker --version)"
info "qemu-img: $(qemu-img --version | head -1)"

# ---------------------------------------------------------------------------
# Step 1: Build static binary (unless --skip-build)
# ---------------------------------------------------------------------------
if [[ "$SKIP_BUILD" -eq 0 ]]; then
    info "Building static agentd binary via build_musl_agentd.sh..."
    bash "$SCRIPTS_DIR/build_musl_agentd.sh"
else
    info "--skip-build set; skipping musl build."
fi

if [[ ! -f "$AGENTD_BIN" ]]; then
    die "Static binary not found at $AGENTD_BIN.  Run build_musl_agentd.sh first."
fi

info "Using binary: $AGENTD_BIN ($(du -sh "$AGENTD_BIN" | cut -f1))"

# ---------------------------------------------------------------------------
# Step 2: Prepare dist/ directory
# ---------------------------------------------------------------------------
mkdir -p "$DIST_DIR"

# ---------------------------------------------------------------------------
# Step 3: Build Docker image (Alpine + socat + agentd)
# ---------------------------------------------------------------------------
info "Building Docker image '$DOCKER_IMAGE'..."

docker build \
    --platform linux/amd64 \
    --tag "$DOCKER_IMAGE:latest" \
    --file - \
    "$REPO_ROOT" \
    <<'DOCKERFILE'
FROM alpine:3.19

# Install runtime dependencies.
#   socat     — TCP→Unix socket bridge (exposes agentd.sock over TCP 9722)
#   bash      — used by start-agentd.sh and any interactive debugging
#   ca-certificates — required for TLS connections to Vertex AI / GCP APIs
RUN apk add --no-cache \
        socat \
        bash \
        ca-certificates \
    && rm -rf /var/cache/apk/*

# Copy the pre-built static agentd binary.
# The binary is fully static (musl) so no glibc is needed inside Alpine.
COPY target/x86_64-unknown-linux-musl/release/agentd /usr/local/bin/agentd
RUN chmod +x /usr/local/bin/agentd

# Startup script:
#   1. Launch agentd listening on a Unix socket.
#   2. Wait briefly for the socket to appear.
#   3. Forward TCP port 9722 to the Unix socket via socat.
#      Clients (macOS/Windows) connect on port 9722 and are transparently
#      proxied to /tmp/agentd.sock inside the VM.
RUN cat > /usr/local/bin/start-agentd.sh <<'SCRIPT'
#!/bin/sh
set -e

# Start agentd in the background.
/usr/local/bin/agentd socket --path /tmp/agentd.sock &
AGENTD_PID=$!

# Wait for the Unix socket to become available (max 10 s).
RETRIES=20
while [ $RETRIES -gt 0 ]; do
    [ -S /tmp/agentd.sock ] && break
    sleep 0.5
    RETRIES=$(( RETRIES - 1 ))
done
if [ ! -S /tmp/agentd.sock ]; then
    echo "ERROR: agentd socket did not appear at /tmp/agentd.sock" >&2
    exit 1
fi

echo "agentd running (pid $AGENTD_PID), bridging TCP:9722 -> /tmp/agentd.sock"

# Bridge: any TCP connection on 9722 is forwarded to the Unix socket.
# fork    — handle multiple concurrent connections
# reuseaddr — allow fast restart without TIME_WAIT delay
exec socat TCP-LISTEN:9722,fork,reuseaddr UNIX-CLIENT:/tmp/agentd.sock
SCRIPT
RUN chmod +x /usr/local/bin/start-agentd.sh

EXPOSE 9722

ENTRYPOINT ["/usr/local/bin/start-agentd.sh"]
DOCKERFILE

success "Docker image '$DOCKER_IMAGE:latest' built."

# ---------------------------------------------------------------------------
# Step 4: Export as filesystem tar (WSL2 target)
# ---------------------------------------------------------------------------
info "Exporting Docker image to $TAR_OUT (WSL2 format)..."

CONTAINER_ID=$(docker create --platform linux/amd64 "$DOCKER_IMAGE:latest")
docker export "$CONTAINER_ID" > "$TAR_OUT"
docker rm "$CONTAINER_ID" >/dev/null

success "WSL2 tar written: $TAR_OUT ($(du -sh "$TAR_OUT" | cut -f1))"

# ---------------------------------------------------------------------------
# Step 5: Convert to qcow2 (QEMU target)
#
# Strategy:
#   a) Create a blank raw ext4 disk image.
#   b) Mount it (Linux only — requires loop device + root).
#      On macOS we fall back to a simpler approach: convert the tar directly
#      to a raw image via the 'hdiutil' path is not available for ext4, so
#      instead we wrap the tar inside a qcow2 using "qemu-img" with a raw
#      image that carries the tar content as the root FS.
#   c) Convert raw → qcow2 with qemu-img.
#
# NOTE: Creating a proper bootable ext4 image requires root (loop mounts).
# If root is not available, we produce a "data qcow2" that holds the rootfs
# tar stream — suitable for use with 9p/virtio-fs in QEMU rather than as a
# boot disk.  Either way the file is a valid qcow2.
# ---------------------------------------------------------------------------
info "Converting to qcow2 for QEMU..."

UNAME_S="$(uname -s)"

if [[ "$UNAME_S" == "Linux" ]] && [[ "$(id -u)" -eq 0 ]]; then
    # --- Full ext4 path (Linux root) ---
    info "Running as root on Linux — creating ext4 image and mounting."

    dd if=/dev/zero of="$RAW_IMG" bs=1M count="$IMG_SIZE_MIB" status=none
    mkfs.ext4 -q -L agentd-root "$RAW_IMG"

    mkdir -p "$MOUNT_DIR"
    mount -o loop "$RAW_IMG" "$MOUNT_DIR"

    info "Extracting filesystem into raw image..."
    tar -xf "$TAR_OUT" -C "$MOUNT_DIR" --numeric-owner 2>/dev/null || true

    umount "$MOUNT_DIR"
    rmdir  "$MOUNT_DIR"

    qemu-img convert -f raw -O qcow2 "$RAW_IMG" "$QCOW2_OUT"
    rm -f "$RAW_IMG"

    success "ext4 qcow2 image written: $QCOW2_OUT"

else
    # --- Fallback: raw image carrying the tar stream ---
    # This path runs on macOS or when not root on Linux.
    # The resulting qcow2 wraps the tar as a raw data blob.
    # To use it as a proper VM root FS you should unpack it on the Linux host
    # that will run the VM, or use a cloud-init/9p approach.
    info "Not root / not Linux — using fallback: wrapping tar in qcow2."
    info "(For a proper bootable ext4 image, re-run as root on Linux.)"

    # qemu-img can convert a raw file (our tar) to qcow2 directly.
    # The resulting disk image is not ext4-formatted but contains the tar
    # data and can be accessed via qemu-nbd + mount on the target host.
    qemu-img convert -f raw -O qcow2 "$TAR_OUT" "$QCOW2_OUT"

    success "Data qcow2 written (tar-wrapped fallback): $QCOW2_OUT"
    cat <<'NOTE'

NOTE: The qcow2 was built in fallback mode (raw tar wrapped in qcow2).
To get a fully-bootable ext4 qcow2, run this script as root on a Linux host:
    sudo bash scripts/build_alpine_image.sh --skip-build

NOTE
fi

# ---------------------------------------------------------------------------
# Step 6: Print checksums + sizes
# ---------------------------------------------------------------------------
echo ""
success "Build complete.  Output files:"
echo ""
printf '  %-36s  %s  SHA256: %s\n' \
    "$QCOW2_OUT" \
    "$(du -sh "$QCOW2_OUT" | cut -f1)" \
    "$(sha256_of "$QCOW2_OUT")"
printf '  %-36s  %s  SHA256: %s\n' \
    "$TAR_OUT" \
    "$(du -sh "$TAR_OUT" | cut -f1)" \
    "$(sha256_of "$TAR_OUT")"
echo ""
info "To import into WSL2 (Windows):"
echo "  wsl --import agentd C:\\wsl\\agentd dist\\agentd-alpine.tar"
echo ""
info "To run in QEMU (macOS/Linux):"
echo "  qemu-system-x86_64 -m 512 -drive file=dist/agentd-alpine.qcow2,format=qcow2 -net user,hostfwd=tcp::9722-:9722"
echo ""
info "Next step: run scripts/release_assets.sh <tag> to upload to a GitHub release."
