# HEIC import fixtures

These synthetic images come from [heic-rs](https://github.com/tbraun96/heic-rs/tree/4f0d4df474c773dfc3b14fa80e7219be6898866e/tests/fixtures), commit `4f0d4df474c773dfc3b14fa80e7219be6898866e`:

- `rgb-strips.heic` is upstream's `rgb-strips-96.heic`: a 96 × 32 image with red, green and blue strips.
- `checker-grid.heic` is upstream's `checker-1024.heic`: a 1024 × 1024 checkerboard stored as four 512 × 512 tiles.

Upstream generated these from synthetic PNGs with Apple's `sips`. They are used under the [MIT license](../../../licenses/heic-rs-MIT.txt). Tests modify the small image's rotation and dimensions in memory to exercise orientation and size validation. Tests require no external converter or network access.
