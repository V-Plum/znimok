#!/bin/bash
# Double-click in Finder: runs the Znimok prototype self-test in a real window and writes the
# report and window snapshots to /Users/Shared/znimok-builds/inbox/app-selftest-<user> for the agent.
DIR="/Users/Shared/znimok-builds"
OUT="$DIR/inbox/app-selftest-$(whoami)"
rm -rf "$OUT" "/tmp/znimok-selftest-lib-$(whoami)"
mkdir -p "$OUT"
ZNIMOK_LIBRARY="/tmp/znimok-selftest-lib-$(whoami)" ZNIMOK_SELFTEST="$OUT" \
  "$DIR/Znimok.app/Contents/MacOS/znimok-app" "$DIR/sample.png" 2>&1 | tee "$OUT/console.txt"
echo "exit code: ${PIPESTATUS[0]}" | tee -a "$OUT/console.txt"
chmod -R a+rwX "$OUT"
echo
echo "Готово. Результат: $OUT — можна закрити це вікно."
