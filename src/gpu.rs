//! wgpu compute compositor. The CPU renderer remains the export/reference path.

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
    points: [[f32; 4]; 32],
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
    sources: HashMap<usize, Source>,
    size: [u32; 2],
    buffers: [wgpu::Texture; 2],
    display: wgpu::Texture,
    blank: wgpu::Texture,
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
        let buffers =
            std::array::from_fn(|_| target(&device, [1, 1], wgpu::TextureFormat::Rgba16Float));
        let display = target(&device, [1, 1], wgpu::TextureFormat::Rgba8Unorm);
        let blank = target(&device, [1, 1], wgpu::TextureFormat::Rgba8Unorm);
        Self {
            device,
            queue,
            pipeline,
            sources: HashMap::new(),
            size: [1, 1],
            buffers,
            display,
            blank,
        }
    }

    pub fn display_view(&self) -> wgpu::TextureView {
        self.display.create_view(&Default::default())
    }
    pub fn display_texture(&self) -> &wgpu::Texture {
        &self.display
    }

    pub fn render(&mut self, document: &Document, size: [u32; 2]) {
        if size != self.size {
            self.size = size;
            self.buffers = std::array::from_fn(|_| {
                target(&self.device, size, wgpu::TextureFormat::Rgba16Float)
            });
            self.display = target(&self.device, size, wgpu::TextureFormat::Rgba8Unorm);
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
                let key = Arc::as_ptr(pixels) as usize;
                retained.insert(key);
                self.sources.entry(key).or_insert_with(|| {
                    let texture = self.device.create_texture_with_data(
                        &self.queue,
                        &wgpu::TextureDescriptor {
                            label: Some("xuan layer source"),
                            size: wgpu::Extent3d {
                                width: pixels.width(),
                                height: pixels.height(),
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
                        pixels.as_raw(),
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
                .map(|pixels| &self.sources[&(Arc::as_ptr(pixels) as usize)].texture)
                .unwrap_or(&self.blank);
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
        self.queue.submit([encoder.finish()]);
        self.sources
            .retain(|key, source| retained.contains(key) && source.pixels.strong_count() > 0);
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
        .map(|texture| texture.create_view(&Default::default()));
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

fn target(device: &wgpu::Device, size: [u32; 2], format: wgpu::TextureFormat) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("xuan canvas target"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
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
    if let Some(adjustment) = &layer.adjustment {
        match adjustment {
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

    fn readback(compositor: &GpuCompositor) -> Vec<u8> {
        let [width, height] = compositor.size;
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
            compositor.display.as_image_copy(),
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
