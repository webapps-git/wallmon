#!/bin/sh
# Installs or upgrades wallmon on tty1. Run as root from the unpacked bundle directory.
# Everything it adds to the system is recorded in /var/lib/wallmon, so uninstall.sh can
# remove exactly that and nothing more.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
state=/var/lib/wallmon
backports=/etc/apt/sources.list.d/wallmon-trixie-backports.sources

say() { printf '\n== %s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

[ "$(id -u)" -eq 0 ] || die "run as root"
for f in wallmon wallmon.service uninstall.sh; do
    [ -f "$here/$f" ] || die "missing $here/$f"
done
mkdir -p "$state"

if ! command -v kmscon >/dev/null 2>&1; then
    # shellcheck disable=SC1091
    . /etc/os-release
    [ "${VERSION_CODENAME:-}" = trixie ] || die "kmscon is not installed; install it first (on Debian 13 it is in trixie-backports)"
    if ! grep -rqs trixie-backports /etc/apt/sources.list /etc/apt/sources.list.d/; then
        say "Adding trixie-backports to apt sources"
        cat >"$backports" <<'EOF'
# Added by wallmon install.sh for kmscon; removed by uninstall.sh.
Types: deb
URIs: http://deb.debian.org/debian
Suites: trixie-backports
Components: main
Signed-By: /usr/share/keyrings/debian-archive-keyring.gpg
EOF
        echo backports >>"$state/added"
    fi
    say "Installing kmscon from trixie-backports"
    apt-get update
    dpkg-query -W -f '${Package}\n' | sort >"$state/before"
    apt-get install -y -t trixie-backports kmscon fonts-dejavu-core
    dpkg-query -W -f '${Package}\n' | sort | comm -13 "$state/before" - | sed 's/^/package /' >>"$state/added"
    rm -f "$state/before"
fi

say "Installing files"
install -m 0755 "$here/wallmon" /usr/local/bin/wallmon
install -m 0644 "$here/wallmon.service" /etc/systemd/system/wallmon.service
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
