#!/usr/bin/env bash
# Builds dist/Arcade Find.app and dist/Arcade-Find-<version>-<arch>.dmg (macOS only).
#
#   packaging/macos/build-app.sh target/release/arcade-find 0.1.0 arm64 dist
#
# The bundle is not signed or notarized; Gatekeeper asks on first open.
set -euo pipefail
BIN=$1
VERSION=$2
ARCH=${3:-$(uname -m)}
DIST=${4:-dist}
APP="$DIST/Arcade Find.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
install -m755 "$BIN" "$APP/Contents/MacOS/arcade-find"
ICON_ROOT=$(mktemp -d)
trap 'rm -rf "$ICON_ROOT"' EXIT
ICONSET="$ICON_ROOT/arcade-find.iconset"
mkdir -p "$ICONSET"
for s in 16 32 128 256 512; do
  "$BIN" --export-icon "$ICONSET/icon_${s}x${s}.png" --size $s
  "$BIN" --export-icon "$ICONSET/icon_${s}x${s}@2x.png" --size $((s * 2))
done
iconutil -c icns "$ICONSET" -o "$APP/Contents/Resources/arcade-find.icns"
cp LICENSE-MIT LICENSE-APACHE THIRD_PARTY_NOTICES.md "$APP/Contents/Resources/"
mkdir -p "$APP/Contents/Resources/docs"
cp -R docs/user "$APP/Contents/Resources/docs/user"
cat >"$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Arcade Find</string>
  <key>CFBundleDisplayName</key><string>Arcade Find</string>
  <key>CFBundleIdentifier</key><string>io.github.qa-p1.arcade-find</string>
  <key>CFBundleVersion</key><string>$VERSION</string>
  <key>CFBundleShortVersionString</key><string>$VERSION</string>
  <key>CFBundleExecutable</key><string>arcade-find</string>
  <key>CFBundleIconFile</key><string>arcade-find</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
DMG_STAGE=$(mktemp -d)
trap 'rm -rf "$ICON_ROOT" "$DMG_STAGE"' EXIT
cp -R "$APP" "$DMG_STAGE/"
ln -s /Applications "$DMG_STAGE/Applications"
hdiutil create -volname "Arcade Find" -srcfolder "$DMG_STAGE" -ov -format UDZO "$DIST/Arcade-Find-$VERSION-$ARCH.dmg"
echo "built $DIST/Arcade-Find-$VERSION-$ARCH.dmg"
