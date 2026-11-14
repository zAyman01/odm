#!/bin/bash
# Installs odm for the current user. No administrator password is needed.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
dest="$HOME/Applications/odm"
agent="$HOME/Library/LaunchAgents/io.github.zayman01.odm-daemon.plist"

echo "Installing odm to $dest"
mkdir -p "$dest"
cp "$here/files/odm" "$here/files/odm-daemon" "$here/files/odm-gui" "$dest/"
cp "$here/files/README.md" "$here/files/LICENSE-MIT" "$here/files/LICENSE-APACHE" "$dest/"
rm -rf "$dest/extension"
cp -R "$here/files/extension" "$dest/extension"
chmod +x "$dest/odm" "$dest/odm-daemon" "$dest/odm-gui"
# The files were downloaded, so macOS marks them; clearing the mark avoids a prompt per program.
xattr -dr com.apple.quarantine "$dest" 2>/dev/null || true

read -r -p "Start the odm download service when you sign in? [Y/n] " answer
if [[ "${answer:-Y}" =~ ^[Yy]$ ]]; then
  mkdir -p "$HOME/Library/LaunchAgents"
  cat > "$agent" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>io.github.zayman01.odm-daemon</string>
  <key>ProgramArguments</key><array><string>$dest/odm-daemon</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><true/>
</dict>
</plist>
PLIST
  launchctl unload "$agent" 2>/dev/null || true
  launchctl load "$agent"
  echo "The download service is running."
else
  echo "Start it yourself with: $dest/odm-daemon"
fi

if ! echo ":$PATH:" | grep -q ":$dest:"; then
  echo
  echo "To use the odm command in a terminal, add this line to ~/.zshrc:"
  echo "  export PATH=\"$dest:\$PATH\""
fi

echo
echo "To add the browser extension, follow the steps in:"
echo "  $dest/extension/INSTALL.txt"
open "$dest/extension/INSTALL.txt" || true
nohup "$dest/odm-gui" >/dev/null 2>&1 &
