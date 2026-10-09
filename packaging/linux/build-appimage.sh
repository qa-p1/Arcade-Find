#!/usr/bin/env bash
# Builds dist/Arcade-Find-x86_64.AppImage from a release binary.
#
#   packaging/linux/build-appimage.sh target/release/arcade-find dist
#
# appimagetool is downloaded once (pinned release, SHA-256 checked) into
# target/tools; nothing is installed system-wide. libxkbcommon is the only
# directly linked library besides glibc and is present on every desktop;
# X11, Wayland and OpenGL libraries are loaded at run time from the system.
set -euo pipefail
BIN=${1:-target/release/arcade-find}
DIST=${2:-dist}
ARCH=x86_64
TOOL_VERSION=1.9.0
TOOL_SHA256=46fdd785094c7f6e545b61afcfb0f3d98d8eab243f644b4b17698c01d06083d1
TOOL=target/tools/appimagetool-$TOOL_VERSION-$ARCH.AppImage

mkdir -p target/tools "$DIST"
if [[ ! -x "$TOOL" ]]; then
  curl -fsSL -o "$TOOL.part" "https://github.com/AppImage/appimagetool/releases/download/$TOOL_VERSION/appimagetool-$ARCH.AppImage"
  echo "$TOOL_SHA256  $TOOL.part" | sha256sum -c -
  chmod +x "$TOOL.part" && mv "$TOOL.part" "$TOOL"
fi

APPDIR=$(mktemp -d)/Arcade-Find.AppDir
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" "$APPDIR/usr/share/icons/hicolor/scalable/apps" "$APPDIR/usr/share/icons/hicolor/256x256/apps" "$APPDIR/usr/share/doc/arcade-find"
install -m755 "$BIN" "$APPDIR/usr/bin/arcade-find"
install -m644 packaging/linux/arcade-find.desktop "$APPDIR/usr/share/applications/arcade-find.desktop"
cp packaging/linux/arcade-find.desktop "$APPDIR/arcade-find.desktop"
"$BIN" --export-icon "$APPDIR/usr/share/icons/hicolor/scalable/apps/arcade-find.svg"
"$BIN" --export-icon "$APPDIR/usr/share/icons/hicolor/256x256/apps/arcade-find.png" --size 256
cp "$APPDIR/usr/share/icons/hicolor/256x256/apps/arcade-find.png" "$APPDIR/arcade-find.png"
cp LICENSE-MIT LICENSE-APACHE THIRD_PARTY_NOTICES.md "$APPDIR/usr/share/doc/arcade-find/"
cat >"$APPDIR/AppRun" <<'RUN'
#!/bin/sh
HERE=$(dirname "$(readlink -f "$0")")
exec "$HERE/usr/bin/arcade-find" "$@"
RUN
chmod +x "$APPDIR/AppRun"
ARCH=$ARCH "$TOOL" --appimage-extract-and-run "$APPDIR" "$DIST/Arcade-Find-$ARCH.AppImage"
echo "built $DIST/Arcade-Find-$ARCH.AppImage"
