#!/bin/bash
# Builds ZnimokThumbnail.appex (Finder thumbnails of .znimok, ZK-76): the Rust core as a static
# library + the Swift extension, ad-hoc signed with the App Sandbox (extensions must be
# sandboxed). Put the result into Znimok.app/Contents/PlugIns/ and sign the app afterwards.
#
#   bash crates/znimok-thumbnail/mac/build-appex.sh [out-dir] [version]
set -euo pipefail
cd "$(dirname "$0")/../../.."
OUT="${1:-target/appex}"
VERSION="${2:-0.0.0}"
BUILD="$(git rev-parse --short HEAD 2>/dev/null || echo dev)"
HERE=crates/znimok-thumbnail/mac

cargo build --release -p znimok-thumbnail
APPEX="$OUT/ZnimokThumbnail.appex"
rm -rf "$APPEX"
mkdir -p "$APPEX/Contents/MacOS"
swiftc -O -parse-as-library -module-name ZnimokThumbnail \
    "$HERE/ThumbnailProvider.swift" \
    -o "$APPEX/Contents/MacOS/ZnimokThumbnail" \
    -L target/release -lznimok_thumbnail \
    -framework QuickLookThumbnailing -framework ImageIO -framework CoreGraphics \
    -Xlinker -e -Xlinker _NSExtensionMain \
    -Xlinker -rpath -Xlinker /usr/lib/swift \
    -target "$(uname -m)-apple-macos15.0"
sed -e "s/@VERSION@/$VERSION/" -e "s/@BUILD@/$BUILD/" "$HERE/Info.plist" > "$APPEX/Contents/Info.plist"
codesign --force --sign "${ZNIMOK_SIGN_IDENTITY:--}" --entitlements "$HERE/sandbox.entitlements" "$APPEX"
codesign --verify --strict "$APPEX"
echo "built: $APPEX"
