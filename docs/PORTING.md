# Compositor → xuan

Source baseline: `/home/silver/workspace/repo/Compositor` (Swift, AppKit, Core Graphics, Metal).

## Architecture

- Rust document model with immutable, shared pixel assets and snapshot undo/redo.
- Deterministic raster engine for compositing, brushes, selections, masks, adjustments and export.
- egui desktop interface rendered by wgpu on Wayland and X11.
- Portable `.xuan` archives with embedded PNG assets; import original `.comp` directory packages.
- File operations validate inputs and stage saves before replacing existing files.

## Implementation sequence

1. Document, layers, transforms, blend modes, history, and core renderer.
2. Selection and painting tools, masks, retouching, shapes and gradients.
3. Adjustments, filters, canvas operations and project/image I/O.
4. Themed desktop shell, panels, tabs, menus and canvas interactions.
5. Linux integration, regression coverage, visual and runtime validation.

Each completed module is committed separately using a conventional commit message. Feature parity is assessed against the source, not just the README (the source currently writes `.comp` version 7).

## Visual reference

Source metrics: background white level 0.14; tool rail 56 px; tool buttons 36 px with 7 px corners; contextual header 42 px; Layers panel 252 px, resizable 202–352 px; status bar 30 px. Typography is restrained (11–13 px UI text), with rounded controls, neutral selections, subtle dividers and a dark checkerboard canvas.

## Verification

Track module tests, build checks, native startup and screenshot inspection here as completed. Known gaps must be explicit before declaring the port complete.
