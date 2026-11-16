#!/usr/bin/env bash
# Store the "MiniMacPad Dev" signing identity as GitHub Actions secrets for
# .github/workflows/release.yml (docs/RELEASING.md). Uses `gh`; asks first. Pass the repo as $1 to
# target another repo than this checkout's.
set -euo pipefail

dir="$HOME/.config/minimacpad"
repo=(${1:+--repo "$1"})
[ -f "$dir/signing.p12" ] || { echo "Run scripts/create-signing-identity.sh first." >&2; exit 1; }
read -r -p "Upload the signing identity in $dir to GitHub Actions secrets? [y/N] " answer
[ "$answer" = y ] || [ "$answer" = Y ] || exit 0
base64 -i "$dir/signing.p12" | gh secret set MAC_SIGNING_P12_BASE64 ${repo[@]+"${repo[@]}"}
gh secret set MAC_SIGNING_P12_PASSWORD ${repo[@]+"${repo[@]}"} < "$dir/signing.password"
echo "Done."
