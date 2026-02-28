#!/bin/bash

# MowisAI Engine - Alpine Linux RootFS Setup Script
# Downloads and prepares Alpine Linux minirootfs for container isolation

set -e

ALPINE_VERSION="3.19"
ARCH="x86_64"
ROOTFS_DIR="./rootfs"
ALPINE_URL="https://dl-cdn.alpinelinux.org/alpine/v${ALPINE_VERSION}/releases/${ARCH}/alpine-minirootfs-${ALPINE_VERSION}.0-${ARCH}.tar.gz"

echo "Setting up Alpine Linux ${ALPINE_VERSION} rootfs..."

# Create rootfs directory if it doesn't exist
if [ -d "$ROOTFS_DIR" ]; then
    echo "RootFS directory already exists. Cleaning up..."
    rm -rf "$ROOTFS_DIR"
fi

mkdir -p "$ROOTFS_DIR"
echo "Created directory: $ROOTFS_DIR"

# Download Alpine minirootfs
echo "Downloading Alpine Linux minirootfs..."
if ! wget -q "$ALPINE_URL" -O /tmp/alpine-minirootfs.tar.gz; then
    echo "Failed to download Alpine minirootfs. Trying with curl..."
    if ! curl -fsSL "$ALPINE_URL" -o /tmp/alpine-minirootfs.tar.gz; then
        echo "ERROR: Failed to download Alpine minirootfs from $ALPINE_URL"
        exit 1
    fi
fi

echo "Downloaded successfully to /tmp/alpine-minirootfs.tar.gz"

# Extract the rootfs
echo "Extracting rootfs to $ROOTFS_DIR..."
tar -xzf /tmp/alpine-minirootfs.tar.gz -C "$ROOTFS_DIR"
echo "Extraction complete"

# Copy resolv.conf for internet access
echo "Setting up DNS resolution..."
if [ -f /etc/resolv.conf ]; then
    cp /etc/resolv.conf "$ROOTFS_DIR/etc/resolv.conf"
    echo "Copied /etc/resolv.conf to container"
else
    echo "WARNING: /etc/resolv.conf not found on host. Container may not have internet access."
    # Create a basic resolv.conf as fallback
    echo "nameserver 8.8.8.8" > "$ROOTFS_DIR/etc/resolv.conf"
    echo "Created fallback resolv.conf with Google DNS"
fi

# Clean up downloaded archive
rm -f /tmp/alpine-minirootfs.tar.gz
echo "Cleaned up temporary files"

# Create necessary mount points for container
mkdir -p "$ROOTFS_DIR/proc"
mkdir -p "$ROOTFS_DIR/sys"
mkdir -p "$ROOTFS_DIR/dev"
mkdir -p "$ROOTFS_DIR/tmp"
echo "Created mount points"

echo ""
echo "✅ Alpine Linux rootfs setup complete!"
echo "Location: $ROOTFS_DIR"
echo "Size: $(du -sh $ROOTFS_DIR | cut -f1)"

# Create /bin symlink to /usr/bin (Alpine uses merged /usr)
echo "Setting up /bin symlink..."
cd "$ROOTFS_DIR"
rm -rf bin sbin
ln -s usr/bin bin
ln -s usr/bin sbin
echo "✅ /bin and /sbin linked to /usr/bin"
