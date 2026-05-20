#!/usr/bin/env bash
# Full end-to-end smoke test of the new mowis-host / mowis-executor architecture.
#
# Phases:
#   A. Environment diagnostics (kernel, KVM, vsock modules, tools)
#   B. Build everything in release mode
#   C. Unit tests on the new crates
#   D. Loopback protocol test: host process talks to host process over
#      AF_VSOCK via the vsock_loopback module — proves the wire protocol and
#      transport without booting a VM.
#   E. Initrd build + real VM boot via QEMU/KVM + ping/exec across vsock.
#
# All output goes to stdout. Pipe to gsutil to ship the log to the relay
# bucket so Claude can read it from the constrained sandbox.

set +e  # never bail; we want to see every failure in the log
set -o pipefail
shopt -s expand_aliases
export DEBIAN_FRONTEND=noninteractive
export RUST_LOG=info
export PATH="$HOME/.cargo/bin:/root/.cargo/bin:$PATH"
source "$HOME/.cargo/env" 2>/dev/null || true
source "/root/.cargo/env" 2>/dev/null || true

# Find repo root: prefer $REPO_ROOT env, else script's parent dir.
REPO_ROOT="${REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"

step() { printf "\n\n========== %s ==========\n" "$*"; }
sub()  { printf "\n--- %s ---\n" "$*"; }
run()  { printf "\n$ %s\n" "$*"; eval "$@"; printf "(exit=%d)\n" $?; }

# Make sure we don't leak qemu processes between runs.
cleanup() {
    sub "cleanup"
    sudo pkill -f mowis-executor 2>/dev/null || true
    sudo pkill -f qemu-system 2>/dev/null || true
    sudo pkill -f 'target/release/mowisd' 2>/dev/null || true
}
trap cleanup EXIT

step "A. ENVIRONMENT"
run "date -u"
run "uname -a"
run "lsb_release -a 2>/dev/null || cat /etc/os-release"
run "nproc"
run "free -h | head -3"
run "df -h / | head -3"

sub "virtualization"
run "ls -la /dev/kvm"
run "grep -E '^flags' /proc/cpuinfo | head -1 | tr ' ' '\\n' | grep -E '^(vmx|svm|nested)$'"
run "kvm-ok 2>&1 || true"

sub "load vsock modules"
run "sudo modprobe vhost_vsock"
run "sudo modprobe vsock_loopback"
run "lsmod | grep -E 'vsock|vhost'"
run "ls -la /dev/vhost-vsock /dev/vsock 2>&1"

sub "tools on PATH"
for t in qemu-system-x86_64 skopeo cpio gzip tar cargo rustc git; do
    if command -v "$t" >/dev/null 2>&1; then
        printf "  %-22s %s\n" "$t" "$(command -v "$t")"
    else
        printf "  %-22s MISSING\n" "$t"
    fi
done

step "B. PULL + BUILD"
cd "$REPO_ROOT" || { echo "REPO MISSING at $REPO_ROOT"; exit 1; }
echo "REPO_ROOT=$REPO_ROOT"
run "git status --porcelain"
run "git fetch origin"
run "git checkout claude/refactor-agentd-mowis-zR6sh"
run "git pull origin claude/refactor-agentd-mowis-zR6sh"
run "git log --oneline -5"

sub "release build"
run "cargo build --release -p mowis-protocol -p mowis-host -p mowis-executor 2>&1 | tail -20"
run "ls -la target/release/mowisd target/release/mowis-executor"

step "C. UNIT TESTS"
run "cargo test --release -p mowis-protocol 2>&1 | tail -10"

step "D. LOOPBACK PROTOCOL TEST (no VM)"
# vsock_loopback exposes CID=1 (VMADDR_CID_LOCAL); a server bound to
# VMADDR_CID_ANY accepts loopback connections from CID=1 on the same host.

sub "start executor in background"
LOG=/tmp/exec-loopback.log
sudo RUST_LOG=info target/release/mowis-executor --port 5252 > "$LOG" 2>&1 &
EXEC_PID=$!
sleep 2
run "sudo ss -lnp 2>/dev/null | grep -E 'vsock|5252' || true"
run "tail -20 $LOG"

sub "ping over vsock loopback"
run "timeout 10 target/release/mowisd ping --cid 1 --port 5252 2>&1"

sub "exec /bin/echo over vsock loopback (no sandbox — host's binaries are visible)"
run "timeout 20 target/release/mowisd exec --cid 1 --port 5252 --no-sandbox -- /bin/echo hello-from-vsock 2>&1"

sub "create sandbox over vsock loopback (sandboxed path; expected non-zero — tmpfs has no /bin/true)"
run "timeout 10 target/release/mowisd exec --cid 1 --port 5252 -- /bin/true 2>&1 || true"

sub "executor log after exec"
run "tail -30 $LOG"

run "sudo kill $EXEC_PID 2>/dev/null"
sleep 1

step "E. INITRD BUILD + REAL VM BOOT"

sub "build initrd"
run "target/release/mowisd build-initrd --executor target/release/mowis-executor --output /tmp/mowis-initrd.cpio.gz 2>&1"
run "ls -la /tmp/mowis-initrd.cpio.gz"
run "gzip -dc /tmp/mowis-initrd.cpio.gz | cpio -t 2>&1 | head -15"

sub "kernel discovery"
run "ls /boot/vmlinuz* | head -3"
KERNEL=$(ls -t /boot/vmlinuz-* 2>/dev/null | head -1)
echo "selected kernel: $KERNEL"

sub "boot VM in background (output to $BOOT_LOG)"
BOOT_LOG=/tmp/boot.log
# Memory 2GB so the kernel + initramfs decompress comfortably. Quiet kernel
# noise but keep our executor's stderr.
sudo RUST_LOG=info target/release/mowisd boot \
    --kernel "$KERNEL" \
    --initrd /tmp/mowis-initrd.cpio.gz \
    --memory-mb 2048 \
    --vcpus 2 \
    --cid 42 \
    --port 5252 > "$BOOT_LOG" 2>&1 &
BOOT_PID=$!
echo "boot pid=$BOOT_PID; polling for guest executor (up to 60s)"

# Wait for executor inside guest to start. Poll ping.
GUEST_UP=0
for i in $(seq 1 30); do
    sleep 2
    if timeout 3 target/release/mowisd ping --cid 42 --port 5252 >/dev/null 2>&1; then
        echo "guest reachable after ${i}x2s"
        GUEST_UP=1
        break
    fi
done
if [[ "$GUEST_UP" -eq 0 ]]; then
    echo "*** guest never came up — VM boot likely failed; see qemu log below ***"
fi

sub "ping guest"
run "timeout 10 target/release/mowisd ping --cid 42 --port 5252 2>&1"

sub "exec inside guest (no sandbox — should print hello)"
run "timeout 20 target/release/mowisd exec --cid 42 --port 5252 --no-sandbox -- /bin/echo hello-from-guest-vm 2>&1"

sub "qemu+guest log (last 120 lines, includes kernel boot + executor stderr)"
run "tail -120 $BOOT_LOG"

step "DONE"
date -u
