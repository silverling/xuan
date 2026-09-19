# Development

Run the commands below from the repository root. For installation and editing, see the [user guide](USAGE.md).

## Prerequisites

Requires Rust **1.88+**, a C toolchain, and a Linux desktop with working Vulkan drivers. Wayland and X11 are supported; Mesa software Vulkan can also run the editor. Native file dialogs use the desktop portal, so install the portal backend for your desktop if dialogs do not appear.

Typical Debian/Ubuntu prerequisites:

```sh
sudo apt install build-essential pkg-config libxkbcommon-dev libwayland-dev \
    libvulkan1 mesa-vulkan-drivers xdg-desktop-portal libheif-examples
```

`libheif-examples` supplies the optional `heif-convert` executable for HEIC/HEIF import. Nikon NEF/NRW import uses the bundled Rawler library and needs no external converter.

## Build and run

```sh
cargo run --release --locked
cargo run --release --locked -- --demo
cargo run --release --locked -- photograph.png composition.xuan
```

To install a local build:

```sh
cargo build --release --locked
scripts/install.sh                 # installs under ~/.local
scripts/install.sh /custom/prefix   # optional destination
```

The installer adds a desktop launcher, icons, and the `.xuan` file association. Add the installation prefix's `bin` directory to `PATH`.

## Packaging

```sh
scripts/package.sh                 # archive and SHA-256 checksum in dist/
```

Packaging also requires Python 3. A release archive includes `bin/xuan`, `scripts/install.sh`, and rebuildable sources, including the LGPL RAW decoder. It can run directly after extraction. See [third-party notices](../THIRD_PARTY.md) for rebuilding or relinking with a modified decoder.

Build on the oldest distribution you intend to support. The locally produced archive uses this workstation's glibc **2.43**; build from source on older distributions. The [CI workflow](../.github/workflows/linux.yml) builds on Ubuntu 24.04.

## Application icons

Application and desktop icons are generated from [`assets/Xuan.png`](../assets/Xuan.png). After changing the logo, run:

```sh
scripts/generate-icons.sh
```

This requires ImageMagick 7. The generated transparent PNGs cover sizes from 16 to 1024 pixels and are committed, so building, packaging, and installing do not require ImageMagick. Rebuild the application after regenerating the icons.

## Checks

```sh
scripts/check.sh          # formatting, Clippy, engine and UI tests
scripts/check.sh --gpu    # also compares wgpu output against the CPU reference
```

The GPU checks require a working graphics environment. CI also validates the desktop entry, builds the release archive, and runs native screenshot and clipboard checks under Xvfb. See [implementation and verification notes](PORTING.md) for the architecture and recorded results.

## Screenshots

Refresh the README screenshot from the current release build:

```sh
cargo run --release --locked -- --demo --screenshot docs/screenshots/editor.png
```

The screenshot helper captures the real native window and exits. Inspect the resulting image before committing it. To capture a panel:

```sh
cargo run --release --locked -- --demo --screenshot /tmp/levels.png --screenshot-panel levels
```

The helper also supports `hue`, `curves`, `export`, `new`, `brush`, `selection`, `gradient`, `shape`, and `text`. On a Wayland desktop with XWayland available, prefix the command with `env -u WAYLAND_DISPLAY` to capture the X11 path.

## Benchmarks

To benchmark large-image zoom and editing updates on a GPU:

```sh
cargo test --release --locked --bin xuan benchmark_large_image -- --ignored --nocapture --test-threads=1
```

These measure UI updates, tessellation, and compositor completion on a 3000×3000 image: 48 zoom steps and 24 pointer updates each for moving a layer, marquee, lasso, brush, and eraser, plus gesture release. The Levels benchmark also measures 24 pointer updates and 24 live preview changes with its adjustment-layer dialog open. Set `XUAN_ZOOM_BENCH_IMAGE` to use a local image instead of the generated image. Window presentation is not included.

The Motion Blur benchmarks measure live GPU preview updates and full-resolution Apply, including GPU readback, against CPU filtering at distances 15 and 200. Apply uses original layer pixels even when the preview texture is downsampled. See the [GPU processing audit](GPU_PROCESSING.md) for backend routing, CPU exceptions, and transfer-inclusive benchmarks.
