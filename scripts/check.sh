#!/usr/bin/env bash
set -euo pipefail
cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.."
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
if [[ ${1:-} == --gpu ]]; then
    cargo test --locked --lib gpu::tests -- --ignored
fi
