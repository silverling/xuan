//! wgpu compute compositor. The CPU renderer remains the export/reference path.

mod motion_blur;
pub use motion_blur::GpuMotionBlur;

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Weak},
};

use bytemuck::{Pod, Zeroable};
use image::RgbaImage;
use rayon::prelude::*;
use wgpu::util::DeviceExt;

use crate::{
    blend::BlendMode,
    document::{Adjustment, Document, Layer, Point},
    render,
};

/// Selections and layers clipped to the edited pixels need the materialized result.
/// Keep those cases on the cancellable CPU path until coverage is also GPU resident.
pub fn can_preview_motion_blur(document: &Document) -> bool {
    document.selection.is_none()
        && document.active().is_some_and(|layer| {
            layer.pixels.is_some()
                && !layer.locked
                && !layer.group
                && layer.adjustment.is_none()
                && layer.raw.is_none()
                && !document
                    .layers
                    .iter()
                    .any(|other| other.clip_to == Some(layer.id))
        })
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Parameters {
    canvas: [f32; 4],
    bounds: [f32; 4],
    rotation: [f32; 4],
    flags: [u32; 4],
    appearance: [f32; 4],
    first: [f32; 4],
    second: [f32; 4],
    points: [[f32; 4]; 128],
}

struct Source {
    // A weak reference prevents pointer reuse without forcing copies during painting.
    pixels: Weak<RgbaImage>,
    texture: wgpu::Texture,
}

pub struct GpuCompositor {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: wgpu::ComputePipeline,
    mipmap_pipeline: wgpu::ComputePipeline,
    sources: HashMap<(usize, [u32; 2]), Source>,
    size: [u32; 2],
    buffers: [wgpu::Texture; 2],
    display: wgpu::Texture,
    blank: wgpu::Texture,
    motion_blur: GpuMotionBlur,
}

impl GpuCompositor {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("xuan layer compositor"),
            source: wgpu::ShaderSource::Wgsl(include_str!("composite.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("xuan compositor"),
            layout: None,
            module: &shader,
            entry_point: Some("composite"),
            compilation_options: Default::default(),
            cache: None,
        });
        let mipmap_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("xuan preview mipmaps"),
            source: wgpu::ShaderSource::Wgsl(include_str!("mipmap.wgsl").into()),
        });
        let mipmap_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("xuan preview mipmaps"),
            layout: None,
            module: &mipmap_shader,
            entry_point: Some("downsample"),
            compilation_options: Default::default(),
            cache: None,
        });
        let buffers =
            std::array::from_fn(|_| target(&device, [1, 1], wgpu::TextureFormat::Rgba16Float, 1));
        let display = target(&device, [1, 1], wgpu::TextureFormat::Rgba8Unorm, 1);
        let blank = target(&device, [1, 1], wgpu::TextureFormat::Rgba8Unorm, 1);
        let motion_blur = GpuMotionBlur::new(device.clone(), queue.clone());
        Self {
            device,
            queue,
            pipeline,
            mipmap_pipeline,
            sources: HashMap::new(),
            size: [1, 1],
            buffers,
            display,
            blank,
            motion_blur,
        }
    }

    pub fn display_view(&self) -> wgpu::TextureView {
        self.display.create_view(&Default::default())
    }
    pub fn display_texture(&self) -> &wgpu::Texture {
        &self.display
    }

    pub fn motion_blur_worker(&self, pixels: &Arc<RgbaImage>) -> GpuMotionBlur {
        let key = (
            Arc::as_ptr(pixels) as usize,
            [pixels.width(), pixels.height()],
        );
        match self.sources.get(&key) {
            Some(source) => self
                .motion_blur
                .clone()
                .with_source(pixels, source.texture.clone()),
            None => self.motion_blur.clone(),
        }
    }

    pub fn render(&mut self, document: &Document, size: [u32; 2]) {
        self.render_with_motion_blur(document, size, None);
    }

    /// Preview distance/angle from cached source textures without changing document
    /// pixels or reading the GPU result back to the CPU.
    pub fn render_with_motion_blur(
        &mut self,
        document: &Document,
        size: [u32; 2],
        motion_blur: Option<[f32; 2]>,
    ) {
        let motion_blur = motion_blur.filter(|_| can_preview_motion_blur(document));
        if size != self.size {
            self.size = size;
            self.buffers = std::array::from_fn(|_| {
                target(&self.device, size, wgpu::TextureFormat::Rgba16Float, 1)
            });
            self.display = target(
                &self.device,
                size,
                wgpu::TextureFormat::Rgba8Unorm,
                size[0].max(size[1]).ilog2() + 1,
            );
        }
        let mut retained = HashSet::new();
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("xuan composite frame"),
            });
        {
            let view = self.buffers[0].create_view(&Default::default());
            let _clear = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("clear composition"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                occlusion_query_set: None,
                timestamp_writes: None,
            });
        }
        let mut current = 0;
        for layer in render::paint_order(document) {
            if !layer.visible || (layer.pixels.is_none() && layer.adjustment.is_none()) {
                continue;
            }
            if let Some(pixels) = &layer.pixels {
                let source_size = render::source_size(document, layer, size);
                let key = (Arc::as_ptr(pixels) as usize, source_size);
                retained.insert(key);
                self.sources.entry(key).or_insert_with(|| {
                    let scaled = ((source_size[0], source_size[1]) != pixels.dimensions())
                        .then(|| render::resize_quality(pixels, source_size[0], source_size[1]));
                    let uploaded = scaled.as_ref().unwrap_or(pixels);
                    let texture = self.device.create_texture_with_data(
                        &self.queue,
                        &wgpu::TextureDescriptor {
                            label: Some("xuan layer source"),
                            size: wgpu::Extent3d {
                                width: uploaded.width(),
                                height: uploaded.height(),
                                depth_or_array_layers: 1,
                            },
                            mip_level_count: 1,
                            sample_count: 1,
                            dimension: wgpu::TextureDimension::D2,
                            format: wgpu::TextureFormat::Rgba8Unorm,
                            usage: wgpu::TextureUsages::TEXTURE_BINDING,
                            view_formats: &[],
                        },
                        wgpu::util::TextureDataOrder::LayerMajor,
                        uploaded.as_raw(),
                    );
                    Source {
                        pixels: Arc::downgrade(pixels),
                        texture,
                    }
                });
            }
            let mut params = parameters(document, layer, size);
            let needs_coverage = layer.parent.is_some()
                || layer.mask.as_ref().is_some_and(|m| m.enabled)
                || layer.clip_to.is_some();
            let coverage = if needs_coverage {
                params.flags[2] = 1;
                let mut pixels = vec![0_u8; size[0] as usize * size[1] as usize];
                pixels
                    .par_iter_mut()
                    .enumerate()
                    .for_each(|(index, pixel)| {
                        let point = Point::new(
                            (index as u32 % size[0]) as f32 + 0.5,
                            (index as u32 / size[0]) as f32 + 0.5,
                        );
                        let point = Point::new(
                            point.x * document.width as f32 / size[0] as f32,
                            point.y * document.height as f32 / size[1] as f32,
                        );
                        let mut alpha = render::inherited_coverage(document, layer, point)
                            * render::own_mask(layer, point);
                        if let Some(source) = layer
                            .clip_to
                            .and_then(|id| document.layers.iter().find(|l| l.id == id))
                        {
                            alpha *= render::layer_alpha(document, source, point, 0);
                        }
                        *pixel = (alpha * 255.0).round() as u8;
                    });
                Some(self.device.create_texture_with_data(
                    &self.queue,
                    &wgpu::TextureDescriptor {
                        label: Some("xuan mask coverage"),
                        size: wgpu::Extent3d {
                            width: size[0],
                            height: size[1],
                            depth_or_array_layers: 1,
                        },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format: wgpu::TextureFormat::R8Unorm,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING,
                        view_formats: &[],
                    },
                    wgpu::util::TextureDataOrder::LayerMajor,
                    &pixels,
                ))
            } else {
                None
            };
            let source = layer
                .pixels
                .as_ref()
                .map(|pixels| {
                    &self.sources[&(
                        Arc::as_ptr(pixels) as usize,
                        render::source_size(document, layer, size),
                    )]
                        .texture
                })
                .unwrap_or(&self.blank);
            if document.active == Some(layer.id)
                && let Some([distance, angle]) = motion_blur
                && let Some(pixels) = &layer.pixels
            {
                let (sin, cos) = angle.to_radians().sin_cos();
                let scale = (source.width() as f32 / pixels.width() as f32)
                    .max(source.height() as f32 / pixels.height() as f32);
                params.appearance[1] = (distance * scale).ceil().clamp(1.0, 256.0);
                params.appearance[2] = distance * cos / pixels.width() as f32;
                params.appearance[3] = distance * sin / pixels.height() as f32;
            }
            self.dispatch(
                &mut encoder,
                current,
                source,
                coverage.as_ref().unwrap_or(&self.blank),
                &params,
            );
            current = 1 - current;
        }
        let mut params = Parameters::zeroed();
        params.canvas = [
            size[0] as f32,
            size[1] as f32,
            document.width as f32,
            document.height as f32,
        ];
        params.flags[1] = 100;
        self.dispatch(&mut encoder, current, &self.blank, &self.blank, &params);
        self.generate_mipmaps(&mut encoder);
        self.queue.submit([encoder.finish()]);
        self.sources
            .retain(|key, source| retained.contains(key) && source.pixels.strong_count() > 0);
    }

    fn generate_mipmaps(&self, encoder: &mut wgpu::CommandEncoder) {
        for level in 1..self.display.mip_level_count() {
            let views = [level - 1, level].map(|level| {
                self.display.create_view(&wgpu::TextureViewDescriptor {
                    base_mip_level: level,
                    mip_level_count: Some(1),
                    ..Default::default()
                })
            });
            let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("xuan preview mipmap inputs"),
                layout: &self.mipmap_pipeline.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&views[0]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::TextureView(&views[1]),
                    },
                ],
            });
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("xuan preview mipmap"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.mipmap_pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(
                (self.size[0] >> level).max(1).div_ceil(8),
                (self.size[1] >> level).max(1).div_ceil(8),
                1,
            );
        }
    }

    fn dispatch(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        current: usize,
        source: &wgpu::Texture,
        coverage: &wgpu::Texture,
        params: &Parameters,
    ) {
        let uniform = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("xuan layer parameters"),
                contents: bytemuck::bytes_of(params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let views = [
            &self.buffers[current],
            source,
            coverage,
            &self.buffers[1 - current],
            &self.display,
        ]
        .map(|texture| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                mip_level_count: Some(1),
                ..Default::default()
            })
        });
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("xuan composite inputs"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&views[0]),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&views[1]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&views[2]),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::TextureView(&views[3]),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 5,
                    resource: wgpu::BindingResource::TextureView(&views[4]),
                },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("xuan layer"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(self.size[0].div_ceil(8), self.size[1].div_ceil(8), 1);
    }
}

fn target(
    device: &wgpu::Device,
    size: [u32; 2],
    format: wgpu::TextureFormat,
    mip_level_count: u32,
) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("xuan canvas target"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

fn parameters(document: &Document, layer: &Layer, size: [u32; 2]) -> Parameters {
    let mut p = Parameters::zeroed();
    let t = layer.transform;
    let (sin, cos) = t.rotation.to_radians().sin_cos();
    p.canvas = [
        size[0] as f32,
        size[1] as f32,
        document.width as f32,
        document.height as f32,
    ];
    p.bounds = [t.x, t.y, t.width, t.height];
    p.rotation = [
        cos,
        sin,
        if t.flip_x { -1.0 } else { 1.0 },
        if t.flip_y { -1.0 } else { 1.0 },
    ];
    p.flags[0] = BlendMode::ALL
        .iter()
        .position(|b| *b == layer.blend)
        .unwrap_or(0) as u32;
    p.appearance[0] = layer.opacity;
    if let Some(inverse) = t
        .warp
        .and_then(crate::geometry::Homography::from_quad)
        .and_then(|h| h.inverse())
    {
        p.flags[3] = 1;
        for (index, row) in inverse.0.iter().enumerate() {
            p.points[index] = [row[0], row[1], row[2], 0.0];
        }
    }
    if let Some(adjustment) = &layer.adjustment {
        match adjustment {
            Adjustment::LevelsChannels { ranges } => {
                p.flags[1] = 8;
                for (i, range) in ranges.iter().enumerate() {
                    p.points[i * 2] = [range[0], range[1], range[2], range[3]];
                    p.points[i * 2 + 1][0] = range[4];
                }
            }
            Adjustment::CurvesChannels { channels } => {
                p.flags[1] = 9;
                for (i, points) in channels.iter().enumerate() {
                    p.first[i] = points.len() as f32;
                    for (j, point) in points.iter().enumerate() {
                        p.points[i * 32 + j] = [point.x, point.y, 0.0, 0.0];
                    }
                }
            }
            Adjustment::HueRanges { settings } => {
                p.flags[1] = 10;
                p.first = [
                    settings.range as f32,
                    if settings.colorize { 1.0 } else { 0.0 },
                    if settings.invert_range { 1.0 } else { 0.0 },
                    0.0,
                ];
                for i in 0..7 {
                    p.points[i] = [
                        settings.adjustments[i][0],
                        settings.adjustments[i][1],
                        settings.adjustments[i][2],
                        0.0,
                    ];
                    p.points[i + 7] = settings.bands[i];
                }
            }
            Adjustment::HueSaturation {
                hue,
                saturation,
                lightness,
                colorize,
            } => {
                p.flags[1] = 1;
                p.first = [
                    *hue,
                    *saturation,
                    *lightness,
                    if *colorize { 1.0 } else { 0.0 },
                ];
            }
            Adjustment::Levels {
                black,
                gamma,
                white,
                output_black,
                output_white,
            } => {
                p.flags[1] = 2;
                p.first = [*black, *gamma, *white, *output_black];
                p.second[0] = *output_white;
            }
            Adjustment::Curves { points } => {
                p.flags[1] = 3;
                p.flags[3] = points.len() as u32;
                for (target, point) in p.points.iter_mut().zip(points) {
                    *target = [point.x, point.y, 0.0, 0.0];
                }
            }
            Adjustment::Exposure {
                exposure,
                offset,
                gamma,
            } => {
                p.flags[1] = 4;
                p.first = [*exposure, *offset, *gamma, 0.0];
            }
            Adjustment::GradientMap {
                shadows,
                highlights,
            } => {
                p.flags[1] = 5;
                p.first = shadows.map(|v| v as f32 / 255.0);
                p.second = highlights.map(|v| v as f32 / 255.0);
            }
            Adjustment::FilmGrain {
                amount,
                size,
                roughness,
                seed,
            } => {
                p.flags[1] = 11;
                p.first = [*amount, *size, *roughness, f32::from_bits(*seed)];
            }
            Adjustment::Grain {
                amount,
                monochrome,
                seed,
            } => {
                p.flags[1] = 6;
                p.first = [
                    *amount,
                    if *monochrome { 1.0 } else { 0.0 },
                    f32::from_bits(*seed),
                    0.0,
                ];
            }
            Adjustment::Invert => p.flags[1] = 7,
        }
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn motion_blur_preview_rejects_edits_that_need_cpu_coverage() {
        let mut document = Document::new(8, 8).unwrap();
        document.insert(Layer::image("Pixels", RgbaImage::new(8, 8)));
        assert!(can_preview_motion_blur(&document));
        document.active_mut().unwrap().locked = true;
        assert!(!can_preview_motion_blur(&document));
        document.active_mut().unwrap().locked = false;
        document.selection = Some(Arc::new(image::GrayImage::new(8, 8)));
        assert!(!can_preview_motion_blur(&document));
        document.selection = None;
        let mut clipped = Layer::blank("Clipped", 8, 8);
        clipped.clip_to = document.active;
        document.layers.push(clipped);
        assert!(!can_preview_motion_blur(&document));
    }

    #[test]
    #[ignore = "requires a Vulkan or OpenGL compute adapter; run explicitly for native verification"]
    fn motion_blur_preview_matches_cpu_and_reuses_source_texture() {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&Default::default())).unwrap();
        let mut compositor = GpuCompositor::new(device, queue);
        let mut document = Document::new(64, 48).unwrap();
        let mut layer = Layer::image(
            "Blurred",
            RgbaImage::from_fn(24, 16, |x, y| {
                Rgba([
                    255,
                    (y * 13) as u8,
                    (x * 9) as u8,
                    if x % 3 == 0 { 0 } else { 210 },
                ])
            }),
        );
        layer.transform.x = 20.0;
        layer.transform.y = 16.0;
        document.insert(layer);
        compositor.render(&document, [64, 48]);
        let pixels = document.active().unwrap().pixels.clone().unwrap();
        let key = (Arc::as_ptr(&pixels) as usize, [24, 16]);
        let texture = compositor.sources[&key].texture.clone();
        for distance in [1.0, 4.0, 15.0, 23.7, 200.0] {
            for angle in [0.0, 35.0, -90.0] {
                compositor.render_with_motion_blur(&document, [64, 48], Some([distance, angle]));
                assert_eq!(compositor.sources[&key].texture, texture);
                assert!(Arc::ptr_eq(
                    &pixels,
                    document.active().unwrap().pixels.as_ref().unwrap()
                ));
                let mut expected = document.clone();
                crate::effects::apply_filter(
                    &mut expected,
                    &crate::effects::Filter::MotionBlur { distance, angle },
                    false,
                )
                .unwrap();
                compare(
                    &expected,
                    &readback(&compositor),
                    &format!("Motion Blur {distance}, {angle}"),
                );
            }
        }
        // Expanded blur still uses the original mask placement and layer blend.
        for rotation in [0.0, 90.0] {
            let layer = document.active_mut().unwrap();
            layer.transform.rotation = rotation;
            layer.transform.flip_x = true;
            layer.opacity = 0.73;
            layer.mask = Some(crate::document::Mask {
                pixels: Arc::new(image::GrayImage::from_fn(24, 16, |x, _| {
                    image::Luma([(x * 11) as u8])
                })),
                ..crate::document::Mask::white()
            });
            compositor.render_with_motion_blur(&document, [64, 48], Some([15.0, 35.0]));
            let mut expected = document.clone();
            crate::effects::apply_filter(
                &mut expected,
                &crate::effects::Filter::MotionBlur {
                    distance: 15.0,
                    angle: 35.0,
                },
                false,
            )
            .unwrap();
            compare(
                &expected,
                &readback(&compositor),
                "Motion Blur with transformed mask",
            );
        }
        compositor.render(&document, [64, 48]);
        compare(&document, &readback(&compositor), "Preview off");
    }

    fn readback(compositor: &GpuCompositor) -> Vec<u8> {
        readback_mip(compositor, 0)
    }

    fn readback_mip(compositor: &GpuCompositor, level: u32) -> Vec<u8> {
        let [width, height] = compositor.size.map(|side| (side >> level).max(1));
        let stride = (width * 4).div_ceil(256) * 256;
        let buffer = compositor.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("compositor verification"),
            size: (stride * height) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = compositor
            .device
            .create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                mip_level: level,
                ..compositor.display.as_image_copy()
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        compositor.queue.submit([encoder.finish()]);
        let (send, receive) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                send.send(result).unwrap();
            });
        compositor
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        receive.recv().unwrap().unwrap();
        let mapped = buffer.slice(..).get_mapped_range();
        mapped
            .chunks_exact(stride as usize)
            .flat_map(|row| row[..width as usize * 4].iter().copied())
            .collect()
    }

    #[test]
    #[ignore = "requires a Vulkan or OpenGL compute adapter; run explicitly for native verification"]
    fn preview_mipmaps_filter_detail_and_preserve_transparency_and_edges() {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let mut compositor = GpuCompositor::new(device, queue);
        let mut document = Document::new(64, 32).unwrap();
        document.layers = vec![Layer::image(
            "Fine detail",
            RgbaImage::from_fn(64, 32, |x, y| {
                let value = if (x + y) % 2 == 0 { 0 } else { 255 };
                Rgba([value, value, value, 255])
            }),
        )];
        compositor.render(&document, [64, 32]);
        compare(&document, &readback(&compositor), "full resolution");
        assert_eq!(compositor.display.mip_level_count(), 7);
        for level in 1..compositor.display.mip_level_count() {
            for pixel in readback_mip(&compositor, level).as_chunks::<4>().0 {
                assert!(pixel[..3].iter().all(|v| (127..=128).contains(v)));
                assert_eq!(pixel[3], 255);
            }
        }

        for (width, height) in [(3, 5), (1, 7), (7, 1), (1, 1)] {
            document = Document::new(width, height).unwrap();
            document.layers = vec![Layer::image(
                "Transparent edge",
                RgbaImage::from_fn(width, height, |x, y| {
                    if x == width - 1 && y == height - 1 {
                        Rgba([255, 0, 0, 255])
                    } else {
                        Rgba([0, 0, 255, 0])
                    }
                }),
            )];
            compositor.render(&document, [width, height]);
            let level = compositor.display.mip_level_count() - 1;
            let pixel = readback_mip(&compositor, level);
            let coverage = (255.0 / (width * height) as f32).round() as u8;
            assert!(
                pixel[0].abs_diff(coverage) <= 1,
                "{width} x {height}: {pixel:?}"
            );
            assert!(
                pixel[3].abs_diff(coverage) <= 1,
                "{width} x {height}: {pixel:?}"
            );
            assert_eq!(&pixel[1..3], &[0, 0]);

            // Editing must update every level, including the one used when fit to screen.
            let pixels = document.layers[0].pixels.as_mut().unwrap();
            Arc::make_mut(pixels).fill(255);
            compositor.render(&document, [width, height]);
            assert_eq!(readback_mip(&compositor, level), vec![255; 4]);
        }
    }

    #[test]
    #[ignore = "requires a Vulkan or OpenGL compute adapter; run explicitly for native verification"]
    fn gpu_matches_cpu_blending_adjustments_and_masks() {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
        let adapter =
            pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
                .unwrap();
        let (device, queue) =
            pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
        let mut compositor = GpuCompositor::new(device, queue);
        let mut document = Document::new(12, 10).unwrap();
        document.layers = vec![
            Layer::image(
                "Base",
                RgbaImage::from_fn(12, 10, |x, y| Rgba([x as u8 * 20, y as u8 * 20, 105, 180])),
            ),
            Layer::image(
                "Top",
                RgbaImage::from_fn(12, 10, |x, y| Rgba([165, y as u8 * 15, x as u8 * 20, 140])),
            ),
        ];
        document.layers[1].transform.rotation = 17.0;
        document.layers[1].transform.warp = Some([
            Point::new(0.1, 0.1),
            Point::new(0.95, 0.0),
            Point::new(1.0, 0.85),
            Point::new(0.0, 1.0),
        ]);
        document.layers[1].opacity = 0.73;
        for mode in BlendMode::ALL {
            document.layers[1].blend = mode;
            compositor.render(&document, [12, 10]);
            compare(&document, &readback(&compositor), mode.name());
        }
        document.layers[1].mask = Some(crate::document::Mask {
            pixels: Arc::new(image::GrayImage::from_pixel(1, 1, image::Luma([137]))),
            ..crate::document::Mask::white()
        });
        let mut adjustment = Layer::blank("Adjustment", 12, 10);
        adjustment.opacity = 0.6;
        document.layers.push(adjustment);
        for effect in [
            Adjustment::LevelsChannels {
                ranges: [
                    crate::color::DEFAULT_LEVELS,
                    [8.0, 0.9, 243.0, 2.0, 254.0],
                    [0.0, 1.3, 255.0, 0.0, 255.0],
                    crate::color::DEFAULT_LEVELS,
                ],
            },
            Adjustment::CurvesChannels {
                channels: std::array::from_fn(|i| {
                    vec![
                        Point::new(0.0, 0.0),
                        Point::new(0.5, 0.3 + i as f32 * 0.1),
                        Point::new(1.0, 1.0),
                    ]
                }),
            },
            Adjustment::HueRanges {
                settings: Box::new(crate::color::HueSettings {
                    adjustments: [
                        [10.0, 5.0, -2.0],
                        [70.0, -50.0, 8.0],
                        [0.0; 3],
                        [0.0; 3],
                        [0.0; 3],
                        [-40.0, 20.0, -5.0],
                        [0.0; 3],
                    ],
                    ..Default::default()
                }),
            },
            Adjustment::HueSaturation {
                hue: 30.0,
                saturation: 15.0,
                lightness: -12.0,
                colorize: false,
            },
            Adjustment::Levels {
                black: 12.0,
                gamma: 0.8,
                white: 230.0,
                output_black: 9.0,
                output_white: 249.0,
            },
            Adjustment::Curves {
                points: vec![
                    Point::new(0.0, 0.0),
                    Point::new(0.4, 0.65),
                    Point::new(1.0, 1.0),
                ],
            },
            Adjustment::Exposure {
                exposure: 0.4,
                offset: 0.05,
                gamma: 1.1,
            },
            Adjustment::GradientMap {
                shadows: [20, 40, 70, 255],
                highlights: [200, 220, 150, 255],
            },
            Adjustment::FilmGrain {
                amount: 60.0,
                size: 2.7,
                roughness: 35.0,
                seed: 419,
            },
            Adjustment::Grain {
                amount: 15.0,
                monochrome: false,
                seed: 999,
            },
            Adjustment::Invert,
        ] {
            let name = effect.name();
            document.layers[2].adjustment = Some(effect);
            compositor.render(&document, [12, 10]);
            compare(&document, &readback(&compositor), name);
        }
    }

    fn compare(document: &Document, gpu: &[u8], context: &str) {
        let cpu = render::render(document);
        for (index, (pixel, actual)) in cpu.pixels().zip(gpu.as_chunks::<4>().0).enumerate() {
            let expected = [
                ((pixel[0] as f32 * pixel[3] as f32 / 255.0).round() as u8),
                ((pixel[1] as f32 * pixel[3] as f32 / 255.0).round() as u8),
                ((pixel[2] as f32 * pixel[3] as f32 / 255.0).round() as u8),
                pixel[3],
            ];
            assert!(
                actual.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 2),
                "{context} pixel {index}: GPU {actual:?}, CPU {expected:?}"
            );
        }
    }
}
