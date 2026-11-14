#!/bin/bash
# Builds the macOS disk image.
# Usage: installer/macos/build-dmg.sh <version> <folder with odm binaries> <output .dmg>
# The extension must already be unpacked at dist/chrome and dist/firefox.
set -euo pipefail

version="$1"
stage="$2"
out="$3"
root="$(cd "$(dirname "$0")/../.." && pwd)"
dmg="$(mktemp -d)/odm-${version}"

mkdir -p "$dmg/files/extension"
cp "$stage/odm" "$stage/odm-daemon" "$stage/odm-gui" "$dmg/files/"
cp -R "$root/dist/chrome" "$root/dist/firefox" "$dmg/files/extension/"
cp "$root/installer/EXTENSION-INSTALL.txt" "$dmg/files/extension/INSTALL.txt"
cp "$root/README.md" "$root/LICENSE-MIT" "$root/LICENSE-APACHE" "$dmg/files/"
cp "$root/installer/macos/install.command" "$dmg/Install odm.command"
chmod +x "$dmg/Install odm.command"
cat > "$dmg/READ ME FIRST.txt" <<'TXT'
To install odm, right-click "Install odm.command", choose Open, and confirm.
macOS asks because odm is not signed with an Apple Developer ID.
It copies odm to ~/Applications/odm and can start the download service at login.
TXT

mkdir -p "$(dirname "$out")"
hdiutil create -volname "odm ${version}" -srcfolder "$dmg" -ov -format UDZO "$out"
