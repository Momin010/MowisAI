#!/usr/bin/env bash
# Bootstrap a GCE instance for testing the new mowis-host / mowis-executor
# architecture. Run via:
#
#   gcloud compute ssh mowis-test --zone=us-central1-a --project=YOUR_PROJECT
#   sudo bash scripts/setup-gcp-test.sh
#
# Requirements on the instance:
#   - Ubuntu 22.04 or 24.04
#   - Nested virtualization enabled (--enable-nested-virtualization)
#   - Firewall allows tcp:443 from 0.0.0.0/0
#
# What this does:
#   1. Installs QEMU/KVM, skopeo, cpio, build deps, Rust
#   2. Verifies /dev/kvm exists
#   3. Creates a `mowis` user with sudo + a pre-baked SSH key for Claude
#   4. Opens sshd on port 443 (this sandbox can only reach 80/443 outbound)
#   5. Clones the repo on the refactor branch and builds the new crates
#
# Update CLAUDE_PUBKEY below if Claude rotates its key.

set -euo pipefail

CLAUDE_PUBKEY="ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIFjNGUwM2WxQeC8mnnyBBhmGs3WZGt1m6ttAU/djLDd4 claude-mowis-test"
REPO_URL="https://github.com/momin010/mowisai.git"
BRANCH="claude/refactor-agentd-mowis-zR6sh"

if [[ $EUID -ne 0 ]]; then
    echo "must run as root (use sudo)" >&2
    exit 1
fi

echo "=== installing deps ==="
apt-get update -qq
DEBIAN_FRONTEND=noninteractive apt-get install -y -qq \
    qemu-system-x86 qemu-kvm qemu-utils ovmf \
    skopeo cpio tar wget curl git \
    build-essential pkg-config libssl-dev

echo "=== checking KVM ==="
if [[ ! -e /dev/kvm ]]; then
    echo "ERROR: /dev/kvm missing — nested virtualization not enabled" >&2
    exit 1
fi
kvm-ok || true

echo "=== creating mowis user ==="
useradd -m -s /bin/bash -G sudo,kvm mowis 2>/dev/null || true
echo "mowis ALL=(ALL) NOPASSWD:ALL" > /etc/sudoers.d/mowis
install -d -m 700 -o mowis -g mowis /home/mowis/.ssh
echo "$CLAUDE_PUBKEY" > /home/mowis/.ssh/authorized_keys
chmod 600 /home/mowis/.ssh/authorized_keys
chown mowis:mowis /home/mowis/.ssh/authorized_keys

echo "=== sshd on port 443 ==="
grep -q "^Port 443" /etc/ssh/sshd_config || echo "Port 443" >> /etc/ssh/sshd_config
grep -q "^Port 22"  /etc/ssh/sshd_config || echo "Port 22"  >> /etc/ssh/sshd_config
systemctl restart ssh

echo "=== installing rust as mowis ==="
sudo -u mowis bash -c '
    curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs |
        sh -s -- -y --default-toolchain stable --profile minimal
    grep -q cargo/env ~/.bashrc || echo "source \$HOME/.cargo/env" >> ~/.bashrc
'

echo "=== cloning repo + building ==="
sudo -u mowis bash -c "
    set -e
    cd ~
    if [[ ! -d mowisai ]]; then
        git clone '$REPO_URL'
    fi
    cd mowisai
    git fetch origin
    git checkout '$BRANCH'
    git pull origin '$BRANCH'
    source \$HOME/.cargo/env
    cargo build -p mowis-host -p mowis-executor
"

echo
echo "=== DONE ==="
echo "Public IP: $(curl -s ifconfig.me || echo 'unknown')"
echo "Claude can now connect: ssh mowis@<ip> -p 443"
