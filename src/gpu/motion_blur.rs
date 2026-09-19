use std::{
    sync::{
        Arc, OnceLock, Weak,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use bytemuck::{Pod, Zeroable};
use image::RgbaImage;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Sample {
    offset: [i32; 4],
    weights: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Parameters {
    size: [u32; 4],
    samples: [Sample; 256],
}

/// A worker-safe full-resolution filter. Shader compilation and readback never
/// need to run on the UI thread; clones share the lazily compiled pipeline.
#[derive(Clone)]
pub struct GpuMotionBlur {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipeline: Arc<OnceLock<wgpu::ComputePipeline>>,
    source: Option<(Weak<RgbaImage>, wgpu::Texture)>,
}

impl GpuMotionBlur {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Self {
        Self {
            device,
            queue,
            pipeline: Arc::default(),
            source: None,
        }
    }

    pub(super) fn with_source(mut self, pixels: &Arc<RgbaImage>, texture: wgpu::Texture) -> Self {
        self.source = Some((Arc::downgrade(pixels), texture));
        self
    }

    /// Returns None when the full layer exceeds this device's resource limits.
    /// A preview proxy must never be substituted for the original pixels here.
    pub fn render(
        &self,
        pixels: &Arc<RgbaImage>,
        distance: f32,
        angle: f32,
        padding: u32,
        cancel: &AtomicBool,
    ) -> Result<Option<RgbaImage>> {
        ensure!(!cancel.load(Ordering::Relaxed), "Filter cancelled");
        let size =
            [pixels.width(), pixels.height()].map(|side| side.checked_add(padding.checked_mul(2)?));
        let [Some(width), Some(height)] = size else {
            return Ok(None);
        };
        let limits = self.device.limits();
        if pixels.width() == 0
            || pixels.height() == 0
            || width.max(height) > limits.max_texture_dimension_2d
            || width.max(height).div_ceil(8) > limits.max_compute_workgroups_per_dimension
        {
            return Ok(None);
        }
        let stride = (u64::from(width) * 4).div_ceil(256) * 256;
        let buffer_size = stride * u64::from(height);
        if buffer_size > limits.max_buffer_size
            || stride > u32::MAX as u64
            || size_of::<Parameters>() > limits.max_uniform_buffer_binding_size as usize
        {
            return Ok(None);
        }

        let pipeline = self.pipeline.get_or_init(|| {
            let shader = self
                .device
                .create_shader_module(wgpu::ShaderModuleDescriptor {
                    label: Some("xuan full-resolution motion blur"),
                    source: wgpu::ShaderSource::Wgsl(include_str!("motion_blur.wgsl").into()),
                });
            self.device
                .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                    label: Some("xuan full-resolution motion blur"),
                    layout: None,
                    module: &shader,
                    entry_point: Some("motion_blur"),
                    compilation_options: Default::default(),
                    cache: None,
                })
        });
        ensure!(!cancel.load(Ordering::Relaxed), "Filter cancelled");
        let source = if let Some((cached, texture)) = &self.source
            && cached.ptr_eq(&Arc::downgrade(pixels))
            && [texture.width(), texture.height()] == [pixels.width(), pixels.height()]
        {
            texture.clone()
        } else {
            self.device.create_texture_with_data(
                &self.queue,
                &wgpu::TextureDescriptor {
                    label: Some("xuan full-resolution filter source"),
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
            )
        };
        let output = super::target(
            &self.device,
            [width, height],
            wgpu::TextureFormat::Rgba8Unorm,
            1,
        );
        let mut params = Parameters::zeroed();
        let steps = distance.ceil().clamp(1.0, 256.0) as u32;
        params.size = [width, height, padding, steps];
        let (sin, cos) = angle.to_radians().sin_cos();
        for i in 0..steps {
            let offset = ((i as f32 + 0.5) / steps as f32 - 0.5) * distance;
            let (x, y) = (offset * cos, offset * sin);
            let (fx, fy) = (x - x.floor(), y - y.floor());
            params.samples[i as usize] = Sample {
                offset: [x.floor() as i32, y.floor() as i32, 0, 0],
                weights: [
                    (1.0 - fx) * (1.0 - fy),
                    fx * (1.0 - fy),
                    (1.0 - fx) * fy,
                    fx * fy,
                ],
            };
        }
        let uniform = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("xuan motion blur samples"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let readback = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("xuan filtered layer readback"),
            size: buffer_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let source_view = source.create_view(&Default::default());
        let output_view = output.create_view(&Default::default());
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("xuan motion blur inputs"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&source_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&output_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: uniform.as_entire_binding(),
                },
            ],
        });
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("xuan apply motion blur"),
            });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("xuan apply motion blur"),
                timestamp_writes: None,
            });
            pass.set_pipeline(pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(width.div_ceil(8), height.div_ceil(8), 1);
        }
        encoder.copy_texture_to_buffer(
            output.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &readback,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(stride as u32),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        ensure!(!cancel.load(Ordering::Relaxed), "Filter cancelled");
        self.queue.submit([encoder.finish()]);
        let (send, receive) = mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = send.send(result);
            });
        loop {
            ensure!(!cancel.load(Ordering::Relaxed), "Filter cancelled");
            self.device
                .poll(wgpu::PollType::Poll)
                .context("Could not poll GPU filter")?;
            match receive.recv_timeout(Duration::from_millis(2)) {
                Ok(result) => {
                    result.context("Could not read filtered pixels from GPU")?;
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    anyhow::bail!("GPU filter readback stopped")
                }
            }
        }
        let mapped = readback.slice(..).get_mapped_range();
        let mut result = RgbaImage::new(width, height);
        for (src, dst) in mapped
            .chunks_exact(stride as usize)
            .zip(result.as_mut().chunks_exact_mut(width as usize * 4))
        {
            ensure!(!cancel.load(Ordering::Relaxed), "Filter cancelled");
            dst.copy_from_slice(&src[..dst.len()]);
        }
        drop(mapped);
        readback.unmap();
        Ok(Some(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        document::{Document, Layer, Mask},
        effects::{self, Filter},
        gpu::GpuCompositor,
    };
    use image::{GrayImage, Luma, Rgba};

    fn gpu(limits: wgpu::Limits) -> GpuMotionBlur {
        let instance = wgpu::Instance::new(&Default::default());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            required_limits: limits,
            ..Default::default()
        }))
        .unwrap();
        GpuMotionBlur::new(device, queue)
    }

    fn compare(actual: &RgbaImage, expected: &RgbaImage) {
        assert_eq!(actual.dimensions(), expected.dimensions());
        for (p, q) in actual.pixels().zip(expected.pixels()) {
            for c in 0..4 {
                if c < 3 && p[3] == 0 && q[3] == 0 {
                    continue;
                }
                assert!(p[c].abs_diff(q[c]) <= 1, "GPU {p:?}, CPU {q:?}");
            }
        }
    }

    #[test]
    #[ignore = "requires a Vulkan or OpenGL compute adapter"]
    fn full_resolution_pixels_match_cpu_with_transparency_and_unaligned_rows() {
        let gpu = gpu(Default::default());
        for (width, height) in [(31, 17), (1, 7), (7, 1)] {
            let pixels = Arc::new(RgbaImage::from_fn(width, height, |x, y| {
                Rgba([
                    (x * 67 + y * 41) as u8,
                    (x * 23 + y * 59) as u8,
                    (x * 13 + y * 17) as u8,
                    if (x + y) % 3 == 0 {
                        0
                    } else {
                        (x * 53 + y * 97) as u8
                    },
                ])
            }));
            for distance in [1.0, 4.0, 15.0, 23.7, 200.0] {
                for angle in [0.0, 35.0, -35.0, 90.0, -180.0] {
                    let padding = (distance * 0.5_f32).ceil() as u32 + 1;
                    let actual = gpu
                        .render(&pixels, distance, angle, padding, &AtomicBool::new(false))
                        .unwrap()
                        .unwrap();
                    let mut expanded = RgbaImage::new(width + padding * 2, height + padding * 2);
                    image::imageops::replace(
                        &mut expanded,
                        &*pixels,
                        padding as i64,
                        padding as i64,
                    );
                    let expected =
                        effects::filtered(&expanded, &Filter::MotionBlur { distance, angle });
                    compare(&actual, &expected);
                }
            }
        }
    }

    #[test]
    #[ignore = "requires a Vulkan or OpenGL compute adapter"]
    fn apply_uploads_original_when_preview_is_reduced_and_preserves_selection_and_mask() {
        let gpu = gpu(Default::default());
        let mut compositor = GpuCompositor::new(gpu.device.clone(), gpu.queue.clone());
        let mut original = Document::new(160, 120).unwrap();
        original.insert(Layer::image(
            "Detail",
            RgbaImage::from_fn(129, 97, |x, y| {
                Rgba([
                    if (x + y) % 2 == 0 { 255 } else { 0 },
                    x as u8,
                    y as u8,
                    210,
                ])
            }),
        ));
        let layer = original.active_mut().unwrap();
        layer.transform.x = 8.0;
        layer.transform.y = 6.0;
        layer.transform.rotation = 17.0;
        layer.mask = Some(Mask::white());
        let pixels = layer.pixels.clone().unwrap();
        for size in [[16, 12], [160, 120]] {
            compositor.render(&original, size);
            let worker = compositor.motion_blur_worker(&pixels);
            assert_eq!(worker.source.is_some(), size == [160, 120]);
            for selection in [
                None,
                Some(Arc::new(GrayImage::from_fn(160, 120, |x, _| {
                    Luma([if x < 80 { 127 } else { 0 }])
                }))),
            ] {
                let mut actual = original.clone();
                actual.selection = selection;
                let mut expected = actual.clone();
                let filter = Filter::MotionBlur {
                    distance: 8.0,
                    angle: 35.0,
                };
                effects::apply_filter_with_gpu(
                    &mut actual,
                    &filter,
                    false,
                    &AtomicBool::new(false),
                    &worker,
                )
                .unwrap();
                effects::apply_filter(&mut expected, &filter, false).unwrap();
                let actual = actual.active().unwrap();
                let expected = expected.active().unwrap();
                compare(
                    actual.pixels.as_ref().unwrap(),
                    expected.pixels.as_ref().unwrap(),
                );
                assert_eq!(actual.transform, expected.transform);
                assert_eq!(
                    actual.mask.as_ref().unwrap().placement,
                    expected.mask.as_ref().unwrap().placement
                );
                assert!(Arc::ptr_eq(
                    &actual.mask.as_ref().unwrap().pixels,
                    &expected.mask.as_ref().unwrap().pixels
                ));
            }
        }
    }

    #[test]
    #[ignore = "requires a Vulkan or OpenGL compute adapter"]
    fn device_limits_fall_back_to_cpu_and_cancellation_does_not_commit() {
        let gpu = gpu(wgpu::Limits {
            max_texture_dimension_2d: 32,
            ..Default::default()
        });
        let mut actual = Document::new(32, 24).unwrap();
        actual.insert(Layer::image(
            "Pixels",
            RgbaImage::from_pixel(31, 17, Rgba([240, 50, 80, 210])),
        ));
        let original = actual.clone();
        let pixels = actual.active().unwrap().pixels.as_ref().unwrap();
        let filter = Filter::MotionBlur {
            distance: 20.0,
            angle: 35.0,
        };
        assert!(
            gpu.render(pixels, 20.0, 35.0, 11, &AtomicBool::new(false))
                .unwrap()
                .is_none()
        );
        assert!(
            gpu.pipeline.get().is_none(),
            "Unsupported sizes should not allocate GPU resources"
        );
        let mut expected = actual.clone();
        effects::apply_filter(&mut expected, &filter, false).unwrap();
        effects::apply_filter_with_gpu(&mut actual, &filter, false, &AtomicBool::new(false), &gpu)
            .unwrap();
        assert_eq!(
            actual.active().unwrap().pixels,
            expected.active().unwrap().pixels
        );
        actual = original.clone();
        assert!(
            effects::apply_filter_with_gpu(
                &mut actual,
                &filter,
                false,
                &AtomicBool::new(true),
                &gpu
            )
            .is_err()
        );
        assert_eq!(
            actual.active().unwrap().pixels,
            original.active().unwrap().pixels
        );
        assert_eq!(
            actual.active().unwrap().transform,
            original.active().unwrap().transform
        );
    }
}
