#!/bin/bash
# Builds "Znimok.app" (the prototype app), signs it with the stable "Znimok Dev" identity (same
# as P3, so a future Screen Recording grant survives rebuilds) and publishes it to
# /Users/Shared/znimok-builds for the owner's account. Run from the repo root on the Mac.
set -euo pipefail
cd "$(dirname "$0")/../../.."
export PATH="$HOME/.cargo/bin:$PATH"
BUILD="$(git rev-parse --short HEAD)$(git diff --quiet || echo -dirty)"
cargo build --release -p znimok-app
SHA1="$(bash crates/znimok-p3/mac/make-identity.sh | tail -1)"
KC="$HOME/Library/Keychains/znimok-dev.keychain-db"
security unlock-keychain -p "$(cat "$HOME/.znimok-sign/pass")" "$KC"
APP="target/app/Znimok.app"
rm -rf "$APP"; mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/znimok-app "$APP/Contents/MacOS/znimok-app"
cp crates/znimok-app/icons/Znimok.icns "$APP/Contents/Resources/Znimok.icns"
# Sparkle (ZK-143): the same framework as the release, cached in ~/.znimok-sparkle. A dev build
# has no feed (SUFeedURL is stripped below) — the page says updates are not set up; to try the
# whole flow, start the app with ZNIMOK_SPARKLE_FEED=<appcast url>.
SPARKLE_VERSION=2.10.0
SPARKLE_SHA256=c2bf58aa8387266ac179357b1415d6f2635f044da8be41042af32425dae6da0c
SPARKLE_DIR="$HOME/.znimok-sparkle"
if [ ! -d "$SPARKLE_DIR/Sparkle.framework" ]; then
    mkdir -p "$SPARKLE_DIR"
    curl -sSL -o "$SPARKLE_DIR/Sparkle-$SPARKLE_VERSION.tar.xz" "https://github.com/sparkle-project/Sparkle/releases/download/$SPARKLE_VERSION/Sparkle-$SPARKLE_VERSION.tar.xz"
    echo "$SPARKLE_SHA256  $SPARKLE_DIR/Sparkle-$SPARKLE_VERSION.tar.xz" | shasum -a 256 -c -
    tar -xf "$SPARKLE_DIR/Sparkle-$SPARKLE_VERSION.tar.xz" -C "$SPARKLE_DIR" Sparkle.framework
fi
mkdir -p "$APP/Contents/Frameworks"
ditto "$SPARKLE_DIR/Sparkle.framework" "$APP/Contents/Frameworks/Sparkle.framework"
# The working build has its own identifier and name (ZK-125): with the release's
# ua.plum.znimok.app and version 0.0.0 (newer than any -preview by semver) Launch Services
# preferred it over /Applications/Znimok.app, which declares the .znimok type and carries the
# thumbnail extension — Finder then showed blank sheets. It declares no document types itself.
ID="ua.plum.znimok.app.dev"
sed -e "s/@BUILD@/$BUILD/" \
    -e "s|<string>ua.plum.znimok.app</string>|<string>$ID</string>|" \
    -e "s|<string>Znimok</string>|<string>Znimok Dev</string>|g" \
    -e "/<key>SUFeedURL<\/key>/d" \
    crates/znimok-app/mac/Info.plist > "$APP/Contents/Info.plist"
grep -q "<string>$ID</string>" "$APP/Contents/Info.plist"
codesign --force --sign "$SHA1" --keychain "$KC" --identifier "$ID" --timestamp=none "$APP"
codesign --verify --strict "$APP"
DEST="/Users/Shared/znimok-builds"
rm -rf "$DEST/Znimok.app"
ditto "$APP" "$DEST/Znimok.app"
chmod -R a+rX "$DEST/Znimok.app"
echo "published: $DEST/Znimok.app ($BUILD)"
