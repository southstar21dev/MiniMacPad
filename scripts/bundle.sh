#!/usr/bin/env bash
# Build both programs, wrap them in MiniMacPad.app (in target/), sign it and install it to
# /Applications, replacing the copy there.
#
#   scripts/bundle.sh [--no-install]
#
# The version comes from scripts/version.sh: the latest release tag plus build metadata, or
# $MINIMACPAD_RELEASE_VERSION when the release workflow builds a release. The app is signed with
# the "MiniMacPad Dev" identity (scripts/create-signing-identity.sh), looked up in
# $SIGNING_KEYCHAIN if that is set.
set -euo pipefail
cd "$(dirname "$0")/.."

read -r version build <<< "$(scripts/version.sh "${MINIMACPAD_RELEASE_VERSION:-}")"
MINIMACPAD_VERSION="$version" cargo build --release

APP=target/MiniMacPad.app
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp target/release/minipad-ui target/release/minipad "$APP/Contents/MacOS/"
cp assets/MiniMacPad.icns "$APP/Contents/Resources/"
# CFBundleShortVersionString takes plain X.Y.Z only.
short="${version%%[-+]*}"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleName</key><string>MiniMacPad</string>
<key>CFBundleDisplayName</key><string>MiniMacPad</string>
<key>CFBundleIdentifier</key><string>dev.southstar21.minimacpad</string>
<key>CFBundleExecutable</key><string>minipad-ui</string>
<key>CFBundleIconFile</key><string>MiniMacPad</string>
<key>CFBundlePackageType</key><string>APPL</string>
<key>CFBundleShortVersionString</key><string>$short</string>
<key>CFBundleVersion</key><string>$build</string>
<key>LSMinimumSystemVersion</key><string>13.0</string>
<key>NSHighResolutionCapable</key><true/>
</dict></plist>
PLIST

# macOS ties privacy permissions (Input Monitoring, Accessibility) to the signature, and the
# updater only accepts builds signed like the running app. An ad-hoc signature changes with every
# build, so it is a last resort.
identity="MiniMacPad Dev"
keychain=(${SIGNING_KEYCHAIN:+--keychain "$SIGNING_KEYCHAIN"})
if security find-identity -p codesigning ${SIGNING_KEYCHAIN:+"$SIGNING_KEYCHAIN"} | grep -q "\"$identity\""; then
  # Inside out: the daemon is also copied out of the bundle to run as a login item.
  codesign --force --sign "$identity" ${keychain[@]+"${keychain[@]}"} \
    --identifier dev.southstar21.minimacpad.daemon "$APP/Contents/MacOS/minipad"
  codesign --force --sign "$identity" ${keychain[@]+"${keychain[@]}"} "$APP"
  echo "signed with \"$identity\""
else
  codesign --force --sign - "$APP"
  echo "ad-hoc signed (run scripts/create-signing-identity.sh): permissions reset on every build and the app cannot update itself"
fi
echo "built $APP ($version, build $build)"

if [ "${1:-}" != "--no-install" ]; then
  DEST=/Applications/MiniMacPad.app
  pkill -x minipad-ui 2>/dev/null || true
  rm -rf "$DEST"
  ditto "$APP" "$DEST"
  echo "installed $DEST"
fi
