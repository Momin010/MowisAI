#!/usr/bin/env bash
# Autonomous boot-time runner for the GCE test instance.
#
# Triggered via the instance's startup-script metadata. On every boot (or
# `gcloud compute instances reset`), this:
#   1. Disables UFW + iptables (the previous test loop kept silently dropping
#      IAP traffic; eliminate the entire firewall as a variable).
#   2. Installs build deps + Rust if missing (idempotent — apt skips installed
#      packages, rustup detects an existing toolchain).
#   3. Clones or fast-forwards /opt/mowisai on the refactor branch.
#   4. Runs scripts/full-test.sh end-to-end.
#   5. Uploads the full log to GCS so Claude can read it without SSH.
#
# Required at instance creation:
#   --scopes=cloud-platform                (so gsutil from the VM can write to GCS)
#   --enable-nested-virtualization         (so KVM works inside the guest VM)
#   --metadata=startup-script=<bootstrap>  (a 2-liner that curls this script)

set +e
set -o pipefail
export DEBIAN_FRONTEND=noninteractive

REPO_URL="https://github.com/momin010/mowisai.git"
BRANCH="claude/refactor-agentd-mowis-zR6sh"
REPO_DIR="/opt/mowisai"
BUCKET="mowis-test-relay-490516"

STAMP="$(date -u +%Y%m%d-%H%M%S)"
LOG="/var/log/mowis-run-${STAMP}.log"
exec > "$LOG" 2>&1
echo "=== MOWIS INSTANCE RUNNER ${STAMP} ==="
date -u
uname -a

echo "=== killing leftover qemu / mowis-executor from previous run ==="
pkill -9 -f mowis-executor 2>/dev/null
pkill -9 -f qemu-system   2>/dev/null
pkill -9 -f 'target/release/mowisd' 2>/dev/null
sleep 1

echo "=== disabling host firewall (UFW + iptables) ==="
ufw disable 2>/dev/null || true
systemctl disable --now ufw 2>/dev/null || true
iptables -P INPUT ACCEPT 2>/dev/null || true
iptables -P FORWARD ACCEPT 2>/dev/null || true
iptables -P OUTPUT ACCEPT 2>/dev/null || true
iptables -F 2>/dev/null || true

echo "=== installing deps ==="
if ! command -v cargo >/dev/null 2>&1 || ! command -v qemu-system-x86_64 >/dev/null 2>&1 || ! command -v zstd >/dev/null 2>&1; then
    apt-get update -qq
    apt-get install -y -qq \
        qemu-system-x86 qemu-kvm qemu-utils ovmf \
        skopeo cpio tar wget curl git \
        build-essential pkg-config libssl-dev \
        iproute2 zstd xz-utils
fi

if ! command -v cargo >/dev/null 2>&1; then
    echo "=== installing rust (as root) ==="
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs |
        sh -s -- -y --default-toolchain stable --profile minimal --no-modify-path
fi
# Ensure cargo on PATH for the rest of this script and the test script
export PATH="/root/.cargo/bin:$PATH"
source /root/.cargo/env 2>/dev/null || true

echo "=== loading vsock modules ==="
modprobe vhost_vsock 2>&1
modprobe vsock_loopback 2>&1
lsmod | grep -E 'vsock|vhost'

echo "=== cloning / updating repo ==="
if [[ ! -d "$REPO_DIR/.git" ]]; then
    git clone "$REPO_URL" "$REPO_DIR"
fi
cd "$REPO_DIR"
git fetch origin
git checkout "$BRANCH"
git reset --hard "origin/$BRANCH"
git log --oneline -3

echo "=== running full-test.sh ==="
export REPO_ROOT="$REPO_DIR"
bash scripts/full-test.sh
TEST_EXIT=$?
echo "=== full-test.sh exit: $TEST_EXIT ==="

echo "=== uploading log to GCS ==="
# gsutil ships with gcloud SDK; default GCE service account with cloud-platform
# scope has perms thanks to the project's IAM bindings.
gsutil cp "$LOG" "gs://$BUCKET/run-${STAMP}.log" || echo "gsutil run upload FAILED"
gsutil cp "$LOG" "gs://$BUCKET/run-latest.log"  || echo "gsutil latest upload FAILED"

echo "=== DONE ${STAMP} (test_exit=$TEST_EXIT) ==="
