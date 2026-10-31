#!/bin/sh
# Builds the static Linux binary and packs it into target/wallmon_<version>_amd64.deb.
# Works on macOS: the package is assembled with tar and ar, no dpkg needed.
#   packaging/deb.sh
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
target=x86_64-unknown-linux-musl
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -n 1)
name=wallmon_${version}_amd64
work=$root/target/deb
stage=$work/$name

cd "$root"
cargo zigbuild --release --target "$target"

rm -rf "$work"
mkdir -p "$stage/usr/bin" "$stage/usr/lib/systemd/system" "$stage/usr/share/doc/wallmon" "$work/control"
install -m 0755 "target/$target/release/wallmon" "$stage/usr/bin/wallmon"
# Packages install to /usr/bin; install.sh uses /usr/local/bin.
sed 's|/usr/local/bin/wallmon|/usr/bin/wallmon|' packaging/wallmon.service >"$stage/usr/lib/systemd/system/wallmon.service"
install -m 0644 LICENSE "$stage/usr/share/doc/wallmon/copyright"
install -m 0644 config.toml.example "$stage/usr/share/doc/wallmon/config.toml.example"
chmod 0644 "$stage/usr/lib/systemd/system/wallmon.service"

size=$(du -sk "$stage" | cut -f1)
cat >"$work/control/control" <<EOF
Package: wallmon
Version: $version
Architecture: amd64
Maintainer: Piotr Piwoński <github@webapps.biz.pl>
Installed-Size: $size
Depends: kmscon, fonts-dejavu-core
Section: admin
Priority: optional
Homepage: https://github.com/webapps-git/wallmon
Description: always-on server status board for a monitor in the server room
 wallmon shows CPU, memory, disks, network, processes, sensors, Docker,
 Apache, databases, UPS, systemd services, fail2ban and websites on one
 screen. It replaces the login prompt on tty1 with a kmscon console running
 wallmon. Ctrl+Alt+F2 switches to a normal login on tty2.
EOF

cat >"$work/control/postinst" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = configure ] && [ -d /run/systemd/system ]; then
    systemctl daemon-reload
    # Masking also stops logind from bringing getty back when someone switches to tty1.
    systemctl mask getty@tty1.service
    systemctl enable wallmon.service
    systemctl restart wallmon.service
fi
EOF

cat >"$work/control/prerm" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = remove ] && [ -d /run/systemd/system ]; then
    systemctl disable --now wallmon.service || true
fi
EOF

cat >"$work/control/postrm" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = remove ] || [ "$1" = purge ]; then
    if [ -d /run/systemd/system ]; then
        systemctl daemon-reload
        systemctl unmask getty@tty1.service
        systemctl start getty@tty1.service || true
    fi
fi
if [ "$1" = purge ]; then
    rm -rf /etc/wallmon
fi
EOF
chmod 0644 "$work/control/control"
chmod 0755 "$work/control/postinst" "$work/control/prerm" "$work/control/postrm"
(cd "$stage" && find usr -type f | sort | xargs md5 -r | sed 's/ /  /') >"$work/control/md5sums"
chmod 0644 "$work/control/md5sums"

# Root-owned and without macOS metadata, as dpkg expects.
tarball() {
    COPYFILE_DISABLE=1 tar --no-xattrs --no-mac-metadata --uid 0 --gid 0 --uname root --gname root \
        --format ustar -czf "$1" -C "$2" .
}
tarball "$work/control.tar.gz" "$work/control"
tarball "$work/data.tar.gz" "$stage"
printf '2.0\n' >"$work/debian-binary"
rm -f "target/$name.deb"
(cd "$work" && ar rc "$root/target/$name.deb" debian-binary control.tar.gz data.tar.gz)
echo "target/$name.deb"
