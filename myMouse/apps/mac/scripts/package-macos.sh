#!/usr/bin/env bash
set -euo pipefail

# Packaging scaffold for macOS (Apple Silicon). Run on macOS with Xcode CLT.
# This script does not sign or notarize; it only documents the expected steps.

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUILD_DIR="$ROOT/.build/release"
APP_BUNDLE="$ROOT/dist/MyMouse.app"
CONTENTS="$APP_BUNDLE/Contents"
MACOS="$CONTENTS/MacOS"
RESOURCES="$CONTENTS/Resources"

echo "==> Building release binary"
cd "$ROOT"
swift build -c release --arch arm64

echo "==> Assembling .app bundle"
rm -rf "$APP_BUNDLE"
mkdir -p "$MACOS" "$RESOURCES"
cp "$BUILD_DIR/MyMouseMac" "$MACOS/MyMouseMac"
cp "$ROOT/Info.plist" "$CONTENTS/Info.plist"

cat > "$CONTENTS/PkgInfo" <<'PKG'
APPL????
PKG

echo "==> Bundle created at $APP_BUNDLE"
echo "==> Next steps (manual, credentials required):"
echo "  1. codesign --deep --force --options runtime --entitlements MyMouseMac.entitlements --sign \"Developer ID Application: ...\" dist/MyMouse.app"
echo "  2. hdiutil create -volname myMouse -srcfolder dist/MyMouse.app -ov -format UDZO dist/myMouse-mac-arm64.dmg"
echo "  3. xcrun notarytool submit dist/myMouse-mac-arm64.dmg --apple-id ... --team-id ... --password ... --wait"
echo "  4. xcrun stapler staple dist/myMouse-mac-arm64.dmg"
