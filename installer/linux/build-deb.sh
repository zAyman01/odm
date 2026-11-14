#!/bin/bash
# Builds the Debian package.
# Usage: installer/linux/build-deb.sh <version> <folder with odm binaries> <output .deb>
# The extension must already be unpacked at dist/chrome and dist/firefox.
set -euo pipefail

version="$1"
stage="$2"
out="$3"
root="$(cd "$(dirname "$0")/../.." && pwd)"
pkg="$(mktemp -d)/odm"

install -d "$pkg/DEBIAN" "$pkg/usr/bin" "$pkg/usr/share/odm/extension" \
  "$pkg/usr/share/applications" "$pkg/usr/lib/systemd/user" "$pkg/usr/share/doc/odm"
install -m 0755 "$stage/odm" "$stage/odm-daemon" "$stage/odm-gui" "$pkg/usr/bin/"
cp -r "$root/dist/chrome" "$root/dist/firefox" "$pkg/usr/share/odm/extension/"
install -m 0644 "$root/installer/EXTENSION-INSTALL.txt" "$pkg/usr/share/odm/extension/INSTALL.txt"
install -m 0644 "$root/README.md" "$root/LICENSE-MIT" "$root/LICENSE-APACHE" "$pkg/usr/share/doc/odm/"

cat > "$pkg/usr/share/applications/odm.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=odm
Comment=Open Download Manager
Exec=odm-gui
Terminal=false
Categories=Network;FileTransfer;
DESKTOP

cat > "$pkg/usr/lib/systemd/user/odm-daemon.service" <<UNIT
[Unit]
Description=odm download service

[Service]
ExecStart=/usr/bin/odm-daemon
Restart=on-failure

[Install]
WantedBy=default.target
UNIT

cat > "$pkg/DEBIAN/control" <<CONTROL
Package: odm
Version: ${version}
Section: net
Priority: optional
Architecture: amd64
Maintainer: zAyman01 <183708298+zAyman01@users.noreply.github.com>
Homepage: https://github.com/zAyman01/odm
Depends: libc6
Recommends: ffmpeg, yt-dlp, libgl1, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libxcursor1, libxi6, libxrandr2
Description: Open Download Manager
 A fast download manager: segmented HTTP, BitTorrent, HLS and DASH video,
 a desktop app, a command-line client and a browser extension.
CONTROL

cat > "$pkg/DEBIAN/postinst" <<'POSTINST'
#!/bin/sh
set -e
echo "odm installed."
echo "Start the download service for your user with:"
echo "  systemctl --user enable --now odm-daemon"
echo "Then open odm from your applications menu, or run odm-gui."
echo "To add the browser extension, read /usr/share/odm/extension/INSTALL.txt"
POSTINST
chmod 0755 "$pkg/DEBIAN/postinst"

mkdir -p "$(dirname "$out")"
dpkg-deb -Zxz --build --root-owner-group "$pkg" "$out"
