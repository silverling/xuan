//! Resident RAW previews. Only histogram bins cross back to the CPU; display
//! pixels are copied directly into immutable textures for the UI to register.
use std::sync::{
    Arc, Weak,
    atomic::{AtomicBool, Ordering},
};

use anyhow::{Result, ensure};

use super::{
    Analysis, Processor,
    processor::attempt,
    raw::{RawBuffers, SHADER, crop, settings},
};
use crate::raw::{DecodedRaw, DevelopSettings};

pub struct RawPreview {
    pub texture: wgpu::Texture,
    pub warnings: Option<wgpu::Texture>,
    pub analysis: Analysis,
}

struct Entry {
    source: Weak<DecodedRaw>,
    work: RawBuffers,
    output: Option<wgpu::Buffer>,
    analysis: Option<wgpu::Buffer>,
}

/// Each session retains at most two inputs, allowing interactive proxies to
/// alternate with full detail without uploading the camera data on every edit.
#[derive(Default)]
pub struct RawPreviewRenderer {
    processor: Option<Arc<Processor>>,
    entries: Vec<Entry>,
}

impl RawPreviewRenderer {
    pub fn clear(&mut self) {
        self.entries.clear();
        self.processor = None;
    }

    pub fn render(
        &mut self,
        raw: &Arc<DecodedRaw>,
        s: &DevelopSettings,
        warnings: bool,
        cancel: &AtomicBool,
    ) -> Result<Option<RawPreview>> {
        s.validate()?;
        ensure!(!cancel.load(Ordering::Relaxed), "RAW preview cancelled");
        let count = u64::from(raw.camera.width()) * u64::from(raw.camera.height());
        let result = attempt(count, 16_384, |gpu| {
            if self
                .processor
                .as_ref()
                .is_none_or(|previous| !std::ptr::eq(previous.as_ref(), gpu))
            {
                self.clear();
                self.processor = super::current();
            }
            if let Some(index) = self
                .entries
                .iter()
                .position(|entry| entry.source.ptr_eq(&Arc::downgrade(raw)))
            {
                let entry = self.entries.remove(index);
                self.entries.push(entry);
            } else {
                if self.entries.len() == 2 {
                    self.entries.remove(0);
                }
                self.entries.push(Entry {
                    source: Arc::downgrade(raw),
                    work: RawBuffers::new(gpu, raw)?,
                    output: None,
                    analysis: None,
                });
            }
            let entry = self.entries.last_mut().unwrap();
            gpu.raw_preview(raw, s, warnings, entry, cancel)
        });
        ensure!(!cancel.load(Ordering::Relaxed), "RAW preview cancelled");
        Ok(result)
    }
}

fn buffer<'a>(
    gpu: &Processor,
    slot: &'a mut Option<wgpu::Buffer>,
    bytes: u64,
) -> Result<&'a wgpu::Buffer> {
    if slot.as_ref().is_none_or(|buffer| buffer.size() < bytes) {
        *slot = Some(gpu.empty(bytes)?);
    }
    Ok(slot.as_ref().unwrap())
}

impl Processor {
    fn raw_preview(
        &self,
        raw: &DecodedRaw,
        s: &DevelopSettings,
        warnings: bool,
        entry: &mut Entry,
        cancel: &AtomicBool,
    ) -> Result<RawPreview> {
        let size = [raw.camera.width(), raw.camera.height()];
        let [left, top, right, bottom] = crop(s, size);
        let target = [right - left, bottom - top];
        ensure!(
            target[0].max(target[1]) <= self.device.limits().max_texture_dimension_2d,
            "RAW preview exceeds GPU texture limits"
        );
        let stride = (target[0] * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let bytes = u64::from(stride) * u64::from(target[1]);
        let wb = crate::raw::white_balance(raw, s);
        let (mut encoder, current) = self.raw_passes(raw, s, wb, &mut entry.work, cancel)?;
        let output = buffer(self, &mut entry.output, bytes)?;
        let mut config = settings(raw, s, wb, 8);
        config[13] = [left as f32, top as f32, target[0] as f32, target[1] as f32];
        config[14] = [8.0, (stride / 4) as f32, 1.0, 0.0];
        self.dispatch(
            &mut encoder,
            "raw_encode",
            SHADER,
            [&entry.work.pixels[current], &entry.work.source, output],
            &config,
            target,
        )?;
        let texture = self.preview_texture(&mut encoder, output, 0, target, stride);
        let analysis = buffer(
            self,
            &mut entry.analysis,
            4112 + if warnings { bytes } else { 0 },
        )?;
        // Histograms are atomic accumulators; reused buffers must start at zero.
        encoder.clear_buffer(analysis, 0, Some(4112));
        self.dispatch(
            &mut encoder,
            "analyze_pixels",
            include_str!("analysis.wgsl"),
            [output, output, analysis],
            &[[
                target[0] as f32,
                target[1] as f32,
                if warnings { 1.0 } else { 0.0 },
                (stride / 4) as f32,
            ]],
            super::analysis::dispatch_size(target),
        )?;
        let warnings =
            warnings.then(|| self.preview_texture(&mut encoder, analysis, 4112, target, stride));
        // Waiting in the worker also ensures the textures are ready before they
        // replace the displayed result. No full-image readback or UI GPU wait.
        let bins = self.read(encoder, analysis, 4112)?;
        ensure!(!cancel.load(Ordering::Relaxed), "RAW preview cancelled");
        Ok(RawPreview {
            texture,
            warnings,
            analysis: super::analysis::from_bins(&bins),
        })
    }

    fn preview_texture(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        buffer: &wgpu::Buffer,
        offset: u64,
        size: [u32; 2],
        stride: u32,
    ) -> wgpu::Texture {
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("RAW preview"),
            size: wgpu::Extent3d {
                width: size[0],
                height: size[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::COPY_DST
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        encoder.copy_buffer_to_texture(
            wgpu::TexelCopyBufferInfo {
                buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset,
                    bytes_per_row: Some(stride),
                    rows_per_image: None,
                },
            },
            texture.as_image_copy(),
            texture.size(),
        );
        texture
    }
}
