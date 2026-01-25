#!/bin/bash
# Build script for SpruceOS Installer macOS .app bundle

set -e

# Source cargo environment if available
source "$HOME/.cargo/env" 2>/dev/null || true

echo "🔨 Building SpruceOS Installer for macOS..."
echo ""

# Build release binary
echo "📦 Compiling release binary..."
cargo build --release

echo ""
echo "📁 Creating .app bundle..."

# Clean up any existing bundle
rm -rf "SpruceOSInstaller.app"

# Create .app bundle structure
mkdir -p "SpruceOSInstaller.app/Contents/MacOS"
mkdir -p "SpruceOSInstaller.app/Contents/Resources"

# Copy binary
cp target/release/spruceos-installer "SpruceOSInstaller.app/Contents/MacOS/spruceos-installer"
chmod +x "SpruceOSInstaller.app/Contents/MacOS/spruceos-installer"

# Copy Info.plist
cp "assets/Mac/Info.plist" "SpruceOSInstaller.app/Contents/"

# Copy 7zz if it exists
if [ -f "assets/Mac/7zz" ]; then
    cp "assets/Mac/7zz" "SpruceOSInstaller.app/Contents/Resources/7zz"
    chmod +x "SpruceOSInstaller.app/Contents/Resources/7zz"
    echo "   ✓ Included 7zz archive tool"
fi

# Create icon if source exists
if [ -f "assets/Icons/icon.png" ]; then
    echo "🎨 Creating app icon..."
    mkdir -p AppIcon.iconset
    sips -z 16 16     "assets/Icons/icon.png" --out "AppIcon.iconset/icon_16x16.png" >/dev/null 2>&1
    sips -z 32 32     "assets/Icons/icon.png" --out "AppIcon.iconset/icon_16x16@2x.png" >/dev/null 2>&1
    sips -z 32 32     "assets/Icons/icon.png" --out "AppIcon.iconset/icon_32x32.png" >/dev/null 2>&1
    sips -z 64 64     "assets/Icons/icon.png" --out "AppIcon.iconset/icon_32x32@2x.png" >/dev/null 2>&1
    sips -z 128 128   "assets/Icons/icon.png" --out "AppIcon.iconset/icon_128x128.png" >/dev/null 2>&1
    sips -z 256 256   "assets/Icons/icon.png" --out "AppIcon.iconset/icon_128x128@2x.png" >/dev/null 2>&1
    sips -z 256 256   "assets/Icons/icon.png" --out "AppIcon.iconset/icon_256x256.png" >/dev/null 2>&1
    sips -z 512 512   "assets/Icons/icon.png" --out "AppIcon.iconset/icon_256x256@2x.png" >/dev/null 2>&1
    sips -z 512 512   "assets/Icons/icon.png" --out "AppIcon.iconset/icon_512x512.png" >/dev/null 2>&1
    sips -z 1024 1024 "assets/Icons/icon.png" --out "AppIcon.iconset/icon_512x512@2x.png" >/dev/null 2>&1
    iconutil -c icns AppIcon.iconset -o "SpruceOSInstaller.app/Contents/Resources/AppIcon.icns" 2>/dev/null
    rm -rf AppIcon.iconset
    echo "   ✓ App icon created"
fi

# Remove quarantine attribute
xattr -cr "SpruceOSInstaller.app" 2>/dev/null || true

echo ""
echo "✅ Build complete!"
echo ""
echo "📍 App location: $(pwd)/SpruceOSInstaller.app"
echo ""
echo "To run: open SpruceOSInstaller.app"
echo ""
echo "⚠️  Remember to grant Full Disk Access:"
echo "   System Settings → Privacy & Security → Full Disk Access"
echo "   Click + and add SpruceOSInstaller.app"
