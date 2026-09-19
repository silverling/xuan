# Nikon RAW Develop

Open a `.nef` or `.nrw` through File → Open, Import Image as Layer, the command line, a file-manager paste, or drag-and-drop. The image first opens in Develop. Files selected together are queued, so each RAW receives its own Develop session. Import-as-layer remembers its destination project.

Use **Develop** to create a photo layer, or **Cancel** to leave the destination document unchanged. Double-click the RAW layer row or the image with Move selected to return to Develop. Layer → Develop RAW and the layer context menu also reopen it. A committed redevelopment is one document undo step; changes inside Develop have their own Undo/Redo. Cancelling redevelopment retains the last committed pixels and settings.

Save as `.xuan` to embed the original RAW bytes, shooting metadata, and all Develop adjustments. The original camera file is never modified and is not needed to reopen a saved project. Duplicates share the in-memory source but have independent settings. Moving, scaling, rotating, masking, grouping, and blending a RAW layer retain editability. Reopening a RAW processes that layer in isolation; the result returns to its existing position in the composition.

Direct pixel painting and destructive filters require **Rasterize RAW Layer**, which is undoable. Paint on a separate pixel layer and use adjustment layers when you want to retain RAW editing. Merging or flattening produces ordinary pixel layers.

## Controls

- **Basic:** as-shot white balance, temperature/tint and lighting presets, neutral picker, auto exposure, ±10 EV exposure, brightness, contrast, highlights/shadows, white/black points, clarity, texture, dehaze, vibrance, saturation.
- **Tone:** draggable five-knot master and RGB curves; eight-band hue/saturation/lightness; monochrome RGB mix; shadow/highlight split toning and balance.
- **Detail:** edge-aware luminance/chroma noise reduction; luminance sharpening with radius and threshold. Choose **100%** or **Full-resolution preview** to evaluate sensor-scale detail. Fit previews are downsampled and can differ in fine detail. Full-resolution preview is available when the image fits the GPU's texture-size limit; larger images still develop/export at full resolution.
- **Lens:** manual radial distortion, red/cyan and blue/yellow aberration, purple defringing, vignette compensation, rotation, horizontal/vertical perspective, and normalized crop bounds. A crop preserves the placement of surviving pixels when redeveloping an existing layer.
- **Masks:** linear and radial gradients or soft brush masks with exposure, warmth, saturation, visibility and inversion. Select a mask, enable Draw mask, and drag on the image. The brush radius/feather applies to the entire stroke collection in that mask. Up to 32 masks and 8,192 total brush points are saved with the RAW.
- **Info:** camera, lens, oriented dimensions, decoded depth, ISO, aperture, shutter, focal length and embedded source size.

The RGB histogram and clipping indicators describe the developed output. Compare Edited, Original (default development), Split, or Side by side. Drag the split divider; Alt-drag pans in Split view. In other views, drag pans and the wheel zooms. Presets can save/load validated JSON settings, including crops and masks.

## Precision and output

Rawler decodes the actual sensor data, corrects camera black/white levels, and performs PPG Bayer demosaicing. Xuan preserves oriented camera RGB in 32-bit floating point, applies white balance and exposure before the camera-to-sRGB matrix and display transfer function, and retains values above 1 until tone processing. Exposure reduction can therefore recover encoded highlight differences that are absent in an 8-bit preview. Sensor-saturated channels contain no recoverable detail.

**Develop** renders a full-resolution 8-bit sRGB photo layer, matching the existing compositor. **16-bit TIFF…** renders directly from the floating-point pipeline, without an intermediate 8-bit conversion, and embeds an sRGB ICC profile. It exports the current RAW development alone, including crop and local masks. To export the whole composition, use the normal photo editor's File → Export. Both outputs are independent of preview zoom and comparison/clipping overlays.

Decoding, previews, full-resolution development, and TIFF encoding run in background workers. Preview edits are debounced, and stale results are discarded. Cancelling never commits a worker's late result. RAW import/development is limited to the editor's 100-megapixel image limit; project RAW sources have an aggregate 512 MiB budget. Full-resolution processing uses substantially more memory than the fit preview.

## Current limits

This implements the Develop → embedded RAW layer → Develop workflow and the controls listed above. It is not full Affinity feature parity. Camera support follows Rawler 0.7.2's NEF/NRW decoder and requires an RGB Bayer sensor. Unsupported/damaged files produce an error. The camera's embedded JPEG is not used as the development source.

Lens correction is manual; there is no automatic lens-profile database. Noise reduction is a conventional local filter, not a learned denoiser. Defringing suppresses purple excess and can affect purple objects. There is no reconstruction of saturated sensor channels, dual-illuminant profile interpolation, custom camera/ICC output profiles, wide-gamut/HDR compositor, RAW spot-healing tool, automatic subject masks, or batch preset development. Use the photo editor's healing tools after developing/rasterizing. RAW metadata remains in the project; the TIFF export currently includes the output color profile but does not copy shooting EXIF.
