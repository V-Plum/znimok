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
sed "s/@BUILD@/$BUILD/" crates/znimok-app/mac/Info.plist > "$APP/Contents/Info.plist"
codesign --force --sign "$SHA1" --keychain "$KC" --identifier ua.plum.znimok.app --timestamp=none "$APP"
codesign --verify --strict "$APP"
DEST="/Users/Shared/znimok-builds"
rm -rf "$DEST/Znimok.app"
ditto "$APP" "$DEST/Znimok.app"
chmod -R a+rX "$DEST/Znimok.app"
echo "published: $DEST/Znimok.app ($BUILD)"
