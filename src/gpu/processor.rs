//! Shared compute context. Scoped installation keeps CPU references available and
//! gives background jobs the same device as the editor without a global singleton.
use std::{
    cell::RefCell,
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::{Result, ensure};
use wgpu::util::DeviceExt;

thread_local! {
    static CANCEL: RefCell<Option<Arc<AtomicBool>>> = const { RefCell::new(None) };
    static CURRENT: RefCell<Option<Arc<Processor>>> = const { RefCell::new(None) };
}

pub struct Cancellation(Option<Arc<AtomicBool>>);
impl Drop for Cancellation {
    fn drop(&mut self) {
        CANCEL.with(|slot| *slot.borrow_mut() = self.0.take());
    }
}
pub fn cancellation(cancel: Arc<AtomicBool>) -> Cancellation {
    Cancellation(CANCEL.with(|slot| slot.replace(Some(cancel))))
}
pub(crate) fn cancelled() -> bool {
    CANCEL.with(|slot| {
        slot.borrow()
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
    })
}

fn check_cancelled() -> Result<()> {
    ensure!(!cancelled(), "Processing cancelled");
    Ok(())
}

pub struct Processor {
    pub(super) device: wgpu::Device,
    pub(super) queue: wgpu::Queue,
    pub(crate) motion_blur: super::GpuMotionBlur,
    layout: wgpu::BindGroupLayout,
    pipelines: Mutex<HashMap<&'static str, wgpu::ComputePipeline>>,
    pub(super) compositor: Mutex<Option<super::GpuCompositor>>,
}

/// Install a device for this operation, restoring the caller's context on unwind.
pub fn scope<T>(processor: Option<Arc<Processor>>, operation: impl FnOnce() -> T) -> T {
    struct Restore(Option<Arc<Processor>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CURRENT.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(CURRENT.with(|slot| slot.replace(processor)));
    operation()
}

pub fn current() -> Option<Arc<Processor>> {
    CURRENT.with(|slot| slot.borrow().clone())
}

/// Jobs inherit the device, while independent library callers can use CPU only.
pub fn spawn<T: Send + 'static>(
    operation: impl FnOnce() -> T + Send + 'static,
) -> std::thread::JoinHandle<T> {
    let processor = current();
    std::thread::spawn(move || scope(processor, operation))
}

pub(super) fn attempt<T>(
    pixels: u64,
    minimum: u64,
    operation: impl FnOnce(&Processor) -> Result<T>,
) -> Option<T> {
    if pixels < minimum || check_cancelled().is_err() {
        return None;
    }
    let processor = current()?;
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| operation(&processor))) {
        Ok(Ok(value)) => Some(value),
        Ok(Err(error)) => {
            eprintln!("GPU processing unavailable, using CPU: {error:#}");
            None
        }
        Err(_) => {
            eprintln!("GPU processing failed, using CPU");
            None
        }
    }
}

impl Processor {
    pub fn new(device: wgpu::Device, queue: wgpu::Queue) -> Arc<Self> {
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("image processing buffers"),
            entries: &std::array::from_fn::<_, 4, _>(|index| wgpu::BindGroupLayoutEntry {
                binding: index as u32,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage {
                        read_only: index != 2,
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }),
        });
        let motion_blur = super::GpuMotionBlur::new(device.clone(), queue.clone());
        Arc::new(Self {
            motion_blur,
            device,
            queue,
            layout,
            pipelines: Mutex::new(HashMap::new()),
            compositor: Mutex::new(None),
        })
    }

    pub(super) fn buffer(&self, bytes: &[u8]) -> Result<wgpu::Buffer> {
        self.check_size(bytes.len() as u64)?;
        Ok(self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("processing input"),
                contents: if bytes.is_empty() { &[0; 16] } else { bytes },
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            }))
    }

    pub(super) fn empty(&self, size: u64) -> Result<wgpu::Buffer> {
        self.check_size(size)?;
        Ok(self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("processing intermediate"),
            size: size.max(16),
            mapped_at_creation: false,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        }))
    }

    fn check_size(&self, size: u64) -> Result<()> {
        check_cancelled()?;
        let limits = self.device.limits();
        ensure!(
            size <= limits.max_storage_buffer_binding_size as u64 && size <= limits.max_buffer_size,
            "Image exceeds GPU buffer limits"
        );
        Ok(())
    }

    pub(super) fn encoder(&self) -> wgpu::CommandEncoder {
        self.device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("image processing"),
            })
    }

    pub(super) fn dispatch(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        entry: &'static str,
        shader: &str,
        buffers: [&wgpu::Buffer; 3],
        config: &[[f32; 4]],
        size: [u32; 2],
    ) -> Result<()> {
        check_cancelled()?;
        let limit = self.device.limits().max_compute_workgroups_per_dimension;
        ensure!(
            size[0].div_ceil(8) <= limit && size[1].div_ceil(8) <= limit,
            "Image exceeds GPU dispatch limits"
        );
        let mut pipelines = self.pipelines.lock().unwrap_or_else(|p| p.into_inner());
        let pipeline = pipelines
            .entry(entry)
            .or_insert_with(|| {
                let module = self
                    .device
                    .create_shader_module(wgpu::ShaderModuleDescriptor {
                        label: Some(entry),
                        source: wgpu::ShaderSource::Wgsl(shader.into()),
                    });
                let layout = self
                    .device
                    .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                        label: Some("processing pipeline"),
                        bind_group_layouts: &[&self.layout],
                        push_constant_ranges: &[],
                    });
                self.device
                    .create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                        label: Some(entry),
                        layout: Some(&layout),
                        module: &module,
                        entry_point: Some(entry),
                        compilation_options: Default::default(),
                        cache: None,
                    })
            })
            .clone();
        drop(pipelines);
        let params = self.buffer(bytemuck::cast_slice(config))?;
        let bind = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(entry),
            layout: &self.layout,
            entries: &std::array::from_fn::<_, 4, _>(|index| wgpu::BindGroupEntry {
                binding: index as u32,
                resource: if index == 3 {
                    params.as_entire_binding()
                } else {
                    buffers[index].as_entire_binding()
                },
            }),
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some(entry),
            timestamp_writes: None,
        });
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.dispatch_workgroups(size[0].div_ceil(8), size[1].div_ceil(8), 1);
        Ok(())
    }

    pub(super) fn read(
        &self,
        mut encoder: wgpu::CommandEncoder,
        buffer: &wgpu::Buffer,
        size: u64,
    ) -> Result<Vec<u8>> {
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("processing readback"),
            size,
            mapped_at_creation: false,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        });
        encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, size);
        self.queue.submit([encoder.finish()]);
        self.map(&staging)
    }

    pub(super) fn map(&self, buffer: &wgpu::Buffer) -> Result<Vec<u8>> {
        let (send, receive) = std::sync::mpsc::channel();
        buffer
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                let _ = send.send(result);
            });
        loop {
            if let Err(error) = check_cancelled() {
                buffer.unmap();
                return Err(error);
            }
            self.device.poll(wgpu::PollType::Poll)?;
            match receive.recv_timeout(Duration::from_millis(2)) {
                Ok(result) => {
                    result?;
                    break;
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => (),
                Err(error) => return Err(error.into()),
            }
        }
        let bytes = buffer.slice(..).get_mapped_range().to_vec();
        buffer.unmap();
        Ok(bytes)
    }
}
