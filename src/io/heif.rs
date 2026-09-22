use anyhow::{Context, Result, ensure};
use heic_rs::{DecodeOptions, PixelLayout};
use image::RgbaImage;

use crate::document::{MAX_PIXELS, validate_size};

pub(super) fn decode(bytes: &[u8], used: &mut u64) -> Result<RgbaImage> {
    let info = heic_rs::probe(bytes).context("Cannot read HEIC/HEIF image")?;
    // Check both sizes: a clean aperture may hide a much larger coded image.
    validate_size(info.coded_width, info.coded_height)?;
    super::reserve_pixels(info.width, info.height, used)?;

    let options = DecodeOptions {
        layout: PixelLayout::Rgba8,
        max_pixels: Some(MAX_PIXELS),
        strict: true,
        ..Default::default()
    };
    // Decode the primary still image, including grids, alpha and container transforms.
    let image = heic_rs::decode(bytes, &options).context("Cannot decode HEIC/HEIF image")?;
    validate_size(image.width, image.height)?;
    ensure!(
        (image.width, image.height) == (info.width, info.height),
        "HEIC/HEIF decoded dimensions do not match its metadata"
    );
    RgbaImage::from_raw(image.width, image.height, image.data)
        .context("HEIC/HEIF decoder returned an invalid pixel buffer")
}

#[cfg(test)]
#[path = "heif_tests.rs"]
mod tests;
