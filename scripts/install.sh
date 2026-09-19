#!/usr/bin/env bash
set -euo pipefail

xuan_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
xuan_prefix=${1:-"$HOME/.local"}
xuan_binary="$xuan_root/bin/xuan"
if [[ ! -f "$xuan_binary" ]]; then
    xuan_binary="$xuan_root/target/release/xuan"
fi
if [[ ! -f "$xuan_binary" ]]; then
    printf 'Build first with cargo build --release, or extract a release archive.\n' >&2
    exit 1
fi
install -Dm755 "$xuan_binary" "$xuan_prefix/bin/xuan"
install -Dm644 "$xuan_root/assets/org.xuan.Editor.svg" "$xuan_prefix/share/icons/hicolor/scalable/apps/org.xuan.Editor.svg"
install -Dm644 "$xuan_root/packaging/org.xuan.Editor.desktop" "$xuan_prefix/share/applications/org.xuan.Editor.desktop"
install -Dm644 "$xuan_root/packaging/org.xuan.Editor.xml" "$xuan_prefix/share/mime/packages/org.xuan.Editor.xml"
install -Dm644 "$xuan_root/LICENSE" "$xuan_prefix/share/licenses/xuan/LICENSE"
install -Dm644 "$xuan_root/THIRD_PARTY.md" "$xuan_prefix/share/licenses/xuan/THIRD_PARTY.md"
install -Dm644 "$xuan_root/licenses/rawler-LGPL-2.1.txt" "$xuan_prefix/share/licenses/xuan/rawler-LGPL-2.1.txt"
install -Dm644 "$xuan_root/assets/fonts/Inter-LICENSE.txt" "$xuan_prefix/share/licenses/xuan/Inter-LICENSE.txt"
for xuan_license in LICENSE-MIT LICENSE-APACHE; do
    install -Dm644 "$xuan_root/vendor/egui-winit/$xuan_license" "$xuan_prefix/share/licenses/xuan/egui-winit/$xuan_license"
done
if command -v update-desktop-database >/dev/null; then
    update-desktop-database "$xuan_prefix/share/applications"
fi
if command -v update-mime-database >/dev/null; then
    update-mime-database "$xuan_prefix/share/mime"
fi
printf 'Installed xuan to %s. Ensure %s/bin is in PATH.\n' "$xuan_prefix" "$xuan_prefix"
