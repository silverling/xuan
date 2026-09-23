//! Worker-owned RAW preview preparation and inexpensive UI texture registration.
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use anyhow::{Result, ensure};
use egui::{Color32, ColorImage, TextureHandle, TextureOptions};
use image::RgbaImage;
use xuan::{
    gpu,
    raw::{self, DecodedRaw, DevelopSettings},
};

#[derive(Clone)]
pub(super) enum PreviewImage {
    Cpu(Arc<ColorImage>),
    Gpu(wgpu::Texture),
}

#[derive(Clone)]
pub(super) enum PreviewTexture {
    Cpu(TextureHandle),
    Gpu(Arc<NativeTexture>),
}

pub(super) struct NativeTexture {
    id: egui::TextureId,
    size: [usize; 2],
    state: eframe::egui_wgpu::RenderState,
}

impl Drop for NativeTexture {
    fn drop(&mut self) {
        self.state.renderer.write().free_texture(&self.id);
    }
}

impl PreviewTexture {
    pub fn id(&self) -> egui::TextureId {
        match self {
            Self::Cpu(texture) => texture.id(),
            Self::Gpu(texture) => texture.id,
        }
    }

    pub fn size_vec2(&self) -> egui::Vec2 {
        let [width, height] = self.size();
        egui::vec2(width as f32, height as f32)
    }

    pub fn size(&self) -> [usize; 2] {
        match self {
            Self::Cpu(texture) => texture.size(),
            Self::Gpu(texture) => texture.size,
        }
    }
}

impl PreviewImage {
    pub fn register(
        self,
        ctx: &egui::Context,
        state: Option<&eframe::egui_wgpu::RenderState>,
    ) -> PreviewTexture {
        match self {
            Self::Cpu(image) => {
                PreviewTexture::Cpu(ctx.load_texture("raw_preview", image, TextureOptions::LINEAR))
            }
            Self::Gpu(texture) => {
                let state = state.expect("native RAW previews require the editor's renderer");
                let id = state.renderer.write().register_native_texture(
                    &state.device,
                    &texture.create_view(&Default::default()),
                    wgpu::FilterMode::Linear,
                );
                PreviewTexture::Gpu(Arc::new(NativeTexture {
                    id,
                    size: [texture.width() as usize, texture.height() as usize],
                    state: state.clone(),
                }))
            }
        }
    }
}

pub(super) struct PreparedPreview {
    pub image: PreviewImage,
    pub warnings: Option<PreviewImage>,
    pub histogram: [[u32; 256]; 3],
    pub clipping: [f32; 2],
    #[cfg(test)]
    pub pixels: Option<RgbaImage>,
}

impl PreparedPreview {
    pub fn cpu(pixels: RgbaImage, warnings: bool, cancel: &AtomicBool) -> Result<Self> {
        let size = [pixels.width() as usize, pixels.height() as usize];
        let mut image = ColorImage::filled(size, Color32::TRANSPARENT);
        let mut warning_image = warnings.then(|| ColorImage::filled(size, Color32::TRANSPARENT));
        let mut histogram = [[0; 256]; 3];
        let mut counts = [0_u32; 2];
        let mut visible = 0;
        for (i, p) in pixels.pixels().enumerate() {
            if i % 4096 == 0 {
                ensure!(!cancel.load(Ordering::Relaxed), "RAW preview cancelled");
            }
            let color = Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]);
            image.pixels[i] = color;
            let mut warning = color;
            if p[3] != 0 {
                visible += 1;
                for c in 0..3 {
                    histogram[c][p[c] as usize] += 1;
                }
                if p.0[..3].contains(&255) {
                    warning = Color32::from_rgb(255, 35, 65);
                    counts[1] += 1;
                } else if p.0[..3].iter().all(|v| *v <= 1) {
                    warning = Color32::from_rgb(40, 100, 255);
                    counts[0] += 1;
                }
            }
            if let Some(warnings) = &mut warning_image {
                warnings.pixels[i] = warning;
            }
        }
        Ok(Self {
            image: PreviewImage::Cpu(Arc::new(image)),
            warnings: warning_image.map(|image| PreviewImage::Cpu(Arc::new(image))),
            histogram,
            clipping: counts.map(|v| 100.0 * v as f32 / visible.max(1) as f32),
            #[cfg(test)]
            pixels: Some(pixels),
        })
    }
}

#[derive(Default)]
pub(super) struct PreviewWorker {
    renderer: gpu::RawPreviewRenderer,
    intermediate: Option<Arc<DecodedRaw>>,
    original: Option<(u32, PreviewImage)>,
}

impl PreviewWorker {
    pub fn release_buffers(&mut self) {
        self.renderer.clear();
        self.intermediate = None;
    }

    pub fn input(
        &mut self,
        full: &Arc<DecodedRaw>,
        proxy: &Arc<DecodedRaw>,
        side: u32,
        cancel: &AtomicBool,
    ) -> Result<Arc<DecodedRaw>> {
        if side == full.camera.width().max(full.camera.height()) {
            return Ok(full.clone());
        }
        if side == proxy.camera.width().max(proxy.camera.height()) {
            return Ok(proxy.clone());
        }
        if self
            .intermediate
            .as_ref()
            .is_none_or(|raw| raw.camera.width().max(raw.camera.height()) != side)
        {
            self.intermediate = Some(Arc::new(full.preview_cancellable(side, cancel)?));
        }
        Ok(self.intermediate.as_ref().unwrap().clone())
    }

    pub fn render(
        &mut self,
        input: &Arc<DecodedRaw>,
        settings: &DevelopSettings,
        warnings: bool,
        native: bool,
        cancel: &AtomicBool,
    ) -> Result<PreparedPreview> {
        if native && let Some(preview) = self.renderer.render(input, settings, warnings, cancel)? {
            return Ok(PreparedPreview {
                image: PreviewImage::Gpu(preview.texture),
                warnings: preview.warnings.map(PreviewImage::Gpu),
                histogram: preview.analysis.channels,
                clipping: preview.analysis.clipping,
                #[cfg(test)]
                pixels: None,
            });
        }
        let pixels = gpu::scope(None, || raw::render(input, settings, cancel))?;
        PreparedPreview::cpu(pixels, warnings, cancel)
    }

    pub fn original(
        &mut self,
        input: &Arc<DecodedRaw>,
        native: bool,
        cancel: &AtomicBool,
    ) -> Result<PreviewImage> {
        let side = input.camera.width().max(input.camera.height());
        if self
            .original
            .as_ref()
            .is_none_or(|(previous, _)| *previous != side)
        {
            let preview = self.render(input, &DevelopSettings::default(), false, native, cancel)?;
            self.original = Some((side, preview.image));
        }
        Ok(self.original.as_ref().unwrap().1.clone())
    }

    pub fn remember_original(&mut self, side: u32, image: PreviewImage) {
        self.original = Some((side, image));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_preparation_matches_clipping_rules_and_skips_unrequested_warning_pixels() {
        let pixels = RgbaImage::from_raw(
            4,
            1,
            vec![
                255, 0, 0, 255, 0, 1, 0, 255, 20, 30, 40, 255, 255, 255, 255, 0,
            ],
        )
        .unwrap();
        let cancel = AtomicBool::new(false);
        let preview = PreparedPreview::cpu(pixels.clone(), false, &cancel).unwrap();
        assert!(preview.warnings.is_none());
        assert_eq!(
            preview.histogram[0][255], 1,
            "transparent pixels do not enter the histogram"
        );
        assert_eq!(preview.clipping, [100.0 / 3.0; 2]);
        let preview = PreparedPreview::cpu(pixels, true, &cancel).unwrap();
        let PreviewImage::Cpu(warnings) = preview.warnings.unwrap() else {
            panic!("CPU preparation");
        };
        assert_eq!(
            warnings.pixels,
            vec![
                Color32::from_rgb(255, 35, 65),
                Color32::from_rgb(40, 100, 255),
                Color32::from_rgb(20, 30, 40),
                Color32::TRANSPARENT
            ]
        );
        assert!(PreparedPreview::cpu(RgbaImage::new(4, 1), false, &AtomicBool::new(true)).is_err());
    }

    fn native_state() -> eframe::egui_wgpu::RenderState {
        let instance = wgpu::Instance::new(&Default::default());
        let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
        eprintln!("RAW benchmark adapter: {:?}", adapter.get_info());
        let mut descriptor = wgpu::DeviceDescriptor::default();
        let limits = adapter.limits();
        descriptor.required_limits.max_storage_buffer_binding_size =
            limits.max_storage_buffer_binding_size;
        descriptor.required_limits.max_buffer_size = limits.max_buffer_size;
        descriptor.required_limits.max_texture_dimension_2d = limits.max_texture_dimension_2d;
        let (device, queue) = pollster::block_on(adapter.request_device(&descriptor)).unwrap();
        let target_format = wgpu::TextureFormat::Rgba8Unorm;
        let renderer = eframe::egui_wgpu::Renderer::new(&device, target_format, Default::default());
        eframe::egui_wgpu::RenderState {
            adapter,
            available_adapters: Vec::new(),
            device,
            queue,
            target_format,
            renderer: Arc::new(egui::mutex::RwLock::new(renderer)),
        }
    }

    #[test]
    #[ignore = "native GPU timing; optionally set XUAN_TEST_RAW to a camera file"]
    fn benchmark_raw_develop_preview() {
        use std::{hint::black_box, time::Instant};
        let state = native_state();
        let processor = gpu::Processor::new(state.device.clone(), state.queue.clone());
        let ctx = egui::Context::default();
        let full = Arc::new(if let Some(path) = std::env::var_os("XUAN_TEST_RAW") {
            let start = Instant::now();
            let (_, raw) = raw::open(std::path::Path::new(&path)).unwrap();
            eprintln!(
                "Decode {}: {:.2} ms",
                std::path::Path::new(&path).display(),
                start.elapsed().as_secs_f64() * 1000.0
            );
            raw
        } else {
            DecodedRaw {
                camera: image::Rgb32FImage::from_fn(6000, 4000, |x, y| {
                    image::Rgb([x as f32 / 6000.0, y as f32 / 4000.0, 0.4])
                }),
                as_shot: [1.0; 3],
                camera_to_rgb: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                xyz_to_camera: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                metadata: raw::RawMetadata {
                    width: 6000,
                    height: 4000,
                    ..Default::default()
                },
            }
        });
        gpu::scope(Some(processor), || {
            let proxy = Arc::new(full.preview(1600));
            let mut worker = PreviewWorker::default();
            for side in [1600, 3200, full.camera.width().max(full.camera.height())] {
                let input = worker
                    .input(&full, &proxy, side, &AtomicBool::new(false))
                    .unwrap();
                for warnings in [false, true] {
                    let mut renders = Vec::new();
                    let mut registrations = Vec::new();
                    for i in 0..6 {
                        let settings = DevelopSettings {
                            exposure: i as f32 * 0.1,
                            ..Default::default()
                        };
                        let start = Instant::now();
                        let result = worker
                            .render(&input, &settings, warnings, true, &AtomicBool::new(false))
                            .unwrap();
                        assert!(
                            matches!(result.image, PreviewImage::Gpu(_)),
                            "benchmark must not silently use CPU fallback"
                        );
                        let render = start.elapsed().as_secs_f64() * 1000.0;
                        let start = Instant::now();
                        let image = result.image.register(&ctx, Some(&state));
                        let warning = result
                            .warnings
                            .map(|image| image.register(&ctx, Some(&state)));
                        let registration = start.elapsed().as_secs_f64() * 1000.0;
                        black_box((image, warning));
                        if i != 0 {
                            renders.push(render);
                            registrations.push(registration);
                        }
                    }
                    renders.sort_by(f64::total_cmp);
                    registrations.sort_by(f64::total_cmp);
                    eprintln!(
                        "{} x {}, clipping {warnings}: worker {:.2} ms, UI registration {:.3} ms (median of 5, warmed)",
                        input.camera.width(),
                        input.camera.height(),
                        renders[2],
                        registrations[2]
                    );
                }
            }
        });
    }
}
