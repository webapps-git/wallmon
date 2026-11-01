#!/bin/sh
# Removes wallmon completely: the service with its drop-ins, the binary and the
# configuration. tty1 gets its login prompt back. kmscon, if installed, stays.
set -eu

say() { printf '\n== %s\n' "$*"; }

[ "$(id -u)" -eq 0 ] || { echo "error: run as root" >&2; exit 1; }

say "Giving tty1 back to the login prompt"
systemctl disable --now wallmon.service 2>/dev/null || true
rm -f /etc/systemd/system/wallmon.service
rm -rf /etc/systemd/system/wallmon.service.d
systemctl daemon-reload
systemctl unmask getty@tty1.service
systemctl start getty@tty1.service || true

say "Removing files"
rm -f /usr/local/sbin/wallmon
rm -rf /etc/wallmon
rm -rf /var/lib/wallmon

say "wallmon removed"
