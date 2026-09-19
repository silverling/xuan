use super::{Processor, processor::attempt};
use crate::document::{Adjustment, Document, Layer, Transform};
use anyhow::{Result, ensure};
use image::{GrayImage, Rgb32FImage, RgbaImage};

pub(super) const RASTER: &str = concat!(include_str!("buffers.wgsl"), include_str!("raster.wgsl"));
const NEIGHBORHOOD_MIN: u64 = 16_384;
const POINTWISE_MIN: u64 = 65_536;

fn count(size: [u32; 2]) -> u64 {
    u64::from(size[0]) * u64::from(size[1])
}

pub fn filter(image: &RgbaImage, filter: &crate::effects::Filter) -> Option<RgbaImage> {
    use crate::effects::Filter;
    let size = [image.width(), image.height()];
    attempt(count(size), NEIGHBORHOOD_MIN, |gpu| {
        let bytes = match filter {
            Filter::GaussianBlur { radius } => {
                gpu.separable(image.as_raw(), size, size, 0, Some(radius.max(0.01)))?
            }
            Filter::LensCorrection {
                distortion,
                vignette,
            } => gpu.simple(
                "lens",
                RASTER,
                image.as_raw(),
                &[],
                &[
                    [size[0] as f32, size[1] as f32, 0.0, 0.0],
                    [*distortion, *vignette, 0.0, 0.0],
                ],
                size,
            )?,
            Filter::Noise { amount, monochrome } => {
                return gpu.adjustment(
                    image,
                    &Adjustment::Grain {
                        amount: *amount,
                        monochrome: *monochrome,
                        seed: 3187,
                    },
                    Transform::new(size[0], size[1]),
                    None,
                    true,
                );
            }
            Filter::MotionBlur { distance, angle } => {
                return gpu
                    .motion_blur
                    .render(
                        &std::sync::Arc::new(image.clone()),
                        *distance,
                        *angle,
                        0,
                        &std::sync::atomic::AtomicBool::new(false),
                    )?
                    .ok_or_else(|| anyhow::anyhow!("Image exceeds GPU texture limits"));
            }
        };
        Ok(RgbaImage::from_raw(size[0], size[1], bytes).unwrap())
    })
}

pub fn adjustment(
    image: &RgbaImage,
    adjustment: &Adjustment,
    transform: Transform,
    selection: Option<&GrayImage>,
) -> Option<RgbaImage> {
    attempt(
        u64::from(image.width()) * u64::from(image.height()),
        POINTWISE_MIN,
        |gpu| gpu.adjustment(image, adjustment, transform, selection, false),
    )
}

pub fn resize_rgba(image: &RgbaImage, width: u32, height: u32) -> Option<RgbaImage> {
    let size = [image.width(), image.height()];
    attempt(
        count(size).max(u64::from(width) * u64::from(height)),
        NEIGHBORHOOD_MIN,
        |gpu| {
            Ok(RgbaImage::from_raw(
                width,
                height,
                gpu.separable(image.as_raw(), size, [width, height], 0, None)?,
            )
            .unwrap())
        },
    )
}

pub fn blur_gray(image: &GrayImage, radius: f32) -> GrayImage {
    let size = [image.width(), image.height()];
    attempt(count(size), NEIGHBORHOOD_MIN, |gpu| {
        let bytes = gpu.separable(image.as_raw(), size, size, 1, Some(radius))?;
        Ok(GrayImage::from_raw(
            size[0],
            size[1],
            bytes.as_chunks::<4>().0.iter().map(|p| p[0]).collect(),
        )
        .unwrap())
    })
    .unwrap_or_else(|| {
        if super::cancelled() {
            image.clone()
        } else {
            image::imageops::blur(image, radius)
        }
    })
}

pub fn resize_gray(image: &GrayImage, width: u32, height: u32) -> GrayImage {
    let size = [image.width(), image.height()];
    attempt(
        count(size).max(u64::from(width) * u64::from(height)),
        NEIGHBORHOOD_MIN,
        |gpu| {
            let bytes = gpu.separable(image.as_raw(), size, [width, height], 1, None)?;
            Ok(GrayImage::from_raw(
                width,
                height,
                bytes.as_chunks::<4>().0.iter().map(|p| p[0]).collect(),
            )
            .unwrap())
        },
    )
    .unwrap_or_else(|| {
        image::imageops::resize(image, width, height, image::imageops::FilterType::Triangle)
    })
}

pub fn resize_rgb(image: &Rgb32FImage, width: u32, height: u32) -> Rgb32FImage {
    let size = [image.width(), image.height()];
    attempt(
        count(size).max(u64::from(width) * u64::from(height)),
        NEIGHBORHOOD_MIN,
        |gpu| {
            let bytes = gpu.separable(
                bytemuck::cast_slice(image.as_raw()),
                size,
                [width, height],
                2,
                None,
            )?;
            Ok(Rgb32FImage::from_raw(
                width,
                height,
                bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|p| f32::from_ne_bytes(*p))
                    .collect(),
            )
            .unwrap())
        },
    )
    .unwrap_or_else(|| {
        image::imageops::resize(image, width, height, image::imageops::FilterType::Triangle)
    })
}

impl Processor {
    pub(super) fn simple(
        &self,
        entry: &'static str,
        shader: &str,
        input: &[u8],
        auxiliary: &[u8],
        config: &[[f32; 4]],
        size: [u32; 2],
    ) -> Result<Vec<u8>> {
        let source = self.buffer(input)?;
        let aux = self.buffer(auxiliary)?;
        let result = self.empty(count(size) * 4)?;
        let mut encoder = self.encoder();
        self.dispatch(
            &mut encoder,
            entry,
            shader,
            [&source, &aux, &result],
            config,
            size,
        )?;
        self.read(encoder, &result, count(size) * 4)
    }

    pub(super) fn adjustment(
        &self,
        image: &RgbaImage,
        adjustment: &Adjustment,
        transform: Transform,
        selection: Option<&GrayImage>,
        noise_coordinates: bool,
    ) -> Result<RgbaImage> {
        let mut layer = Layer::blank("Adjustment", image.width(), image.height());
        layer.adjustment = Some(adjustment.clone());
        let document = Document::new(image.width(), image.height())?;
        let params = super::parameters(&document, &layer, [image.width(), image.height()]);
        let mut config = vec![[image.width() as f32, image.height() as f32, 0.0, 0.0]];
        config.extend(transform_config(transform));
        config.push([
            selection.map_or(0, GrayImage::width) as f32,
            selection.map_or(0, GrayImage::height) as f32,
            if noise_coordinates { 1.0 } else { 0.0 },
            0.0,
        ]);
        config.extend_from_slice(bytemuck::cast_slice(bytemuck::bytes_of(&params)));
        let selection = padded(selection.map_or(&[], |image| image.as_raw()));
        Ok(RgbaImage::from_raw(
            image.width(),
            image.height(),
            self.simple(
                "adjust_pixels",
                super::raster::adjust_shader(),
                image.as_raw(),
                &selection,
                &config,
                [image.width(), image.height()],
            )?,
        )
        .unwrap())
    }

    pub(super) fn blur_passes(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        input: &wgpu::Buffer,
        scratch: &wgpu::Buffer,
        output: &wgpu::Buffer,
        size: [u32; 2],
        sigma: f32,
    ) -> Result<()> {
        let possible = (((sigma - 0.8) / 0.3 + 1.0) * 2.0 + 1.0).max(3.0) as u32;
        let length = possible | 1;
        ensure!(length <= 4095, "Gaussian kernel exceeds GPU work budget");
        let mut kernel: Vec<f32> = (0..length)
            .map(|i| {
                (-0.5 * ((i as f32 - (length / 2) as f32) / sigma).powi(2)).exp()
                    / (std::f32::consts::TAU.sqrt() * sigma)
            })
            .collect();
        let scale = 1.0 / kernel.iter().sum::<f32>();
        kernel.iter_mut().for_each(|weight| *weight *= scale);
        for (source, target, direction) in
            [(input, scratch, [1.0, 0.0]), (scratch, output, [0.0, 1.0])]
        {
            let mut config = vec![
                [size[0] as f32, size[1] as f32, 0.0, 0.0],
                [(length / 2) as f32, direction[0], direction[1], 0.0],
            ];
            config.extend(kernel.iter().map(|w| [*w, 0.0, 0.0, 0.0]));
            self.dispatch(
                encoder,
                "gaussian",
                RASTER,
                [source, source, target],
                &config,
                size,
            )?;
        }
        Ok(())
    }

    pub(super) fn separable(
        &self,
        input: &[u8],
        size: [u32; 2],
        target: [u32; 2],
        format: u32,
        sigma: Option<f32>,
    ) -> Result<Vec<u8>> {
        ensure!(size.iter().chain(&target).all(|&n| n > 0), "Empty image");
        let bytes = padded(input);
        let source = self.buffer(&bytes)?;
        let decoded = self.empty(count(size) * 16)?;
        let scratch_size = if sigma.is_some() {
            size
        } else {
            [size[0], target[1]]
        };
        let scratch = self.empty(count(scratch_size) * 16)?;
        let filtered = self.empty(count(target) * 16)?;
        let channels = if format == 2 { 3 } else { 1 };
        let output = self.empty(count(target) * 4 * channels)?;
        let mut encoder = self.encoder();
        let mode = [
            format as f32,
            if sigma.is_some() { 1.0 } else { 0.0 },
            0.0,
            0.0,
        ];
        self.dispatch(
            &mut encoder,
            "decode_pixels",
            RASTER,
            [&source, &source, &decoded],
            &[[size[0] as f32, size[1] as f32, 0.0, 0.0], mode],
            size,
        )?;
        if let Some(sigma) = sigma {
            self.blur_passes(&mut encoder, &decoded, &scratch, &filtered, size, sigma)?;
        } else {
            for (source, output, from, to, horizontal) in [
                (&decoded, &scratch, size, scratch_size, false),
                (&scratch, &filtered, scratch_size, target, true),
            ] {
                let config = resample_config(from, to, horizontal, format == 0);
                self.dispatch(
                    &mut encoder,
                    "resample",
                    RASTER,
                    [source, source, output],
                    &config,
                    to,
                )?;
            }
        }
        self.dispatch(
            &mut encoder,
            "encode_pixels",
            RASTER,
            [&filtered, &filtered, &output],
            &[[target[0] as f32, target[1] as f32, 0.0, 0.0], mode],
            target,
        )?;
        self.read(encoder, &output, count(target) * 4 * channels)
    }
}

pub(super) fn padded(bytes: &[u8]) -> Vec<u8> {
    let mut result = bytes.to_vec();
    result.resize(bytes.len().max(16).div_ceil(4) * 4, 0);
    result
}

pub(super) fn transform_config(transform: Transform) -> [[f32; 4]; 5] {
    let (sin, cos) = transform.rotation.to_radians().sin_cos();
    let matrix = transform
        .warp
        .and_then(crate::geometry::Homography::from_quad)
        .map_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], |h| h.0);
    [
        [transform.x, transform.y, transform.width, transform.height],
        [
            cos,
            sin,
            if transform.flip_x { -1.0 } else { 1.0 },
            if transform.flip_y { -1.0 } else { 1.0 },
        ],
        [matrix[0][0], matrix[0][1], matrix[0][2], 0.0],
        [matrix[1][0], matrix[1][1], matrix[1][2], 0.0],
        [matrix[2][0], matrix[2][1], matrix[2][2], 0.0],
    ]
}

fn resample_config(
    source: [u32; 2],
    target: [u32; 2],
    horizontal: bool,
    lanczos: bool,
) -> Vec<[f32; 4]> {
    let axis = if horizontal { 0 } else { 1 };
    let length = target[axis];
    let ratio = source[axis] as f32 / length as f32;
    let scale = ratio.max(1.0);
    let support = if lanczos { 3.0 } else { 1.0 } * scale;
    let mut config = vec![[0.0; 4]; length as usize + 2];
    config[0] = [
        source[0] as f32,
        source[1] as f32,
        target[0] as f32,
        target[1] as f32,
    ];
    config[1][0] = if horizontal { 1.0 } else { 0.0 };
    for i in 0..length {
        let center = (i as f32 + 0.5) * ratio;
        let left = ((center - support).floor() as i64).clamp(0, source[axis] as i64 - 1) as u32;
        let right =
            ((center + support).ceil() as i64).clamp(left as i64 + 1, source[axis] as i64) as u32;
        let mut weights: Vec<f32> = (left..right)
            .map(|j| {
                let x = (j as f32 - center + 0.5) / scale;
                if lanczos {
                    if x.abs() < 3.0 {
                        sinc(x) * sinc(x / 3.0)
                    } else {
                        0.0
                    }
                } else {
                    (1.0 - x.abs()).max(0.0)
                }
            })
            .collect();
        let sum = weights.iter().sum::<f32>();
        weights.iter_mut().for_each(|w| *w /= sum);
        config[i as usize + 2] = [left as f32, (right - left) as f32, config.len() as f32, 0.0];
        config.extend(weights.into_iter().map(|w| [w, 0.0, 0.0, 0.0]));
    }
    config
}
fn sinc(x: f32) -> f32 {
    if x == 0.0 {
        1.0
    } else {
        let x = x * std::f32::consts::PI;
        x.sin() / x
    }
}

/// Destructive adjustments read curve and color settings directly from storage.
/// Copying the entire curve table into every invocation spills private memory on
/// some drivers, costing more than the adjustment itself.
pub(super) fn adjust_shader() -> &'static str {
    static SHADER: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
        let math = include_str!("adjustments.wgsl").replace("params.", "settings.params.");
        let bindings=include_str!("buffers.wgsl").replace("@group(0) @binding(3) var<storage, read> config: array<vec4<f32>>;", "struct AdjustmentConfig { frame: array<vec4<f32>,7>, params: Parameters, }\n@group(0) @binding(3) var<storage,read> settings: AdjustmentConfig;");
        let coordinates = include_str!("coordinates.wgsl").replace("config", "settings.frame");
        let entry = include_str!("raster_adjust.wgsl").replace("config", "settings.frame");
        format!("{math}\n{bindings}\n{coordinates}\n{entry}")
    });
    &SHADER
}
