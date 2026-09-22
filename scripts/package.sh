#!/usr/bin/env bash
set -euo pipefail
umask 022

xuan_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$xuan_root"
xuan_format=${1:-archive}
case "$xuan_format" in
    archive|deb|rpm|all) ;;
    *) printf 'Usage: %s [archive|deb|rpm|all]\n' "$0" >&2; exit 1 ;;
esac
if [[ $# -gt 1 ]]; then
    printf 'Usage: %s [archive|deb|rpm|all]\n' "$0" >&2
    exit 1
fi
for xuan_tool in python3 strip; do
    command -v "$xuan_tool" >/dev/null || { printf 'Required tool: %s\n' "$xuan_tool" >&2; exit 1; }
done
if [[ "$xuan_format" != archive ]]; then
    python3 scripts/package-native.py --check-tools "$xuan_format"
fi
cargo build --release --locked
xuan_version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)
xuan_name="xuan-${xuan_version}-linux-$(uname -m)"
mkdir -p dist
xuan_temporary=$(mktemp -d "$xuan_root/dist/.package.XXXXXX")
trap 'rm -rf -- "$xuan_temporary"' EXIT
xuan_stage="$xuan_temporary/$xuan_name"
mkdir -p "$xuan_stage"/{bin,share,scripts}
install -m755 target/release/xuan "$xuan_stage/bin/xuan"
strip "$xuan_stage/bin/xuan"
cp -R assets/icons "$xuan_stage/share/"
install -Dm644 packaging/me.silverl.xuan.desktop "$xuan_stage/share/applications/me.silverl.xuan.desktop"
install -Dm644 packaging/me.silverl.xuan.xml "$xuan_stage/share/mime/packages/me.silverl.xuan.xml"
xuan_licenses="$xuan_stage/share/licenses/xuan"
install -Dm644 LICENSE "$xuan_licenses/LICENSE"
install -m644 licenses/rawler-LGPL-2.1.txt "$xuan_licenses/"
install -m644 assets/fonts/Inter-LICENSE.txt "$xuan_licenses/"
for xuan_license in LICENSE-MIT LICENSE-APACHE; do
    install -Dm644 "vendor/egui-winit/$xuan_license" "$xuan_licenses/egui-winit/$xuan_license"
done
python3 scripts/package-docs.py "$xuan_stage/share/doc/xuan" "$xuan_version"
install -m755 scripts/install.sh "$xuan_stage/scripts/"
chmod -R u=rwX,go=rX "$xuan_stage"

# Distribute matching rebuildable sources alongside every binary format.
python3 scripts/package-source.py
if [[ "$xuan_format" == archive || "$xuan_format" == all ]]; then
    tar -C "$xuan_temporary" -czf "dist/$xuan_name.tar.gz" "$xuan_name"
    chmod 644 "dist/$xuan_name.tar.gz"
    (cd dist && sha256sum "$xuan_name.tar.gz" > "$xuan_name.tar.gz.sha256")
    printf 'Created %s/dist/%s.tar.gz\n' "$xuan_root" "$xuan_name"
fi
if [[ "$xuan_format" != archive ]]; then
    python3 scripts/package-native.py "$xuan_format" "$xuan_stage" "$xuan_version" "$xuan_root/dist"
fi
