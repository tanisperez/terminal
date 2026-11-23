#!/usr/bin/env bash
# Build the Linux release asset: a tarball with the binary, the desktop entry, the icons and the
# install script, which installs it and, from the in-app updater, updates it.
#
# Usage: packaging/linux/package.sh <version> <path-to-tterm-binary> <out-dir> [arch]
#
# <version> is the bare version (0.1.0 or 0.1.0-rc.1, no leading "v").
# [arch] is x86_64 (default) or aarch64.
set -euo pipefail

if [[ $# -lt 3 || $# -gt 4 ]]; then
    echo "usage: $0 <version> <tterm-binary> <out-dir> [x86_64|aarch64]" >&2
    exit 2
fi

version=$1
binary=$2
out_dir=$3
arch=${4:-x86_64}

case "$arch" in
x86_64 | aarch64) ;;
*)
    echo "error: unsupported arch '$arch' (use x86_64 or aarch64)" >&2
    exit 2
    ;;
esac

if [[ ! -f $binary ]]; then
    echo "error: binary not found: $binary" >&2
    exit 1
fi

root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)
linux_dir=$root/packaging/linux
icons_dir=$root/packaging/icons
mkdir -p "$out_dir"
out_dir=$(cd "$out_dir" && pwd)
binary=$(cd "$(dirname "$binary")" && pwd)/$(basename "$binary")

app_id=codes.tanis.tterm
sizes=(16 24 32 48 64 128 256 512)

# Tarball with a /usr-like prefix layout.
name=tterm-$version-linux-$arch
stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT
top=$stage/$name
install -Dm755 "$binary" "$top/bin/tterm"
install -Dm644 "$linux_dir/$app_id.desktop" "$top/share/applications/$app_id.desktop"
install -Dm644 "$icons_dir/tterm.svg" \
    "$top/share/icons/hicolor/scalable/apps/$app_id.svg"
for size in "${sizes[@]}"; do
    install -Dm644 "$icons_dir/png/tterm-$size.png" \
        "$top/share/icons/hicolor/${size}x${size}/apps/$app_id.png"
done
install -m644 "$root/LICENSE" "$top/LICENSE"
install -m644 "$root/NOTICE" "$top/NOTICE"
install -m644 "$root/README.md" "$top/README.md"
# The in-app updater installs a downloaded tarball with the install script it ships.
install -m755 "$root/site/install.sh" "$top/install.sh"
tar --sort=name --owner=0 --group=0 --numeric-owner --mtime='UTC 2020-01-01' \
    -C "$stage" -czf "$out_dir/$name.tar.gz" "$name"

echo "Built in $out_dir:"
ls -1 "$out_dir"
