#!/bin/sh
# Installs the tarball's contents for the current user: the binary under ~/.local/bin, the
# desktop entry and the icon where the desktop looks for them. No root, no system paths; to
# remove, delete the three files. See spec/packaging.md.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
bin="$HOME/.local/bin"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
mkdir -p "$bin" "$data/applications" "$data/icons/hicolor/512x512/apps"
install -m 755 "$here/rdm" "$bin/rdm"
install -m 644 "$here/rdm.desktop" "$data/applications/rdm.desktop"
install -m 644 "$here/rdm.png" "$data/icons/hicolor/512x512/apps/rdm.png"
command -v update-desktop-database >/dev/null 2>&1 && update-desktop-database "$data/applications" || true
command -v gtk-update-icon-cache >/dev/null 2>&1 && gtk-update-icon-cache -q "$data/icons/hicolor" || true
echo "installed rdm to $bin; make sure $bin is on your PATH"
