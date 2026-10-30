#!/bin/sh
# Removes wallmon completely: the service, the binary, the configuration, and whatever
# install.sh added (kmscon with its dependencies, the backports source). tty1 gets its
# login prompt back.
set -eu

state=/var/lib/wallmon
backports=/etc/apt/sources.list.d/wallmon-trixie-backports.sources

say() { printf '\n== %s\n' "$*"; }

[ "$(id -u)" -eq 0 ] || { echo "error: run as root" >&2; exit 1; }

say "Giving tty1 back to the login prompt"
systemctl disable --now wallmon.service 2>/dev/null || true
rm -f /etc/systemd/system/wallmon.service
systemctl daemon-reload
systemctl unmask getty@tty1.service
systemctl start getty@tty1.service || true

say "Removing files"
rm -f /usr/local/bin/wallmon
rm -rf /etc/wallmon

if [ -f "$state/added" ]; then
    if grep -qx 'package kmscon' "$state/added"; then
        say "Removing kmscon"
        apt-get purge -y kmscon
        # Of the packages that came in with kmscon, only those nothing else needs now.
        apt-get -s autoremove | sed -n 's/^Remv \([^ :]*\).*/\1/p' | sort >"$state/unused"
        extra=$(sed -n 's/^package //p' "$state/added" | sort | comm -12 - "$state/unused" | tr '\n' ' ')
        if [ -n "$extra" ]; then
            say "Removing packages that came with kmscon: $extra"
            # shellcheck disable=SC2086
            apt-get purge -y $extra
        fi
    fi
    if grep -qx backports "$state/added"; then
        say "Removing trixie-backports from apt sources"
        rm -f "$backports"
        apt-get update
    fi
fi
rm -rf "$state"

say "wallmon removed"
