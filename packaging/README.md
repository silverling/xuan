# Xuan

A native image editor for layered compositions, photo retouching, and Nikon RAW development on Linux and Windows.

On Windows, extract the entire ZIP and double-click `xuan.exe`, or run `.\xuan.exe --demo` in PowerShell to explore a sample composition.

On Linux, launch Xuan from your application menu or run `xuan --demo`. In a portable Linux archive, run `bin/xuan` directly or use `scripts/install.sh` to install under `~/.local`.

For an AppImage, make the downloaded file executable with `chmod +x xuan-*.AppImage`, then run `./xuan-*.AppImage --demo`. If FUSE is unavailable, add `--appimage-extract-and-run` before `--demo`.

- [Installation and editing](../docs/USAGE.md)
- [Keyboard shortcuts](../docs/SHORTCUTS.md)
- [Nikon RAW workflow](../docs/RAW.md)
- [Source download and rebuilding](SOURCES.md)
- [Third-party notices](../THIRD_PARTY.md)

Xuan is [MIT licensed](../LICENSE), with separately licensed dependencies. Original Compositor copyright © 2026 Wonder Assembly LLC.
