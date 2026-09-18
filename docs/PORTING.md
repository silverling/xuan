# Compositor →  Xuan

The Linux port is implemented in Rust with egui 0.33 and wgpu 27. The source baseline is Compositor commit `a19db9011282399785dc18efcfded904627bdcc2`, inspected at `/home/silver/workspace/repo/Compositor`. That source writes `.comp` version 7. It was left unchanged.

## Architecture

| Module | Responsibility |
| --- | --- |
| `document`, `geometry`, `history` | Validated layered document, affine/projective transforms, shared pixel assets, bounded snapshot undo/redo |
| `blend`, `render`, `gpu`, `composite.wgsl` | CPU reference/export compositor and wgpu compute preview, thirteen blend modes, inherited masks and clipping, premultiplied Lanczos downsampling |
| `selection`, `paint`, `retouch` | Selection coverage, swept brush strokes, clone/blur/smudge, gradients, live shapes, texture-based healing/fill, border-color background masks |
| `color`, `effects` | Selective hue ranges, RGB channel levels/curves, linear-light exposure, gradient mapping, film grain, raster filters |
| `operations`, `io` | Group/layer commands, canvas sizing/crop, cross-project copies, clipboard rasters, atomic project storage, Compositor import, image export |
| `app` | Native egui shell, menus, contextual controls, tabs, canvas gestures, layer panel, live dialogs, cancellable editing workers |
| `packaging`, `scripts`, `.github/workflows` | Desktop and MIME integration, install/archive/check scripts, Linux CI |

wgpu dispatches an 8×8 compute shader per visible layer, ping-ponging through floating-point targets and registering the final texture with egui's renderer. Mask/clip coverage is prepared on the CPU; pixel compositing and adjustments run on the GPU. CPU export uses full document dimensions, and remains the preview fallback when compute or source texture limits prevent GPU composition. Preview source textures are cached by shared asset identity and scale.

## UI preservation

The port uses the source's neutral 0.14-gray panels, darker canvas, 56 px tool rail, 36 px rounded tool buttons, 42 px contextual header, 252 px Layers panel (202–352 px resize range), 30 px status bar, restrained 11–13 px type, subtle dividers, and compact rounded controls. Icons are drawn as vectors. A new layered Xuan icon replaces the application identity. Native Linux window decorations and portal dialogs follow the desktop environment.

The demo composition is generated locally and contains five editable layers; it is not a screenshot baked into the UI. See [editor](screenshots/editor.png), [Levels](screenshots/levels.png), and [JPEG export](screenshots/export.png) captures from the actual Linux release build.

## Verification completed

Verified locally on **2026-09-19**, Linux x86_64, Rust 1.98.0, glibc 2.43:

- `cargo fmt --all -- --check` and `cargo clippy --all-targets -- -D warnings` pass.
- `cargo test --all-targets`: **37 tests pass**, with one hardware-dependent GPU test ignored in this default run.
- `cargo test --lib gpu::tests -- --ignored`: **1 GPU test passes**, comparing all blend modes, color/channel adjustments, film grain, masks, and perspective transforms against CPU output within two premultiplied 8-bit units.
- Tests cover project round trips and atomic overwrite, original Swift dictionaries/transforms, unsafe asset paths, hierarchy and clipping validation, PNG/JPEG/TIFF/WebP export, PNG DPI, selection connectivity and coverage, copy-on-write pixels, expanded paint/blur bounds, live shape redraw, group transforms, linked mask placement, and history revisions.
- egui input tests exercise brush/selection/pixel-move gestures, scale/distort handles, independent tabs, layer copying between projects, adjustment cancellation, export preview, and worker commit/cancellation.
- Release binary builds and starts on **Wayland** (DISPLAY unset) and **X11** (WAYLAND_DISPLAY unset). Real native screenshots were inspected for editor, welcome, Levels, Hue/Saturation, and export layouts.
- HEIC import was verified using libheif's upstream example file; it decoded to a 1280×854 image in the editor. This external image is not bundled in the repository or archive.
- Desktop entry validation, MIME/icon XML parsing, shell syntax checks, and CI YAML parsing pass. Release packaging and checksum verification succeed; both the extracted binary and an installation to a temporary prefix run successfully.

The GitHub workflow is supplied for future repository runs; it has not been dispatched remotely. The local validation above was run directly.

## Explicit differences and limits

- The macOS foreground-recognition service is replaced by editable edge-color masks. It suits simple backgrounds and is not semantic subject segmentation.
- Content-aware fill and spot healing use portable patch matching. Results and performance will differ from the original implementation.
- The raster pipeline is 8-bit sRGB. Embedded ICC color profiles are not converted or preserved. Animated input formats import a single frame.
- Original `.comp` packages are imported, not overwritten or exported. Save uses the portable `.xuan` format; selections and history remain session-only.
- Perspective transforms retain source pixels in Xuan. Source Compositor often rasterizes such edits. Round trips through native `.xuan` retain the projective metadata.
- Preview resolution is capped at 4096 px per side, or 1600 px in the CPU fallback. Full-resolution export is independent of that cap. Imports, saves, and some live raster filters are synchronous; the expensive retouching jobs are cancellable background operations.
- Vulkan is tested on both window systems. Explicit OpenGL startup on this workstation returned an incompatible-surface adapter error; availability depends on the EGL/driver configuration.
- The local release archive requires glibc 2.43. For older distributions, build from source on the target distribution. The CI definition uses Ubuntu 24.04 as its packaging baseline.

Completed features are recorded as conventional Git commits. No source repository files, system installation directories, or remote repositories were modified by this port.
