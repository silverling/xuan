#!/usr/bin/env bash
set -euo pipefail

xuan_root=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)
if ! command -v magick >/dev/null; then
    printf 'Generating icons requires ImageMagick 7 (magick).\n' >&2
    exit 1
fi

for xuan_size in 16 24 32 48 64 96 128 256 512 1024; do
    xuan_directory="$xuan_root/assets/icons/hicolor/${xuan_size}x${xuan_size}/apps"
    mkdir -p "$xuan_directory"
    magick "$xuan_root/assets/Xuan.png" \
        -colorspace sRGB -filter Lanczos -resize "${xuan_size}x${xuan_size}" \
        -background none -gravity center -extent "${xuan_size}x${xuan_size}" \
        -strip -depth 8 -define png:color-type=6 \
        "$xuan_directory/me.silverl.xuan.png"
done

printf 'Generated desktop icons from assets/Xuan.png.\n'
