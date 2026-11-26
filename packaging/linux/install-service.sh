#!/bin/sh
# Installs bamf-client as a systemd user service that starts with the
# graphical session. Run from the repository root after building:
#
#   cargo build --release -p bamf-client
#   sh packaging/linux/install-service.sh
#
# Logs: journalctl --user -u bamf-client -f
set -eu

repo="$(cd "$(dirname "$0")/../.." && pwd)"
bin="$HOME/.local/bin"
conf="${XDG_CONFIG_HOME:-$HOME/.config}/bamf"
units="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user"

if [ ! -x "$repo/target/release/bamf-client" ]; then
    echo "Build first: cargo build --release -p bamf-client" >&2
    exit 1
fi

mkdir -p "$bin" "$conf" "$units"
# Replace the binary atomically so a running service is not disturbed.
cp "$repo/target/release/bamf-client" "$bin/bamf-client.new"
mv "$bin/bamf-client.new" "$bin/bamf-client"

if [ ! -f "$conf/bamf-client.toml" ]; then
    if [ -f "$repo/bamf-client.toml" ]; then
        cp "$repo/bamf-client.toml" "$conf/bamf-client.toml"
    else
        cp "$repo/config/bamf-client.example.toml" "$conf/bamf-client.toml"
        echo "Edit $conf/bamf-client.toml (server address and key), then:"
        echo "  systemctl --user restart bamf-client"
    fi
    chmod 600 "$conf/bamf-client.toml"
fi

cp "$repo/packaging/linux/bamf-client.service" "$units/"
systemctl --user daemon-reload
systemctl --user enable bamf-client.service
systemctl --user restart bamf-client.service
echo "bamf-client installed and started; it now starts with every login."
echo "Status: systemctl --user status bamf-client"
