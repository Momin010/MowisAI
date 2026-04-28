#!/bin/bash
set -euo pipefail

ROOTFS_DIR=$(mktemp -d)
ROOTFS_IMG="$HOME/.mowis/vm-assets/mowis-rootfs.ext4"
VM_ASSETS="$HOME/.mowis/vm-assets"
SIZE_MB=512

mkdir -p "$VM_ASSETS"

# Download kernel (Firecracker-compatible v5.10+ minimal)
if [ ! -f "$VM_ASSETS/vmlinux" ]; then
  echo "Downloading vmlinux..."
  curl -fsSL -o "$VM_ASSETS/vmlinux" \
    https://s3.amazonaws.com/spec.ccfc.min/ci-artifacts/kernels/x86_64/vmlinux-5.10.217
fi

# Create ext4 image
echo "Creating ext4 rootfs image..."
dd if=/dev/zero of="$ROOTFS_IMG" bs=1M count=$SIZE_MB
mkfs.ext4 "$ROOTFS_IMG"

# Mount
sudo mount -o loop "$ROOTFS_IMG" "$ROOTFS_DIR"

# Install Alpine minimal
echo "Installing Alpine base..."
curl -fsSL https://dl-cdn.alpinelinux.org/alpine/v3.19/releases/x86_64/alpine-minirootfs-3.19.1-x86_64.tar.gz | sudo tar -xz -C "$ROOTFS_DIR"

# Install packages
sudo chroot "$ROOTFS_DIR" /bin/sh -c &#39;
  apk update
  apk add --no-cache \
    python3 py3-pip \
    nodejs npm \
    git curl bash \
    openssh-server openssh-client \
    docker-cli containerd runc \
    coreutils base64

  mkdir -p /workspace

  cat > /init <<\&#39;EOF\&#39;
#!/bin/sh
mount -t proc proc /proc
mount -t sysfs sysfs /sys
mount -t devtmpfs devtmpfs /dev
mkdir -p /root/.ssh
chmod 700 /root/.ssh
hostname mowis-sandbox
ip link set lo up
echo "MOWIS_READY" > /dev/ttyS0
/usr/sbin/sshd -D
EOF
  chmod +x /init

  ssh-keygen -A
&#39;

sudo umount "$ROOTFS_DIR"
rmdir "$ROOTFS_DIR"

echo "✅ RootFS built: $ROOTFS_IMG (Alpine 3.19, 512MB)"
echo "Kernel: $VM_ASSETS/vmlinux"
echo "Ready for QEMU/Firecracker."

