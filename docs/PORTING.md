# Compositor →  Xuan

The Linux port is implemented in Rust with egui 0.33 and wgpu 27. The source baseline is Compositor commit `a19db9011282399785dc18efcfded904627bdcc2`, inspected at `/home/silver/workspace/repo/Compositor`. That source writes `.comp` version 7. It was left unchanged.

## Architecture

| Module | Responsibility |
| --- | --- |
| `document`, `geometry`, `history` | Validated layered document, affine/projective transforms, shared pixel assets, bounded snapshot undo/redo |
| `blend`, `render`, `gpu`, `composite.wgsl`, `mipmap.wgsl` | CPU reference/fallback compositor and wgpu compute preview/export, thirteen blend modes, inherited masks and clipping, premultiplied Lanczos downsampling and preview mipmaps |
| `selection`, `paint`, `retouch` | Selection coverage, swept brush strokes, clone/blur/smudge, gradients, live shapes, texture-based healing/fill, border-color background masks |
| `color`, `effects` | Selective hue ranges, RGB channel levels/curves, linear-light exposure, gradient mapping, film grain, raster filters |
| `operations`, `io` | Group/layer commands, canvas sizing/crop, cross-project copies, clipboard rasters, atomic project storage, Compositor import, image export |
| `app` | Native egui shell, menus, contextual controls, tabs, canvas gestures, layer panel, live dialogs, cancellable editing workers |
| `packaging`, `scripts`, `.github/workflows` | Desktop and MIME integration, install/archive/check scripts, Linux CI |

wgpu dispatches an 8×8 compute shader per visible layer, ping-ponging through floating-point targets and registering the final texture with egui's renderer. Substantial mask/clip coverage, pixel compositing, and adjustments run on the GPU. Export and merge use full-resolution GPU composition with straight-alpha readback; CPU rendering remains the fallback when compute or resource limits prevent it. Preview source textures are cached by shared asset identity and scale.

The shared processing backend also accelerates every raster filter and adjustment, resampling, large brush/fill/gradient work, histograms, and the floating-point RAW pipeline. See [GPU_PROCESSING.md](GPU_PROCESSING.md) for the complete module audit, CPU exceptions, output precision, and reproducible benchmarks.

The composition uses a fixed, capped resolution and stays cached during zooming and panning, along with layer thumbnails. The GPU builds a premultiplied mipmap pyramid when the composition changes. The canvas blends between these levels when zoomed out and uses nearest filtering when magnified, avoiding CPU image resizing and texture allocation during zoom gestures. The CPU fallback also reuses its cached composition during zooming.

Motion Blur previews sample cached layer textures directly in the compositor. Apply uses a separate full-resolution compute pass, reusing the original GPU texture when available or uploading original pixels in the worker. It reads straight RGBA back once, then uses shared selection blending, expanded transform, mask placement, and history handling. Large selection blends also use compute. Preview scaling never limits the applied image. GPU texture/buffer limits and processing failures use the cancellable CPU fallback. `scripts/check.sh --gpu` checks pixel accuracy, source resolution, selection/mask preservation, fallback, and the native Apply/Cancel/undo workflow.

Selection gestures update the overlay without recompositing image pixels. Lasso masks use scanline filling instead of checking every edge at every image pixel; marquee and selection translation operate on row spans. Layer thumbnails track source asset identity and placement independently, so edits only refresh affected thumbnails. They sample at thumbnail resolution to keep their cost independent of source image size; canvas and export quality filtering remain separate.

Clipboard shortcuts use egui's native Copy/Cut/Paste events and leave focused text fields in control of text editing. A small [egui-winit patch](../vendor/egui-winit/PATCH.md) preserves paste events for image-only clipboards; upstream 0.33.3 otherwise drops the keypress when no text is available.

External paste reads native image pixels or file lists through arboard, with Wayland data-control support and an X11 fallback. Text representations of local file URLs and paths are also accepted. File imports preserve names and form one undo step; unsupported clipboard contents never reuse an older cached image. The clipboard connection remains alive to serve copied images without requiring a clipboard manager. CI runs the separate-process clipboard regression under Xvfb.

The external clipboard regression was also verified locally with isolated Xephyr (X11) and KWin virtual (Wayland) sessions, transferring both pixels and file lists from a separate process and checking that later text copies do not paste stale images.

## UI preservation

The port uses the source's neutral 0.14-gray panels, darker canvas, 56 px tool rail, 36 px rounded tool buttons, 42 px contextual header, 252 px Layers panel (202–352 px resize range), 30 px status bar, restrained 11–13 px type, subtle dividers, and compact rounded controls. Icons are drawn as vectors. A new layered Xuan icon replaces the application identity. A client-side titlebar integrates the menu bar and traffic-light window controls; it supports window-manager dragging, all eight resize directions, minimize, maximize/restore, and the existing save-on-close flow. Handing off a move or resize clears egui's pointer and drag state because Wayland compositors can consume the release event; subsequent drags work without an extra click. File dialogs continue to use the desktop portal.

The shared `app/widgets.rs` controls paint capsule bezels, inset numeric fields, square checkboxes, circular slider thumbs, segmented pickers, paired pop-up chevrons, checked menu items, capsule project tabs, and overlapping palette swatches. Floating panels have a centered utility titlebar and 24 px content insets, retain their dragged positions, and scroll on short windows. Layer rows use the source’s 52 px height, 13 px names, 10 px dimensions, canvas-relative thumbnails, and hairline dividers. Levels uses a 150 px histogram, draggable input/output handles, and grouped numeric fields. Inter supplies portable typography; Apple’s proprietary system font and OS materials are approximated.

The demo composition is generated locally and contains five editable layers; it is not a screenshot baked into the UI. See [editor](screenshots/editor.png), [Levels](screenshots/levels.png), and [JPEG export](screenshots/export.png) captures from the actual Linux release build.

## Verification completed

Verified locally on **2026-09-19**, Linux x86_64, Rust 1.98.0, glibc 2.43:

- `cargo fmt --all -- --check` and `cargo clippy --all-targets -- -D warnings` pass.
- `cargo test --locked --all-targets`: **112 tests pass** (52 library and 60 application tests); native GPU, interaction benchmark, sample-RAW, and system-clipboard checks remain opt-in.
- `cargo test --locked --lib gpu:: -- --ignored`: **16 native checks pass**, covering filters, every adjustment, masks/clipping, brush operations, resampling, 8/16-bit RAW, resource fallback, cancellation, and transfer-inclusive timing. `scripts/check.sh --gpu` also exercises native Motion Blur Apply/Cancel/undo.
- Tests cover project round trips and atomic overwrite, original Swift dictionaries/transforms, unsafe asset paths, hierarchy and clipping validation, PNG/JPEG/TIFF/WebP export, PNG DPI, selection connectivity and coverage, copy-on-write pixels, expanded paint/blur bounds, live shape redraw, group transforms, linked mask placement, and history revisions.
- egui input tests exercise brush/selection/pixel-move gestures, scale/distort handles, independent tabs, layer copying between projects, adjustment cancellation, export preview, and worker commit/cancellation. Style regression tests cover titlebar dragging, double-click maximize/restore, and unsaved-close handling, floating-panel dragging and minimum-size bounds, keyboard/disabled slider behavior, and Levels handle clamping.
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
- Preview resolution is capped at 4096 px per side, or 1600 px in the CPU fallback. Full-resolution filter Apply and export are independent of that cap. Imports, saves, and raster adjustments are synchronous; raster filters and expensive retouching jobs are cancellable background operations.
- Vulkan is tested on both window systems. Explicit OpenGL startup on this workstation returned an incompatible-surface adapter error; availability depends on the EGL/driver configuration.
- The local release archive requires glibc 2.43. For older distributions, build from source on the target distribution. The CI definition uses Ubuntu 24.04 as its packaging baseline.

Completed features are recorded as conventional Git commits. No source repository files, system installation directories, or remote repositories were modified by this port.
