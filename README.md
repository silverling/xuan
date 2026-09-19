# Xuan

A native Linux image editor, ported from Compositor and rewritten in Rust with **egui** and **wgpu**. It retains the original charcoal theme, compact contextual controls, vertical tool rail, tabbed workspace, and Layers panel. Custom egui controls reproduce Compositor’s AppKit-style capsules, fields, sliders, segmented pickers, and floating panels. The menu bar shares a client-side titlebar with window controls; drag to move, double-click to maximize, or drag an edge to resize.

![xuan editing a layered composition](docs/screenshots/editor.png)

## Run

```sh
cargo run --release --locked
cargo run --release --locked -- --demo
cargo run --release --locked -- photograph.png composition.xuan
```

Requires Rust **1.88+**, a C toolchain, and a Linux desktop with working Vulkan drivers. Wayland and X11 are supported. Mesa software Vulkan can also run the editor. Native file dialogs use the desktop portal; install the portal backend for your desktop if dialogs do not appear.

Typical Debian/Ubuntu prerequisites:

```sh
sudo apt install build-essential pkg-config libxkbcommon-dev libwayland-dev \
    libvulkan1 mesa-vulkan-drivers xdg-desktop-portal libheif-examples
```

HEIC/HEIF import uses the optional `heif-convert` executable from `libheif-examples`. Nikon NEF/NRW import uses the bundled Rawler library and needs no external converter. Other image formats are decoded directly in Rust.

## Editing

- **Layers:** folders, 13 blend modes, opacity, visibility, locks, drag reordering/nesting, duplication, merge, clipping masks, linked or independent raster masks, and copying layers to another project tab.
- **Transforms:** move, scale, rotate, flip, free perspective distortion, numeric controls, shared transforms for several layers or folders, and snapping to edges and centers. Original source pixels remain available during transforms. Move / Transform selects and drags anywhere inside a layer's bounds by default; check **Ignore Transparent Pixels** to select only at visible pixels.
- **Selections:** rectangle, ellipse, freehand/polygonal lasso, contiguous/global magic wand, add/subtract/intersect, inverse, feather, outline movement, and moving or duplicating selected pixels.
- **Paint:** brush, eraser, aligned/unaligned clone stamp with layer/all-layer sampling, spot healing, blur/smudge, gradients, rectangles, rounded rectangles, ellipses, and eyedropper. Live shapes redraw at the new size until their pixels are edited.
- **Text:** editable multiline text layers, searchable installed font families, size and color, bold, italic, underline, and strikethrough. Click with the Text tool (T) to place or edit text; double-click a text layer to reopen its live preview. Move and transform text with the Move tool.
- **Adjustments:** editable Hue/Saturation color ranges, per-channel Levels and Curves, Exposure, Gradient Map, Grain, and Invert. Apply directly or add an adjustment layer, with live preview and selection coverage.
- **Filters:** Gaussian and Motion Blur with expanded bounds, Add Noise, Lens Correction, content-aware fill, and edge-color background removal. Healing, filling, and background removal run in cancellable workers.
- **RAW Develop:** NEF/NRW opens in a dedicated Develop workspace. Adjust white balance, exposure, tone curves, HSL, monochrome and split toning, noise reduction, sharpening, manual lens correction, crop, and brush/gradient masks. Compare before/after and inspect clipping or full-resolution detail. Develop creates an embedded RAW layer; double-click it to edit the original RAW again. Save `.xuan` to retain the source and adjustments, or export a 16-bit sRGB TIFF directly from Develop. See [RAW workflow and limits](docs/RAW.md).
- **Documents:** independent tab histories, crop, canvas/image size, high-quality downsampling, pixel grid, pasting copied images or image files as layers, Copy Merged, and save-on-close prompts. Undo retains up to 64 steps with a 512 MiB asset budget, keeping at least one step.

Use **File → Open Compositor Project…** to import an original `.comp` folder package. Save it as `.xuan` to keep editing on Linux. Image export supports PNG, JPEG, TIFF, and WebP; JPEG has a quality preview and PNG/JPEG carry print resolution.

See [keyboard shortcuts](docs/SHORTCUTS.md), [project format](docs/FORMAT.md), and [implementation and verification notes](docs/PORTING.md).

## Install and package

```sh
cargo build --release --locked
scripts/install.sh                 # installs under ~/.local
scripts/install.sh /custom/prefix   # optional destination
scripts/package.sh                 # archive and SHA-256 checksum in dist/
```

A release archive includes `bin/xuan`, `scripts/install.sh`, and rebuildable sources (including the LGPL RAW decoder); it can run directly after extraction. Packaging also requires Python 3. The installer adds a desktop launcher, icon, and `.xuan` file association. Add the installation prefix's `bin` directory to `PATH`.

Build on the oldest distribution you intend to support. The locally produced archive uses this workstation's glibc **2.43**; build from source on older distributions. The included CI workflow builds on Ubuntu 24.04.

## Verification

```sh
scripts/check.sh          # formatting, Clippy, engine and UI tests
scripts/check.sh --gpu    # also compares wgpu output against the CPU reference
cargo run --release -- --demo --screenshot /tmp/xuan.png
cargo run --release -- --demo --screenshot /tmp/levels.png --screenshot-panel levels
```

The screenshot helper also supports `hue`, `curves`, `export`, `new`, `brush`, `selection`, `gradient`, `shape`, and `text`. It captures the real native window and exits.

To benchmark large-image zoom and editing updates on a GPU, run:

```sh
cargo test --release --locked --bin xuan benchmark_large_image -- --ignored --nocapture --test-threads=1
```

These measure UI updates, tessellation, and compositor completion on a 3000×3000 image: 48 zoom steps and 24 pointer updates each for moving a layer, marquee, lasso, brush, and eraser, plus gesture release. The Levels benchmark also measures 24 pointer updates and 24 live preview changes with its adjustment-layer dialog open. Set `XUAN_ZOOM_BENCH_IMAGE` to use a local image instead of the generated image. Window presentation is not included.

## Port differences

The photo editor uses an 8-bit sRGB raster pipeline. RAW Develop uses floating-point camera data and offers direct 16-bit TIFF output with an sRGB profile; its photo-layer render uses the existing 8-bit pipeline. Imported raster ICC profiles are not converted or preserved. `.comp` versions 1–7 can be imported; Xuan does not write the original macOS format. Selections and undo history are session state and are not saved in project archives.

Remove Background uses a border-color matte, intended for simple backgrounds, instead of Apple's foreground-recognition service. Content-aware fill and healing use a portable texture-matching implementation, so results differ from Compositor. The canvas preview is capped at 4096 pixels per side; export uses full document dimensions. Large synchronous filters, imports, and saves can temporarily occupy the UI thread. Vulkan is the verified rendering path; OpenGL surface availability depends on the driver.

## License

Xuan's source is MIT licensed; the RAW decoder is LGPL 2.1. Release packages include its source and rebuild instructions; see [third-party notices](THIRD_PARTY.md). Original Compositor copyright © 2026 Wonder Assembly LLC. The original license is retained in [LICENSE](LICENSE). The bundled [Inter font](assets/fonts/Inter-LICENSE.txt) is licensed under the SIL Open Font License. The Rust port has no runtime dependency on Swift, AppKit, Core Graphics, Metal, or macOS.
