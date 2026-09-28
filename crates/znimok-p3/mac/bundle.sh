#!/bin/bash
# Builds "Znimok P3.app", signs it with the stable "Znimok Dev" identity and publishes it to
# /Users/Shared/znimok-builds for the owner's account. Run from the repo root on the Mac.
set -euo pipefail
cd "$(dirname "$0")/../../.."
export PATH="$HOME/.cargo/bin:$PATH"
BUILD="$(git rev-parse --short HEAD)$(git diff --quiet || echo -dirty)"
ZNIMOK_BUILD="$BUILD" cargo build --release -p znimok-p3
SHA1="$(bash crates/znimok-p3/mac/make-identity.sh | tail -1)"
KC="$HOME/Library/Keychains/znimok-dev.keychain-db"
security unlock-keychain -p "$(cat "$HOME/.znimok-sign/pass")" "$KC"
APP="target/p3/Znimok P3.app"
rm -rf "$APP"; mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/znimok-p3 "$APP/Contents/MacOS/znimok-p3"
sed "s/@BUILD@/$BUILD/" crates/znimok-p3/mac/Info.plist > "$APP/Contents/Info.plist"
codesign --force --sign "$SHA1" --keychain "$KC" --identifier ua.plum.znimok.p3 --timestamp=none "$APP"
codesign --verify --strict "$APP"
codesign -d -r- "$APP" 2>&1 | grep designated
DEST="/Users/Shared/znimok-builds"
rm -rf "$DEST/Znimok P3.app"
ditto "$APP" "$DEST/Znimok P3.app"
chmod -R a+rX "$DEST/Znimok P3.app"
echo "published: $DEST/Znimok P3.app ($BUILD)"
