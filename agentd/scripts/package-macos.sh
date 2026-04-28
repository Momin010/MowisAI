#!/usr/bin/env bash
set -euo pipefail

# Package MowisAI for macOS as an app bundle

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
OUTPUT_DIR="$PROJECT_ROOT/target/macos"
APP_NAME="MowisAI.app"

echo "==> Packaging MowisAI for macOS"
echo "    Output: $OUTPUT_DIR/$APP_NAME"

# Check if running on macOS
if [ "$(uname -s)" != "Darwin" ]; then
    echo "Error: This script must run on macOS"
    exit 1
fi

# Build release binary
echo "==> Building release binary"
cd "$PROJECT_ROOT/mowis-gui"
cargo build --release

# Compile Swift shim
echo "==> Compiling Swift shim"
swiftc \
    -o "$PROJECT_ROOT/target/release/vm_launcher" \
    "$PROJECT_ROOT/mowis-gui/src/launchers/macos/vm_launcher.swift" \
    -framework Virtualization

# Create app bundle structure
echo "==> Creating app bundle structure"
rm -rf "$OUTPUT_DIR/$APP_NAME"
mkdir -p "$OUTPUT_DIR/$APP_NAME/Contents/MacOS"
mkdir -p "$OUTPUT_DIR/$APP_NAME/Contents/Resources"
mkdir -p "$OUTPUT_DIR/$APP_NAME/Contents/Frameworks"

# Copy main binary
cp "$PROJECT_ROOT/target/release/mowisai" \
   "$OUTPUT_DIR/$APP_NAME/Contents/MacOS/mowisai"

# Copy Swift shim
cp "$PROJECT_ROOT/target/release/vm_launcher" \
   "$OUTPUT_DIR/$APP_NAME/Contents/MacOS/vm_launcher"

# Copy Alpine images
echo "==> Bundling Alpine images"
if [ -f "$PROJECT_ROOT/target/images/alpine-x86_64.qcow2" ]; then
    cp "$PROJECT_ROOT/target/images/alpine-x86_64.qcow2" \
       "$OUTPUT_DIR/$APP_NAME/Contents/Resources/"
fi

if [ -f "$PROJECT_ROOT/target/images/alpine-aarch64.qcow2" ]; then
    cp "$PROJECT_ROOT/target/images/alpine-aarch64.qcow2" \
       "$OUTPUT_DIR/$APP_NAME/Contents/Resources/"
fi

# Copy QEMU binaries (if available)
echo "==> Bundling QEMU binaries (optional)"
if command -v qemu-system-x86_64 &> /dev/null; then
    cp "$(which qemu-system-x86_64)" \
       "$OUTPUT_DIR/$APP_NAME/Contents/Resources/" || true
fi

if command -v qemu-system-aarch64 &> /dev/null; then
    cp "$(which qemu-system-aarch64)" \
       "$OUTPUT_DIR/$APP_NAME/Contents/Resources/" || true
fi

# Create Info.plist
echo "==> Creating Info.plist"
cat > "$OUTPUT_DIR/$APP_NAME/Contents/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <key>CFBundleExecutable</key>
    <string>mowisai</string>
    <key>CFBundleIdentifier</key>
    <string>com.mowisai.app</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleName</key>
    <string>MowisAI</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleVersion</key>
    <string>1</string>
    <key>LSMinimumSystemVersion</key>
    <string>10.15</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>NSRequiresAquaSystemAppearance</key>
    <false/>
</dict>
</plist>
EOF

# Set executable permissions
chmod +x "$OUTPUT_DIR/$APP_NAME/Contents/MacOS/mowisai"
chmod +x "$OUTPUT_DIR/$APP_NAME/Contents/MacOS/vm_launcher"

# Code signing (if certificate available)
if security find-identity -v -p codesigning | grep -q "Developer ID Application"; then
    echo "==> Code signing app bundle"
    codesign --force --deep --sign "Developer ID Application" \
        "$OUTPUT_DIR/$APP_NAME"
else
    echo "==> Skipping code signing (no certificate found)"
    echo "    For distribution, you'll need a Developer ID certificate"
fi

# Create DMG
echo "==> Creating DMG"
DMG_PATH="$OUTPUT_DIR/MowisAI.dmg"
rm -f "$DMG_PATH"

hdiutil create -volname "MowisAI" \
    -srcfolder "$OUTPUT_DIR/$APP_NAME" \
    -ov -format UDZO \
    "$DMG_PATH"

echo "==> macOS package created successfully"
echo "    App bundle: $OUTPUT_DIR/$APP_NAME"
echo "    DMG: $DMG_PATH"
ls -lh "$DMG_PATH"
