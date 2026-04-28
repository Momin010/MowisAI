#!/usr/bin/env bash
set -euo pipefail

# Build Alpine Linux VM images with embedded agentd
# Produces qcow2 images for QEMU/Virtualization.framework and tar.gz for WSL2

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
STATIC_DIR="$PROJECT_ROOT/target/static"
OUTPUT_DIR="$PROJECT_ROOT/target/images"
WORK_DIR="$PROJECT_ROOT/target/alpine-work"

ALPINE_VERSION="3.19"
ALPINE_MIRROR="https://dl-cdn.alpinelinux.org/alpine"

echo "==> Building Alpine Linux images with agentd"
echo "    Alpine version: $ALPINE_VERSION"
echo "    Output dir: $OUTPUT_DIR"

# Check static binaries exist
if [ ! -f "$STATIC_DIR/agentd-x86_64" ]; then
    echo "Error: Static x86_64 binary not found. Run build-static-agentd.sh first."
    exit 1
fi

if [ ! -f "$STATIC_DIR/agentd-aarch64" ]; then
    echo "Error: Static aarch64 binary not found. Run build-static-agentd.sh first."
    exit 1
fi

mkdir -p "$OUTPUT_DIR"
rm -rf "$WORK_DIR"
mkdir -p "$WORK_DIR"

# Function to build rootfs for a specific architecture
build_rootfs() {
    local arch=$1
    local agentd_binary=$2
    local rootfs_dir="$WORK_DIR/rootfs-$arch"
    
    echo "==> Building $arch rootfs"
    mkdir -p "$rootfs_dir"
    
    # Download Alpine mini rootfs
    local alpine_arch=$arch
    if [ "$arch" = "x86_64" ]; then
        alpine_arch="x86_64"
    elif [ "$arch" = "aarch64" ]; then
        alpine_arch="aarch64"
    fi
    
    local rootfs_url="$ALPINE_MIRROR/v$ALPINE_VERSION/releases/$alpine_arch/alpine-minirootfs-$ALPINE_VERSION.0-$alpine_arch.tar.gz"
    echo "    Downloading: $rootfs_url"
    
    curl -fsSL "$rootfs_url" | tar -xz -C "$rootfs_dir"
    
    # Copy static agentd binary
    echo "    Installing agentd binary"
    mkdir -p "$rootfs_dir/usr/local/bin"
    cp "$agentd_binary" "$rootfs_dir/usr/local/bin/agentd"
    chmod +x "$rootfs_dir/usr/local/bin/agentd"
    
    # Create init script
    echo "    Creating init script"
    mkdir -p "$rootfs_dir/etc/init.d"
    cat > "$rootfs_dir/etc/init.d/agentd" <<'EOF'
#!/sbin/openrc-run

name="agentd"
description="MowisAI Agent Daemon"
command="/usr/local/bin/agentd"
command_args="socket --path /tmp/agentd.sock"
command_background="yes"
pidfile="/run/agentd.pid"

depend() {
    need net
    after firewall
}

start_pre() {
    # Generate auth token if required
    if [ -n "$AGENTD_AUTH_REQUIRED" ]; then
        /usr/local/bin/agentd generate-token > /root/.agentd-token
        chmod 600 /root/.agentd-token
        export AGENTD_AUTH_TOKEN=$(cat /root/.agentd-token)
    fi
}
EOF
    chmod +x "$rootfs_dir/etc/init.d/agentd"
    
    # Configure networking (DHCP)
    echo "    Configuring networking"
    mkdir -p "$rootfs_dir/etc/network"
    cat > "$rootfs_dir/etc/network/interfaces" <<'EOF'
auto lo
iface lo inet loopback

auto eth0
iface eth0 inet dhcp
EOF
    
    # Install required packages via chroot
    echo "    Installing packages (skopeo, ca-certificates)"
    # Note: This requires running on Linux with chroot support
    # For cross-platform builds, we'll skip this and document manual setup
    if [ "$(uname -s)" = "Linux" ] && [ "$(id -u)" = "0" ]; then
        # Mount proc, sys, dev for chroot
        mount -t proc none "$rootfs_dir/proc" || true
        mount -t sysfs none "$rootfs_dir/sys" || true
        mount -o bind /dev "$rootfs_dir/dev" || true
        
        # Install packages
        chroot "$rootfs_dir" /bin/sh -c "apk add --no-cache openrc skopeo ca-certificates"
        
        # Enable agentd service
        chroot "$rootfs_dir" /bin/sh -c "rc-update add agentd default"
        
        # Cleanup
        umount "$rootfs_dir/proc" || true
        umount "$rootfs_dir/sys" || true
        umount "$rootfs_dir/dev" || true
    else
        echo "    ⚠ Skipping package installation (requires Linux root)"
        echo "    Manual setup required: apk add openrc skopeo ca-certificates"
    fi
    
    echo "    ✓ $arch rootfs complete"
}

# Build x86_64 rootfs
build_rootfs "x86_64" "$STATIC_DIR/agentd-x86_64"

# Build aarch64 rootfs
build_rootfs "aarch64" "$STATIC_DIR/agentd-aarch64"

# Create qcow2 images
create_qcow2() {
    local arch=$1
    local rootfs_dir="$WORK_DIR/rootfs-$arch"
    local image_file="$OUTPUT_DIR/alpine-$arch.qcow2"
    local raw_image="$WORK_DIR/alpine-$arch.raw"
    
    echo "==> Creating qcow2 image for $arch"
    
    # Create 1GB sparse disk image
    dd if=/dev/zero of="$raw_image" bs=1M count=0 seek=1024
    
    # Format as ext4
    mkfs.ext4 -F "$raw_image"
    
    # Mount and copy rootfs
    local mount_point="$WORK_DIR/mnt-$arch"
    mkdir -p "$mount_point"
    
    if [ "$(uname -s)" = "Linux" ] && [ "$(id -u)" = "0" ]; then
        mount -o loop "$raw_image" "$mount_point"
        cp -a "$rootfs_dir"/* "$mount_point/"
        umount "$mount_point"
    else
        echo "    ⚠ Skipping image creation (requires Linux root)"
        return
    fi
    
    # Convert to qcow2 with compression
    qemu-img convert -f raw -O qcow2 -c "$raw_image" "$image_file"
    
    # Cleanup
    rm "$raw_image"
    
    echo "    ✓ Created: $image_file"
    ls -lh "$image_file"
}

# Create WSL2 distribution tarball
create_wsl2_tarball() {
    local arch=$1
    local rootfs_dir="$WORK_DIR/rootfs-$arch"
    local tarball="$OUTPUT_DIR/alpine-wsl2-$arch.tar.gz"
    
    echo "==> Creating WSL2 tarball for $arch"
    
    tar -czf "$tarball" -C "$rootfs_dir" .
    
    echo "    ✓ Created: $tarball"
    ls -lh "$tarball"
}

# Check if we can create images (requires Linux + root + qemu-img)
if [ "$(uname -s)" = "Linux" ] && [ "$(id -u)" = "0" ] && command -v qemu-img &> /dev/null; then
    create_qcow2 "x86_64"
    create_qcow2 "aarch64"
else
    echo "==> Skipping qcow2 image creation (requires Linux root + qemu-img)"
fi

# Create WSL2 tarballs (doesn't require root)
create_wsl2_tarball "x86_64"
create_wsl2_tarball "aarch64"

echo "==> Alpine images built successfully"
echo "    Output directory: $OUTPUT_DIR"
ls -lh "$OUTPUT_DIR"
