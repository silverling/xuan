# Third-party components

Xuan's own source code is MIT licensed. Dependency licenses remain applicable to their respective code.

- **Rawler 0.7.2** provides Nikon RAW decoding, camera calibration data, and PPG demosaicing. Copyright Daniel Vogelbacher, Pedro Côrte-Real, and the Rawler contributors. Licensed under the GNU LGPL 2.1; see [the license](licenses/rawler-LGPL-2.1.txt) and [upstream source](https://crates.io/crates/rawler/0.7.2).
- **Inter Variable**: SIL Open Font License, included in `assets/fonts/Inter-LICENSE.txt`.
- **egui-winit**: MIT / Apache 2.0, included in `vendor/egui-winit` with the local patch notes.
- **sRGB ICC profile**: generated with Little CMS through Pillow's `ImageCms.createProfile("sRGB")`. The profile is embedded in Develop's 16-bit TIFF exports; generating it is not a build or runtime requirement.

Release archives include a `source` directory containing Xuan's build sources and the exact Rawler sources used to build the binary. To rebuild or relink with a modified Rawler, edit `source/vendor/rawler`, then run `cargo build --release` in `source`. A Rust toolchain, a C toolchain, the Linux libraries listed in the README, and access to the Cargo registry for other dependencies are required. The included manifest patches Rawler to the local sources; Cargo adjusts the lockfile for this path dependency on the first build.
