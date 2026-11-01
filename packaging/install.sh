#!/bin/sh
# Installs or upgrades wallmon on tty1. Run as root from the unpacked bundle directory.
# kmscon is not installed here; see "Console" in the README.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
state=/var/lib/wallmon

say() { printf '\n== %s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

[ "$(id -u)" -eq 0 ] || die "run as root"
for f in wallmon wallmon.service uninstall.sh; do
    [ -f "$here/$f" ] || die "missing $here/$f"
done

say "Installing files"
install -m 0755 "$here/wallmon" /usr/local/sbin/wallmon
# Earlier versions installed the binary to /usr/local/bin.
rm -f /usr/local/bin/wallmon
install -m 0644 "$here/wallmon.service" /etc/systemd/system/wallmon.service
install -d -m 0755 "$state"
install -m 0755 "$here/uninstall.sh" "$state/uninstall.sh"
if [ -f /etc/wallmon/config.toml ]; then
    echo "keeping the existing /etc/wallmon/config.toml"
elif [ -f "$here/config.toml" ]; then
    install -d -m 0755 /etc/wallmon
    install -m 0644 "$here/config.toml" /etc/wallmon/config.toml
else
    echo "no config.toml: wallmon shows only what it finds by itself"
fi

say "Replacing the login prompt on tty1 with wallmon"
systemctl daemon-reload
# Masking also stops logind from bringing getty back when someone switches to tty1.
systemctl mask getty@tty1.service
systemctl enable wallmon.service
systemctl restart wallmon.service

say "Done"
systemctl --no-pager status wallmon.service | head -n 5 || true
echo
echo "Remove completely with: $state/uninstall.sh"
