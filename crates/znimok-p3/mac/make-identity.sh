#!/bin/bash
# Creates (once) a self-signed code-signing identity "Znimok Dev" in a dedicated keychain of
# the build account. A stable identity keeps the Screen Recording permission across rebuilds:
# TCC remembers the designated requirement (bundle id + certificate), ad-hoc signatures change
# with every build. The private key never leaves this Mac and never goes into git.
set -euo pipefail
D="$HOME/.znimok-sign"
KC="$HOME/Library/Keychains/znimok-dev.keychain-db"
mkdir -p "$D"; chmod 700 "$D"
[ -f "$D/pass" ] || { openssl rand -hex 24 > "$D/pass"; chmod 600 "$D/pass"; }
P="$(cat "$D/pass")"
[ -f "$KC" ] || security create-keychain -p "$P" znimok-dev.keychain
security unlock-keychain -p "$P" "$KC"
security set-keychain-settings "$KC"   # no auto-lock
if ! security find-certificate -c "Znimok Dev" "$KC" >/dev/null 2>&1; then
  cat > "$D/req.cnf" <<'CNF'
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = Znimok Dev
O = Znimok (local development)
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
subjectKeyIdentifier = hash
CNF
  openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -config "$D/req.cnf" -keyout "$D/key.pem" -out "$D/cert.pem" 2>/dev/null
  openssl pkcs12 -export -inkey "$D/key.pem" -in "$D/cert.pem" -name "Znimok Dev" -out "$D/id.p12" -passout "pass:$P"
  security import "$D/id.p12" -k "$KC" -P "$P" -T /usr/bin/codesign >/dev/null
  security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$P" "$KC" >/dev/null
  rm -f "$D/key.pem" "$D/id.p12"
  echo "identity created"
fi
# Keep the keychain in the search list so codesign can find the key.
if ! security list-keychains -d user | grep -q znimok-dev; then
  security list-keychains -d user -s "$KC" $(security list-keychains -d user | tr -d '"')
fi
openssl x509 -in "$D/cert.pem" -noout -fingerprint -sha1 | sed 's/.*=//; s/://g'
