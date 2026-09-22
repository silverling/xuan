# Development

Run the commands below from the repository root. For installation and editing, see the [user guide](USAGE.md).

## Prerequisites

Requires Rust **1.88+** and a C toolchain.

### Linux

Use a Linux desktop with working Vulkan drivers. Wayland and X11 are supported; Mesa software Vulkan can also run the editor. Native file dialogs use the desktop portal, so install the portal backend for your desktop if dialogs do not appear.

Typical Debian/Ubuntu prerequisites:

```sh
sudo apt install build-essential pkg-config libxkbcommon-dev libwayland-dev \
    libvulkan1 mesa-vulkan-drivers xdg-desktop-portal libheif-examples
```

`libheif-examples` supplies the optional `heif-convert` executable for HEIC/HEIF import. Nikon NEF/NRW import uses the bundled Rawler library and needs no external converter.

### Windows

Install the x86_64 MSVC Rust toolchain (`stable-x86_64-pc-windows-msvc`) and Visual Studio Build Tools with **Desktop development with C++** and a Windows SDK. Use Windows 10/11 with a DirectX 12 or Vulkan driver. Native file dialogs and the clipboard use Windows APIs. Packaging also requires Python **3.11+** on `PATH`.

## Build and run

```sh
cargo run --release --locked
cargo run --release --locked -- --demo
cargo run --release --locked -- photograph.png composition.xuan
```

To install a local Linux build:

```sh
cargo build --release --locked
scripts/install.sh                 # installs under ~/.local
scripts/install.sh /custom/prefix   # optional destination
```

The installer adds a desktop launcher, icons, and the `.xuan` file association. Add the installation prefix's `bin` directory to `PATH`.

## Packaging

On Windows, run from PowerShell:

```powershell
python scripts/package-windows.py
python scripts/check-packages.py
```

This builds `xuan-<version>-windows-x86_64.zip`, its SHA-256 checksum, and the matching source archive in `dist/`. The ZIP includes `xuan.exe` plus licenses and user guides under `share/`. The release executable uses the Windows GUI subsystem and statically links the MSVC runtime, so it opens without a console window and needs no Visual C++ redistributable. The script builds the explicit `x86_64-pc-windows-msvc` target on an x86_64 Windows host. The ZIP is unsigned and does not install file associations.

On Linux:

```sh
scripts/package.sh                 # portable archive (default)
scripts/package.sh deb             # Debian/Ubuntu package
scripts/package.sh rpm             # RPM package
scripts/package.sh all             # all three binary formats and matching sources
python3 scripts/check-packages.py  # inspect binary packages and source archive
```

Outputs and individual SHA-256 checksums are written to `dist/`. Both packaging scripts also produce `xuan-<version>-source.tar.gz`; distribute that matching source archive alongside the binaries. Linux packaging requires Python **3.11+**, binutils, and the normal build prerequisites. Debian packaging additionally needs `dpkg-deb`; RPM packaging needs `rpmbuild`. On Debian/Ubuntu, install the packaging and inspection tools with `sudo apt install dpkg rpm cpio binutils desktop-file-utils`. Linux packaging supports native x86_64 and aarch64 builds. The packaging scripts do not support cross-compilation.

A portable Linux archive includes `bin/xuan`, `scripts/install.sh`, and a `share/` directory for desktop integration, icons, licenses, and user guides. It can run directly after extraction. Debian and RPM packages install the same application files under `/usr`. Binary packages omit source code, CI workflows, original artwork, development guides, and screenshots. Their `share/doc/xuan/SOURCES.md` notice points to the exact source download; links to development documentation point to the release's repository tag.

The separate source archive contains the application and build assets, the patched egui-winit sources, and the exact Rawler sources. Its manifest and lockfile use the bundled Rawler directory so `cargo build --release --locked` works after extraction. Other dependencies are downloaded from the Cargo registry. See [third-party notices](../THIRD_PARTY.md).

Package versions come from `Cargo.toml`. Prerelease versions such as `0.2.0-rc.1` become `0.2.0~rc.1` in Debian/RPM metadata so they sort before the final release. Build metadata (`+...`) is not supported. The Debian package records the executable's required glibc version; RPM derives ELF library requirements automatically. Both declare desktop libraries that are loaded at runtime.

Build on the oldest distribution you intend to support. The locally produced archive uses this workstation's glibc **2.43**; build from source on older distributions. The [CI workflow](../.github/workflows/linux.yml) builds on Ubuntu 24.04.

## GitHub releases

The [release workflow](../.github/workflows/release.yml) runs when a `v*` tag is pushed. It rejects tags that do not match the package version in `Cargo.toml`, then runs the shared Linux and Windows checks. Linux builds all three x86_64 packages and the matching source archive on Ubuntu 24.04 and tests Debian installation, native startup, and removal. The [Windows workflow](../.github/workflows/windows.yml) runs on Windows Server 2022, builds the portable MSVC x86_64 ZIP, verifies its payload and checksums, and launches the packaged application with DirectX 12 to capture a demo screenshot. Both jobs must pass before publishing. Package checks reject development files in binary payloads and verify the source archive's vendored dependencies and resolved lockfile.

To release, update the version in `Cargo.toml` and the `xuan` entry in `Cargo.lock`, commit the changes, then create and push the matching tag. For example, for version `0.2.0`:

```sh
git tag -a v0.2.0 -m 'Release v0.2.0'
git push origin v0.2.0
```

After validation succeeds, [changelogithub](https://github.com/antfu-collective/changelogithub) generates release notes from conventional commits. The GitHub CLI uploads the three Linux binary packages, Windows ZIP, matching source archive, and all five checksums. Missing artifacts fail the workflow before publication; retries replace matching assets and upload failures fail the workflow. The workflow fetches the full Git history and uses pinned changelogithub **15.0.5** with Node.js 24. Only the publishing job receives `contents: write`; it uses the built-in `GITHUB_TOKEN` and needs no separate release secret. Prerelease tags such as `v0.2.0-rc.1` are marked as GitHub prereleases.

Preview release notes locally without publishing:

```sh
npx --yes changelogithub@15.0.5 --dry --to HEAD --github silverling/xuan
```

The first release uses the available commit history; subsequent notes start after the preceding release tag. To retry a failed release, rerun its workflow in GitHub Actions.

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

On Windows, run the equivalent checks in PowerShell:

```powershell
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo test --locked --package egui-winit --lib clipboard_paste
```

The GPU checks require a working graphics environment. CI also validates the desktop entry, builds the release archive, and runs native screenshot and clipboard checks under Xvfb. See [implementation and verification notes](PORTING.md) for the architecture and recorded results.

## RAW sample checks

The regular test suite uses synthetic camera-linear data and small embedded-asset fixtures. Camera files are not committed to the repository. Optional tests use a local NEF:

```sh
XUAN_TEST_NEF=/path/to/photo.NEF cargo test --locked sample_nef -- --ignored --nocapture
cargo run --locked -- /path/to/photo.NEF --screenshot /tmp/develop.png
```

`XUAN_TEST_RAW_PREVIEW=/tmp/preview.png` optionally writes the engine test's default preview. The provided Nikon Z6 III sample was verified at 4032 × 6048 after orientation, including full-resolution rendering, project save/load, reopening Develop and cancellation.

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
