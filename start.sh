#!/bin/bash
cd /workspaces/MowisAI/mowisai-engine
sudo bash setup_rootfs.sh
sudo chroot ./rootfs /bin/sh -c "apk add --no-cache nodejs npm python3 py3-pip"
sudo pkill mowisai-engine 2>/dev/null || true
sleep 1
sudo ./target/debug/mowisai-engine > /tmp/engine.log 2>&1 &
sleep 2
cat /tmp/engine.log
echo "✅ MowisAI ready!"
