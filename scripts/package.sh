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
install -m644 assets/org.xuan.Editor.svg "$xuan_stage/assets/"
install -Dm644 assets/fonts/Inter-LICENSE.txt "$xuan_stage/assets/fonts/Inter-LICENSE.txt"
install -m644 packaging/* "$xuan_stage/packaging/"
install -m644 docs/*.md "$xuan_stage/docs/"
cp -R docs/screenshots "$xuan_stage/docs/"
install -m755 scripts/install.sh "$xuan_stage/scripts/"
tar -C "$xuan_temporary" -czf "dist/$xuan_name.tar.gz" "$xuan_name"
(cd dist && sha256sum "$xuan_name.tar.gz" > "$xuan_name.tar.gz.sha256")
printf 'Created %s/dist/%s.tar.gz\n' "$xuan_root" "$xuan_name"
