# Using Xuan

## Install and launch

Xuan runs on Linux with Wayland or X11 and working Vulkan drivers. Mesa software Vulkan can also run the editor. Native file dialogs use the desktop portal; install the portal backend for your desktop if dialogs do not appear.

From an extracted release archive:

```sh
scripts/install.sh                 # installs under ~/.local
~/.local/bin/xuan
```

The installer adds a desktop launcher, icons, and the `.xuan` file association. Use `scripts/install.sh /custom/prefix` to choose another location, and add the installation prefix's `bin` directory to `PATH`. You can also run `bin/xuan` directly from the extracted archive.

Once `xuan` is on `PATH`:

```sh
xuan --demo
xuan photograph.png composition.xuan
```

To compile the application or produce a release archive, see the [development guide](DEVELOPMENT.md). Archives built on newer distributions may require a newer glibc; build from source on your target distribution if needed.

## Workspace

Xuan has a charcoal theme, contextual controls above the canvas, a vertical tool rail, document tabs, and a Layers panel. The menu bar shares the titlebar with the window controls. Drag the titlebar to move the window, double-click to maximize, or drag an edge to resize.

## Editing tools

- **Layers:** folders, 13 blend modes, opacity, visibility, locks, drag reordering/nesting, duplication, merge, clipping masks, linked or independent raster masks, and copying layers to another project tab.
- **Transforms:** move, scale, rotate, flip, free perspective distortion, numeric controls, shared transforms for several layers or folders, and snapping to edges and centers. Original source pixels remain available during transforms. Move / Transform selects and drags anywhere inside a layer's bounds by default; check **Ignore Transparent Pixels** to select only at visible pixels.
- **Selections:** rectangle, ellipse, freehand/polygonal lasso, contiguous/global magic wand, add/subtract/intersect, inverse, feather, outline movement, and moving or duplicating selected pixels.
- **Paint:** brush, eraser, aligned/unaligned clone stamp with layer/all-layer sampling, spot healing, blur/smudge, gradients, rectangles, rounded rectangles, ellipses, and eyedropper. Live shapes redraw at the new size until their pixels are edited.
- **Text:** editable multiline text layers, searchable installed font families, size and color, bold, italic, underline, and strikethrough. Click with the Text tool (T) to place or edit text; double-click a text layer to reopen its live preview. Move and transform text with the Move tool.
- **Adjustments:** editable Hue/Saturation color ranges, per-channel Levels and Curves, Exposure, Gradient Map, Grain, and Invert. Apply directly or add an adjustment layer, with live preview and selection coverage.
- **Filters:** Gaussian and Motion Blur with expanded bounds, Add Noise, Lens Correction, content-aware fill, and edge-color background removal. Filtering and expensive retouching run in cancellable workers.
- **RAW Develop:** NEF/NRW opens in a dedicated Develop workspace. Adjust white balance, exposure, tone curves, HSL, monochrome and split toning, noise reduction, sharpening, manual lens correction, crop, and brush/gradient masks. Compare before/after and inspect clipping or full-resolution detail. Develop creates an embedded RAW layer; double-click it to edit the original RAW again. Save `.xuan` to retain the source and adjustments, or export a 16-bit sRGB TIFF directly from Develop. See [RAW workflow and limits](RAW.md).
- **Documents:** independent tab histories, crop, canvas/image size, high-quality downsampling, pixel grid, pasting copied images or image files as layers, Copy Merged, and save-on-close prompts. Undo retains up to 64 steps with a 512 MiB asset budget, keeping at least one step.

See [keyboard shortcuts](SHORTCUTS.md) for tool and command bindings.

## Files and export

Use **File → Open Compositor Project…** to import an original `.comp` folder package. Save it as `.xuan` to keep editing on Linux. Image export supports PNG, JPEG, TIFF, and WebP; JPEG has a quality preview and PNG/JPEG carry print resolution.

HEIC/HEIF import uses the optional `heif-convert` executable from `libheif-examples`. Nikon NEF/NRW import uses the bundled Rawler library and needs no external converter. Other image formats are decoded directly in Rust. See the [project format](FORMAT.md) for details about saved documents.

## Current limits

The photo editor uses an 8-bit sRGB raster pipeline. RAW Develop uses floating-point camera data and offers direct 16-bit TIFF output with an sRGB profile; its photo-layer render uses the existing 8-bit pipeline. Imported raster ICC profiles are not converted or preserved. `.comp` versions 1–7 can be imported; Xuan does not write the original macOS format. Selections and undo history are session state and are not saved in project archives.

Remove Background uses a border-color matte, intended for simple backgrounds, instead of Apple's foreground-recognition service. Content-aware fill and healing use a portable texture-matching implementation, so results differ from Compositor. The canvas preview is capped at 4096 pixels per side; filter Apply uses full layer dimensions and export uses full document dimensions. Imports, saves, and raster adjustments can temporarily occupy the UI thread. Vulkan is the verified rendering path; OpenGL surface availability depends on the driver.
