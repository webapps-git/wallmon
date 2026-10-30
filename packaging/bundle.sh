#!/bin/sh
# Builds the static Linux binary and packs it with the service and scripts into
# target/wallmon.tar.gz. An optional argument adds a configuration file to the bundle.
#   packaging/bundle.sh /path/to/config.toml
set -eu

root=$(cd "$(dirname "$0")/.." && pwd)
target=x86_64-unknown-linux-musl
out=$root/target/bundle/wallmon

cd "$root"
cargo zigbuild --release --target "$target"

rm -rf "$out"
mkdir -p "$out"
cp "target/$target/release/wallmon" packaging/wallmon.service packaging/install.sh packaging/uninstall.sh "$out/"
if [ $# -gt 0 ]; then
    cp "$1" "$out/config.toml"
fi
# Root-owned and without macOS metadata, so tar on the server unpacks it cleanly.
COPYFILE_DISABLE=1 tar --no-xattrs --no-mac-metadata --uid 0 --gid 0 --uname root --gname root \
    -czf target/wallmon.tar.gz -C target/bundle wallmon
echo "target/wallmon.tar.gz"
