#!/usr/bin/env bash
set -euo pipefail

xuan_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
cd "$xuan_root"
cargo build --release --locked
xuan_version=$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)
xuan_name="xuan-${xuan_version}-linux-$(uname -m)"
mkdir -p dist
xuan_temporary=$(mktemp -d "$xuan_root/dist/.package.XXXXXX")
trap 'rm -rf -- "$xuan_temporary"' EXIT
xuan_stage="$xuan_temporary/$xuan_name"
mkdir -p "$xuan_stage"/{bin,assets,packaging,scripts,docs}
install -m755 target/release/xuan "$xuan_stage/bin/xuan"
strip "$xuan_stage/bin/xuan"
install -m644 LICENSE README.md "$xuan_stage/"
install -m644 THIRD_PARTY.md "$xuan_stage/"
mkdir -p "$xuan_stage/licenses"
install -m644 licenses/* "$xuan_stage/licenses/"
cp -R assets/icons "$xuan_stage/assets/"
install -Dm644 assets/fonts/Inter-LICENSE.txt "$xuan_stage/assets/fonts/Inter-LICENSE.txt"
mkdir -p "$xuan_stage/vendor/egui-winit"
install -m644 vendor/egui-winit/{LICENSE-MIT,LICENSE-APACHE,PATCH.md} "$xuan_stage/vendor/egui-winit/"
install -m644 packaging/* "$xuan_stage/packaging/"
install -m644 docs/*.md "$xuan_stage/docs/"
cp -R docs/screenshots "$xuan_stage/docs/"
install -m755 scripts/install.sh "$xuan_stage/scripts/"
# Include the application and exact LGPL decoder sources so recipients can
# rebuild/relink the executable after modifying Rawler.
mkdir -p "$xuan_stage/source"
cp -R src assets vendor licenses "$xuan_stage/source/"
install -Dm755 scripts/generate-icons.sh "$xuan_stage/source/scripts/generate-icons.sh"
install -m644 Cargo.lock LICENSE README.md THIRD_PARTY.md "$xuan_stage/source/"
xuan_host=$(rustc -vV | sed -n 's/^host: //p')
xuan_rawler_manifest=$(cargo metadata --locked --format-version 1 --filter-platform "$xuan_host" |
    python3 -c 'import json, sys; print(next(p["manifest_path"] for p in json.load(sys.stdin)["packages"] if p["name"] == "rawler"))')
cp -R "$(dirname -- "$xuan_rawler_manifest")" "$xuan_stage/source/vendor/rawler"
sed '/^\[patch.crates-io\]$/a rawler = { path = "vendor/rawler" }' Cargo.toml > "$xuan_stage/source/Cargo.toml"
tar -C "$xuan_temporary" -czf "dist/$xuan_name.tar.gz" "$xuan_name"
(cd dist && sha256sum "$xuan_name.tar.gz" > "$xuan_name.tar.gz.sha256")
printf 'Created %s/dist/%s.tar.gz\n' "$xuan_root" "$xuan_name"
