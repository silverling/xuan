# Xuan

A native Linux image editor for layered compositions, photo retouching, and Nikon RAW development.

![Xuan editing a layered composition](docs/screenshots/editor.png)

## Features

- Compose with layers, groups, masks, blend modes, editable text, and shapes.
- Retouch with selections, brushes, clone stamp, healing, filters, and adjustment layers.
- Develop Nikon NEF/NRW files and return to their RAW settings at any time.
- Save editable `.xuan` projects, import Compositor projects, and export PNG, JPEG, TIFF, or WebP.

## Get started

From an extracted release archive, install and launch the demo:

```sh
scripts/install.sh
~/.local/bin/xuan --demo
```

Xuan supports Wayland and X11 and requires working Vulkan drivers. See the [user guide](docs/USAGE.md) for installation, file formats, and editing tools. To build from source, follow the [development guide](docs/DEVELOPMENT.md).

[Keyboard shortcuts](docs/SHORTCUTS.md) · [RAW workflow](docs/RAW.md) · [Project format](docs/FORMAT.md)

## License

A Rust port of Compositor. Source code is [MIT licensed](LICENSE); original Compositor copyright © 2026 Wonder Assembly LLC. Dependency licenses and rebuild instructions are in [third-party notices](THIRD_PARTY.md).
