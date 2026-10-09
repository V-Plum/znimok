#!/usr/bin/env bash
# Notarise an .app or a .dmg with Apple and staple the ticket to it (ZK-299).
#
#   tools/mac_notarize.sh dist/Znimok.app
#   tools/mac_notarize.sh dist/Znimok-0.0.23-macos-arm64.dmg
#
# The App Store Connect API key comes from the environment: NOTARY_KEY (the .p8 text),
# NOTARY_KEY_ID, NOTARY_ISSUER. An .app is sent zipped (ditto), a .dmg as it is; on a refusal the
# notary's log is printed and the script fails. The same key notarises any of the team's apps.
set -euo pipefail

target="$1"
[[ -e "$target" ]] || { echo "no such file: $target" >&2; exit 2; }
: "${NOTARY_KEY:?}" "${NOTARY_KEY_ID:?}" "${NOTARY_ISSUER:?}"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
key="$work/AuthKey.p8"
printf '%s\n' "$NOTARY_KEY" > "$key"
auth=(--key "$key" --key-id "$NOTARY_KEY_ID" --issuer "$NOTARY_ISSUER")

if [[ "$target" == *.app ]]; then
  upload="$work/$(basename "$target" .app).zip"
  ditto -c -k --keepParent "$target" "$upload"
else
  upload="$target"
fi

out="$(xcrun notarytool submit "$upload" "${auth[@]}" --wait --timeout 40m --output-format json)"
echo "$out"
id="$(printf '%s' "$out" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("id",""))')"
status="$(printf '%s' "$out" | python3 -c 'import json,sys; print(json.load(sys.stdin).get("status",""))')"
if [[ "$status" != "Accepted" ]]; then
  echo "::error::notarisation of $(basename "$target"): $status"
  [[ -n "$id" ]] && xcrun notarytool log "$id" "${auth[@]}" || true
  exit 1
fi
# The log lists warnings even for an accepted submission: kept in the job's output.
xcrun notarytool log "$id" "${auth[@]}" | python3 -c 'import json,sys; d=json.load(sys.stdin); [print("notary:", i.get("severity"), i.get("path"), i.get("message")) for i in (d.get("issues") or [])]' || true

xcrun stapler staple "$target"
xcrun stapler validate "$target"
