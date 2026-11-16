#!/usr/bin/env bash
# Create the "MiniMacPad Dev" code-signing identity (docs/RELEASING.md), once.
#
# Every build of the app, local or released, is signed with it: macOS ties the app's privacy
# permissions to the signature, and the in-app updater only installs builds signed by it. The
# certificate is self-signed and used for nothing else. The identity is saved to
# ~/.config/minimacpad/ (back it up) and imported into the login keychain for codesign.
set -euo pipefail

name="MiniMacPad Dev"
dir="$HOME/.config/minimacpad"
if security find-identity -p codesigning | grep -q "\"$name\""; then
  echo "\"$name\" is already in your keychain."
  exit 0
fi
if [ -f "$dir/signing.p12" ]; then
  echo "$dir/signing.p12 exists but the identity is not in your keychain; importing it."
else
  mkdir -p "$dir"
  chmod 700 "$dir"
  tmp="$(mktemp -d)"
  trap 'rm -rf "$tmp"' EXIT
  cat > "$tmp/cert.conf" <<CONF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = $name
[ext]
basicConstraints = critical,CA:false
keyUsage = critical,digitalSignature
extendedKeyUsage = critical,codeSigning
CONF
  # The system OpenSSL writes a PKCS#12 that `security import` reads.
  /usr/bin/openssl req -x509 -newkey rsa:2048 -nodes -days 7300 -config "$tmp/cert.conf" \
    -keyout "$tmp/key.pem" -out "$tmp/cert.pem" 2>/dev/null
  password="$(/usr/bin/openssl rand -hex 24)"
  (umask 077
   /usr/bin/openssl pkcs12 -export -inkey "$tmp/key.pem" -in "$tmp/cert.pem" -name "$name" \
     -passout "pass:$password" -out "$dir/signing.p12"
   printf '%s' "$password" > "$dir/signing.password")
fi
security import "$dir/signing.p12" -k "$HOME/Library/Keychains/login.keychain-db" \
  -P "$(cat "$dir/signing.password")" -T /usr/bin/codesign >/dev/null
echo "Created \"$name\". Back up $dir/signing.p12 and signing.password:"
echo "without them, installed copies of the app can never be updated again."
