#!/bin/sh
# Installs (or updates) the bamf GNOME Shell extension for the current user.
set -eu

uuid=bamf@razimbrao.github.io
src="$(dirname "$0")/$uuid"
dst="${XDG_DATA_HOME:-$HOME/.local/share}/gnome-shell/extensions/$uuid"

mkdir -p "$dst/icons"
cp "$src"/*.js "$src"/metadata.json "$dst"/
cp "$src"/icons/*.svg "$dst"/icons/
echo "Installed to $dst"

if gnome-extensions enable "$uuid" 2>/dev/null; then
    echo "Enabled. If the icon does not show up, log out and back in."
else
    echo "GNOME only sees new extensions after a new session on Wayland:"
    echo "log out and back in, then run: gnome-extensions enable $uuid"
fi
