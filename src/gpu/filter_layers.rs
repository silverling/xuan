//! Filter the resident composite without uploading or reading back a raster.
use wgpu::util::DeviceExt;

use super::target;
use crate::effects::Filter;

pub(super) struct FilterLayers {
    pipeline: wgpu::ComputePipeline,
    horizontal: wgpu::ComputePipeline,
    output: wgpu::Texture,
    scratch: Option<wgpu::Texture>,
}

impl FilterLayers {
    pub(super) fn new(device: &wgpu::Device, size: [u32; 2]) -> Self {
        let pipeline = |format, name: &str| {
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("filter layer inputs"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: false },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::StorageTexture {
                            access: wgpu::StorageTextureAccess::WriteOnly,
                            format,
                            view_dimension: wgpu::TextureViewDimension::D2,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 2,
                        visibility: wgpu::ShaderStages::COMPUTE,
                        ty: wgpu::BindingType::Buffer {
                            ty: wgpu::BufferBindingType::Storage { read_only: true },
                            has_dynamic_offset: false,
                            min_binding_size: None,
                        },
                        count: None,
                    },
                ],
            });
            let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("filter layer pipeline"),
                bind_group_layouts: &[&layout],
                push_constant_ranges: &[],
            });
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("filter layers"),
                source: wgpu::ShaderSource::Wgsl(
                    include_str!("filter_layers.wgsl")
                        .replace("OUTPUT_FORMAT", name)
                        .into(),
                ),
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("filter layer"),
                layout: Some(&layout),
                module: &shader,
                entry_point: Some("filter_layer"),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        Self {
            pipeline: pipeline(wgpu::TextureFormat::Rgba8Unorm, "rgba8unorm"),
            horizontal: pipeline(wgpu::TextureFormat::Rgba32Float, "rgba32float"),
            output: target(device, size, wgpu::TextureFormat::Rgba8Unorm, 1),
            scratch: None,
        }
    }

    pub(super) fn render(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        source: &wgpu::Texture,
        filter: &Filter,
    ) -> &wgpu::Texture {
        let size = [source.width(), source.height()];
        if size != [self.output.width(), self.output.height()] {
            self.output = target(device, size, wgpu::TextureFormat::Rgba8Unorm, 1);
            self.scratch = None;
        }
        let mut config = vec![[0.0; 4]];
        match *filter {
            Filter::GaussianBlur { radius } => {
                let sigma = radius.max(0.01);
                let length = ((((sigma - 0.8) / 0.3 + 1.0) * 2.0 + 1.0).max(3.0) as u32) | 1;
                let mut weights: Vec<f32> = (0..length)
                    .map(|i| (-0.5 * ((i as f32 - (length / 2) as f32) / sigma).powi(2)).exp())
                    .collect();
                let total = weights.iter().sum::<f32>();
                weights.iter_mut().for_each(|w| *w /= total);
                config[0] = [0.0, (length / 2) as f32, 0.0, 0.0];
                config.extend(weights.iter().map(|w| [*w, 0.0, 0.0, 0.0]));
                let scratch = self.scratch.get_or_insert_with(|| {
                    target(device, size, wgpu::TextureFormat::Rgba32Float, 1)
                });
                dispatch(device, encoder, &self.horizontal, source, scratch, &config);
                config[0][0] = 1.0;
                dispatch(
                    device,
                    encoder,
                    &self.pipeline,
                    scratch,
                    &self.output,
                    &config,
                );
                return &self.output;
            }
            Filter::MotionBlur { distance, angle } => {
                let (sin, cos) = angle.to_radians().sin_cos();
                config[0] = [
                    2.0,
                    distance.ceil().clamp(1.0, 256.0),
                    distance * cos,
                    distance * sin,
                ];
            }
            Filter::Noise { amount, monochrome } => {
                config[0] = [3.0, amount / 100.0, u32::from(monochrome) as f32, 0.0];
            }
            Filter::LensCorrection {
                distortion,
                vignette,
            } => {
                config[0] = [4.0, distortion, vignette, 0.0];
            }
        }
        dispatch(
            device,
            encoder,
            &self.pipeline,
            source,
            &self.output,
            &config,
        );
        &self.output
    }
}

fn dispatch(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    pipeline: &wgpu::ComputePipeline,
    source: &wgpu::Texture,
    output: &wgpu::Texture,
    config: &[[f32; 4]],
) {
    let parameters = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("filter layer parameters"),
        contents: bytemuck::cast_slice(config),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let views = [source, output].map(|t| t.create_view(&Default::default()));
    let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("filter layer inputs"),
        layout: &pipeline.get_bind_group_layout(0),
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
                resource: parameters.as_entire_binding(),
            },
        ],
    });
    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
        label: Some("filter layer"),
        timestamp_writes: None,
    });
    pass.set_pipeline(pipeline);
    pass.set_bind_group(0, &bind, &[]);
    pass.dispatch_workgroups(output.width().div_ceil(8), output.height().div_ceil(8), 1);
}
