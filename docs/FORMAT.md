# Project format

A `.xuan` file is a ZIP archive containing `manifest.json` and lossless PNG assets. Version 1 uses `format: "org.xuan.project"`, a `document` object, and a `pixel_layers` list. Layer images are stored at `images/<UUID>.png`; masks use `images/<UUID>.mask.png`. Pixel bytes are excluded from JSON.

The document records canvas dimensions, DPI, layer order, IDs, parent groups, clipping references, opacity, blending, visibility, locks, transforms, adjustment parameters, and optional live shape styles. Layers are stored bottom to top; each folder's subtree is composited together in hierarchy order. Masks have enabled/linked flags and an optional independent placement transform.

Text layers also record an optional `text` object with UTF-8 content, font family, pixel size, RGBA color, and bold, italic, underline, and strikethrough flags. Their PNG assets preserve the rendered appearance when fonts are unavailable on another machine. Fonts are discovered from the system and are not embedded in the project; editing unavailable fonts uses the bundled Inter Variable fallback. Text is limited to 16 KiB and font sizes to 1–1024 pixels. Older version 1 files without text metadata remain supported.

Transforms retain original source pixels. Optional perspective corners are normalized coordinates before affine scale/rotation/flip. Channel adjustments retain separate RGB curves/levels and seven hue ranges. Selections, current multi-selection, clipboard contents, and undo/redo snapshots are not serialized.

Saving validates the document, writes a sibling temporary archive, flushes it, and atomically replaces the destination. Loading validates IDs, hierarchy, clipping cycles, transforms, dimensions, metadata size, decompressed asset size, and aggregate image/mask budgets. Assets are decoded in memory, never extracted using archive paths. `.comp` package reads reject symlinked assets and paths outside the package.

Limits: 30,000 pixels per canvas/image dimension, 100 megapixels per canvas, 100 megapixels of layer assets plus 100 megapixels of masks, 10,000 layers, 64 nested group levels, 4 MiB manifest JSON, and 512 MiB encoded asset files.

The importer accepts Compositor package versions 1–7, including Swift's alternating-key enum dictionaries, individual color channels, mask placement/link flags, grain parameters, and live shape styles. Import is one-way: Save creates a `.xuan` file and leaves the `.comp` package untouched.
