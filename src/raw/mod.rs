//! Nondestructive camera RAW assets and a floating-point Develop pipeline.
mod process;
mod settings;
#[cfg(test)]
mod tests;

use std::{fs::File, io::Read, path::Path, sync::Arc};

use anyhow::{Context, Result, bail, ensure};
use image::{DynamicImage, ImageBuffer, Rgb32FImage, RgbaImage};
use rawler::{
    decoders::RawDecodeParams,
    imgop::{
        develop::{Intermediate, ProcessingStep, RawDevelop},
        matrix::{multiply, normalize, pseudo_inverse},
        xyz::{Illuminant, SRGB_TO_XYZ_D65},
    },
    rawimage::RawPhotometricInterpretation,
    rawsource::RawSource,
};
use serde::{Deserialize, Serialize};

use crate::document::validate_size;
pub use process::{auto_exposure, render, render_16, sample_white_balance, source_point};
pub use settings::{DevelopSettings, Overlay, OverlayKind, WhiteBalance};

pub const MAX_RAW_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct RawMetadata {
    pub camera: String,
    pub lens: String,
    pub iso: Option<u32>,
    pub aperture: Option<f32>,
    pub shutter: Option<f32>,
    pub focal_length: Option<f32>,
    pub width: u32,
    pub height: u32,
    pub bits: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RawAsset {
    pub filename: String,
    pub metadata: RawMetadata,
    pub settings: DevelopSettings,
    /// Stored separately in the project archive. Never overwrite the camera file.
    #[serde(skip)]
    pub bytes: Arc<Vec<u8>>,
}

impl RawAsset {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.filename.is_empty() && self.filename.len() <= 16_384,
            "Invalid RAW filename"
        );
        ensure!(
            !self.bytes.is_empty() && self.bytes.len() as u64 <= MAX_RAW_BYTES,
            "Missing or oversized RAW source"
        );
        validate_size(self.metadata.width, self.metadata.height)?;
        self.settings.validate()
    }
}

/// Demosaiced, oriented, black-level-normalized camera RGB. Values are not clipped
/// at 1.0 and have not had white balance, exposure, color conversion or gamma applied.
#[derive(Debug)]
pub struct DecodedRaw {
    pub camera: Rgb32FImage,
    pub as_shot: [f32; 3],
    pub camera_to_rgb: [[f32; 3]; 3],
    pub xyz_to_camera: [[f32; 3]; 3],
    pub metadata: RawMetadata,
}

impl DecodedRaw {
    pub fn preview(&self, max_side: u32) -> Self {
        let scale =
            (max_side as f32 / self.camera.width().max(self.camera.height()) as f32).min(1.0);
        Self {
            camera: image::imageops::resize(
                &self.camera,
                (self.camera.width() as f32 * scale).round().max(1.0) as u32,
                (self.camera.height() as f32 * scale).round().max(1.0) as u32,
                image::imageops::FilterType::Triangle,
            ),
            as_shot: self.as_shot,
            camera_to_rgb: self.camera_to_rgb,
            xyz_to_camera: self.xyz_to_camera,
            metadata: self.metadata.clone(),
        }
    }
}

pub fn is_raw(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("nef") || s.eq_ignore_ascii_case("nrw"))
}

pub fn open(path: &Path) -> Result<(RawAsset, DecodedRaw)> {
    let file = File::open(path).with_context(|| format!("Cannot read {}", path.display()))?;
    ensure!(
        file.metadata()?.len() <= MAX_RAW_BYTES,
        "RAW file exceeds 512 MiB"
    );
    let mut bytes = Vec::new();
    file.take(MAX_RAW_BYTES + 1).read_to_end(&mut bytes)?;
    let decoded = decode(&bytes)?;
    let asset = RawAsset {
        filename: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into(),
        metadata: decoded.metadata.clone(),
        settings: DevelopSettings::default(),
        bytes: Arc::new(bytes),
    };
    Ok((asset, decoded))
}

pub fn decode(bytes: &[u8]) -> Result<DecodedRaw> {
    ensure!(
        !bytes.is_empty() && bytes.len() as u64 <= MAX_RAW_BYTES,
        "Empty or oversized RAW file"
    );
    // The external decoder has panic paths for unsupported encodings. Convert these
    // into import errors so a failed camera file cannot unwind through the editor.
    std::panic::catch_unwind(|| decode_inner(bytes))
        .map_err(|_| anyhow::anyhow!("The RAW decoder could not process this camera file"))?
}

fn decode_inner(bytes: &[u8]) -> Result<DecodedRaw> {
    let source = RawSource::new_from_slice(bytes);
    let header = rawler::decode_dummy(&source).context("Unsupported or damaged RAW file")?;
    validate_size(header.width.try_into()?, header.height.try_into()?)?;
    ensure!(
        matches!(&header.photometric, RawPhotometricInterpretation::Cfa(c)
        if c.cfa.is_rgb() && c.cfa.width == 2 && c.cfa.height == 2),
        "This RAW sensor layout is not supported; an RGB Bayer NEF is required"
    );
    let decoder = rawler::get_decoder(&source)?;
    let params = RawDecodeParams::default();
    let metadata = decoder.raw_metadata(&source, &params)?;
    let raw = decoder.raw_image(&source, &params, false)?;
    validate_size(raw.width.try_into()?, raw.height.try_into()?)?;
    let matrix = raw
        .color_matrix
        .get(&Illuminant::D65)
        .or_else(|| raw.color_matrix.get(&Illuminant::A))
        .or_else(|| {
            raw.color_matrix
                .iter()
                .min_by_key(|(key, _)| **key as u16)
                .map(|(_, value)| value)
        })
        .context("No camera color calibration is available")?;
    ensure!(matrix.len() == 9, "Unsupported camera color matrix");
    let xyz_to_camera = std::array::from_fn(|i| std::array::from_fn(|j| matrix[i * 3 + j]));
    let camera_to_rgb = pseudo_inverse(normalize(multiply(&xyz_to_camera, &SRGB_TO_XYZ_D65)));
    ensure!(
        camera_to_rgb.iter().flatten().all(|v| v.is_finite()),
        "Invalid camera color calibration"
    );
    let as_shot = std::array::from_fn(|i| raw.wb_coeffs[i] / raw.wb_coeffs[1]);
    ensure!(
        as_shot.iter().all(|v| v.is_finite() && *v > 0.0),
        "Invalid camera white balance"
    );
    let developer = RawDevelop {
        steps: vec![
            ProcessingStep::Rescale,
            ProcessingStep::Demosaic,
            ProcessingStep::CropActiveArea,
            ProcessingStep::CropDefault,
        ],
    };
    let Intermediate::ThreeColor(pixels) = developer.develop_intermediate(&raw)? else {
        bail!("RAW decoder did not produce an RGB image");
    };
    let camera = ImageBuffer::from_raw(
        pixels.width as u32,
        pixels.height as u32,
        pixels.data.into_iter().flatten().collect(),
    )
    .context("Invalid decoded RAW dimensions")?;
    let mut oriented = DynamicImage::ImageRgb32F(camera);
    oriented.apply_orientation(
        image::metadata::Orientation::from_exif(
            metadata
                .exif
                .orientation
                .unwrap_or(raw.orientation.to_u16()) as u8,
        )
        .unwrap_or(image::metadata::Orientation::NoTransforms),
    );
    let camera = oriented.into_rgb32f();
    let exif = metadata.exif;
    let metadata = RawMetadata {
        camera: format!("{} {}", raw.clean_make, raw.clean_model),
        lens: exif.lens_model.unwrap_or_default(),
        iso: exif.iso_speed.or(exif.iso_speed_ratings.map(u32::from)),
        aperture: exif.fnumber.map(|v| v.as_f32()),
        shutter: exif.exposure_time.map(|v| v.as_f32()),
        focal_length: exif.focal_length.map(|v| v.as_f32()),
        width: camera.width(),
        height: camera.height(),
        bits: raw.bps,
    };
    Ok(DecodedRaw {
        camera,
        as_shot,
        camera_to_rgb,
        xyz_to_camera,
        metadata,
    })
}

/// Preserve layer placement, masks, blending, and identity when redeveloping.
pub fn update_layer(
    layer: &mut crate::document::Layer,
    asset: RawAsset,
    pixels: RgbaImage,
) -> Result<()> {
    ensure!(
        !layer.locked && layer.raw.is_some(),
        "Select an unlocked RAW layer"
    );
    asset.validate()?;
    validate_size(pixels.width(), pixels.height())?;
    // A new crop changes source bounds. Map it through the old placement so
    // uncropped content stays at its original document position and scale.
    let old = layer.raw.as_ref().unwrap().settings.crop;
    let new = asset.settings.crop;
    if old != new {
        let transform = layer.transform.expanded(
            (new[0] - old[0]) / (old[2] - old[0]),
            (new[1] - old[1]) / (old[3] - old[1]),
            (new[2] - old[0]) / (old[2] - old[0]),
            (new[3] - old[1]) / (old[3] - old[1]),
        );
        // Existing masks keep their document-space alignment through a RAW crop.
        if let Some(mask) = &mut layer.mask {
            mask.placement = Some(mask.placement.unwrap_or(layer.transform));
        }
        layer.transform = transform;
    }
    layer.raw = Some(asset);
    layer.pixels = Some(Arc::new(pixels));
    Ok(())
}
