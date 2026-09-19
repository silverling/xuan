#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo test --locked --package egui-winit --lib clipboard_paste
if [[ ${1:-} == --gpu ]]; then
    cargo test --locked --lib gpu:: -- --ignored
    cargo test --locked --bin xuan motion_blur_gpu_preview -- --ignored
fi
