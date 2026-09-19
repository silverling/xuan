use super::{
    Processor,
    processor::attempt,
    raster::{padded, transform_config},
};
use crate::document::{Document, Layer, Transform};
use anyhow::Result;

struct Coverage {
    config: Vec<[f32; 4]>,
    pixels: Vec<u8>,
}
impl Coverage {
    fn new(document: &Document, layer: &Layer, size: [u32; 2], mode: CoverageMode) -> Self {
        let stride = size[0].div_ceil(256) * 256;
        let mut coverage = Self {
            config: vec![
                [
                    size[0] as f32,
                    size[1] as f32,
                    document.width as f32,
                    document.height as f32,
                ],
                [1.0, 0.0, stride as f32, 0.0],
            ],
            pixels: Vec::new(),
        };
        if mode == CoverageMode::Composite {
            coverage.config[1][0] = if layer.visible { 1.0 } else { 0.0 };
            let mut parent = layer.parent;
            for _ in 0..64 {
                let Some(group) = parent.and_then(|id| document.layers.iter().find(|l| l.id == id))
                else {
                    break;
                };
                coverage.config[1][0] *= if group.visible { group.opacity } else { 0.0 };
                coverage.mask(group);
                parent = group.parent;
            }
        }
        if mode != CoverageMode::Alpha {
            coverage.mask(layer);
        }
        let mut clip = if mode == CoverageMode::Alpha {
            Some(layer.id)
        } else if mode == CoverageMode::Composite {
            layer.clip_to
        } else {
            None
        };
        for depth in 0..258 {
            let Some(source) = clip.and_then(|id| document.layers.iter().find(|l| l.id == id))
            else {
                break;
            };
            if depth > 256 {
                coverage.config[1][0] = 0.0;
                break;
            }
            coverage.config[1][0] *= source.opacity;
            if let Some(pixels) = &source.pixels {
                coverage.source(
                    [pixels.width(), pixels.height()],
                    pixels.as_raw(),
                    source.transform,
                    true,
                );
            } else {
                coverage.config[1][0] = 0.0;
            }
            coverage.mask(source);
            clip = source.clip_to;
        }
        coverage
    }
    fn encode(
        &self,
        gpu: &Processor,
        size: [u32; 2],
    ) -> Result<(wgpu::CommandEncoder, wgpu::Buffer)> {
        let stride = self.config[1][2] as u32;
        let pixels = gpu.buffer(&self.pixels)?;
        let result = gpu.empty(u64::from(stride) * u64::from(size[1]))?;
        let mut encoder = gpu.encoder();
        gpu.dispatch(
            &mut encoder,
            "layer_coverage",
            include_str!("coverage.wgsl"),
            [&pixels, &pixels, &result],
            &self.config,
            [stride / 4, size[1]],
        )?;
        Ok((encoder, result))
    }

    fn source(&mut self, size: [u32; 2], bytes: &[u8], transform: Transform, rgba: bool) {
        self.config.push([
            size[0] as f32,
            size[1] as f32,
            f32::from_bits((self.pixels.len() / 4) as u32),
            if rgba { 1.0 } else { 0.0 },
        ]);
        let mut transform_rows = transform_config(transform);
        let inverse = transform
            .warp
            .and_then(crate::geometry::Homography::from_quad)
            .and_then(|h| h.inverse())
            .map_or([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], |h| h.0);
        for (row, matrix) in transform_rows[2..].iter_mut().zip(inverse) {
            *row = [matrix[0], matrix[1], matrix[2], 0.0];
        }
        self.config.extend(transform_rows);
        self.pixels.extend(padded(bytes));
        self.config[1][1] += 1.0;
    }
    fn mask(&mut self, layer: &Layer) {
        if let Some(mask) = layer.mask.as_ref().filter(|m| m.enabled) {
            self.source(
                [mask.pixels.width(), mask.pixels.height()],
                mask.pixels.as_raw(),
                mask.placement.unwrap_or(layer.transform),
                false,
            );
        }
    }
}
impl Processor {
    pub(super) fn coverage(
        &self,
        document: &Document,
        layer: &Layer,
        size: [u32; 2],
    ) -> Result<wgpu::Texture> {
        let stride = size[0].div_ceil(256) * 256;
        let coverage = Coverage::new(document, layer, size, CoverageMode::Composite);
        let (mut encoder, result) = coverage.encode(self, size)?;
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("layer coverage"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        encoder.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo {
                buffer: &result,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(size[1]),
                },
            },
            texture.as_image_copy(),
            wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        Ok(texture)
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum CoverageMode {
    Composite,
    Mask,
    Alpha,
}

/// Materialize masks for selection, clipping bake and background removal. An
/// optional transform maps the output grid into a layer's source coordinates.
pub(crate) fn coverage_image(
    document: &Document,
    layer: &Layer,
    size: [u32; 2],
    mode: CoverageMode,
    transform: Option<Transform>,
) -> Option<image::GrayImage> {
    attempt(u64::from(size[0]) * u64::from(size[1]), 65_536, |gpu| {
        let mut coverage = Coverage::new(document, layer, size, mode);
        if let Some(transform) = transform {
            coverage.config[1][3] = coverage.config.len() as f32;
            coverage.config.extend(transform_config(transform));
        }
        let (encoder, result) = coverage.encode(gpu, size)?;
        let stride = coverage.config[1][2] as u64;
        let bytes = gpu.read(encoder, &result, stride * u64::from(size[1]))?;
        let mut image = image::GrayImage::new(size[0], size[1]);
        for (source, row) in bytes
            .chunks_exact(stride as usize)
            .zip(image.as_mut().chunks_exact_mut(size[0] as usize))
        {
            row.copy_from_slice(&source[..row.len()]);
        }
        Ok(image)
    })
}

pub(crate) fn bake_alpha(
    document: &Document,
    base: &Layer,
    pixels: &image::RgbaImage,
    transform: Transform,
) -> Option<image::RgbaImage> {
    let size = [pixels.width(), pixels.height()];
    bake(
        document,
        base,
        size,
        CoverageMode::Alpha,
        transform,
        pixels.as_raw(),
        1,
    )
    .map(|bytes| image::RgbaImage::from_raw(size[0], size[1], bytes).unwrap())
}
pub(crate) fn bake_mask(layer: &Layer, mask: &image::GrayImage) -> Option<image::GrayImage> {
    if layer.mask.as_ref().is_none_or(|m| !m.enabled) {
        return Some(mask.clone());
    }
    let size = [mask.width(), mask.height()];
    let document = Document::new(size[0], size[1]).ok()?;
    bake(
        &document,
        layer,
        size,
        CoverageMode::Mask,
        layer.transform,
        mask.as_raw(),
        2,
    )
    .map(|bytes| super::paint::gray(size, bytes))
}
fn bake(
    document: &Document,
    base: &Layer,
    size: [u32; 2],
    mode: CoverageMode,
    transform: Transform,
    original: &[u8],
    format: u32,
) -> Option<Vec<u8>> {
    attempt(u64::from(size[0]) * u64::from(size[1]), 65_536, |gpu| {
        let mut coverage = Coverage::new(document, base, size, mode);
        coverage.config[1][2] = size[0] as f32 * 4.0;
        coverage.config[1][3] = coverage.config.len() as f32;
        let mut mapping = transform_config(transform);
        mapping[2][3] = format as f32;
        coverage.config.extend(mapping);
        let source = gpu.buffer(&coverage.pixels)?;
        let original = gpu.buffer(&padded(original))?;
        let bytes = u64::from(size[0]) * u64::from(size[1]) * 4;
        let output = gpu.empty(bytes)?;
        let mut encoder = gpu.encoder();
        gpu.dispatch(
            &mut encoder,
            "layer_coverage",
            include_str!("coverage.wgsl"),
            [&source, &original, &output],
            &coverage.config,
            size,
        )?;
        gpu.read(encoder, &output, bytes)
    })
}
