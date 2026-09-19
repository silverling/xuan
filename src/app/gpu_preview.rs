use eframe::egui_wgpu::RenderState;
use xuan::{document::Document, gpu::GpuCompositor};

pub(super) struct GpuPreview {
    compositor: GpuCompositor,
    state: RenderState,
    pub texture: Option<egui::TextureId>,
}

impl GpuPreview {
    pub fn new(state: &RenderState) -> Self {
        Self {
            compositor: GpuCompositor::new(state.device.clone(), state.queue.clone()),
            state: state.clone(),
            texture: None,
        }
    }

    pub fn render(&mut self, document: &Document, size: [u32; 2]) {
        self.render_with_motion_blur(document, size, None);
    }

    pub fn render_with_motion_blur(
        &mut self,
        document: &Document,
        size: [u32; 2],
        motion_blur: Option<[f32; 2]>,
    ) {
        self.compositor
            .render_with_motion_blur(document, size, motion_blur);
        let view = self.compositor.display_view();
        let sampler = wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        };
        let mut renderer = self.state.renderer.write();
        if let Some(id) = self.texture {
            renderer.update_egui_texture_from_wgpu_texture_with_sampler_options(
                &self.state.device,
                &view,
                sampler,
                id,
            );
        } else {
            self.texture = Some(renderer.register_native_texture_with_sampler_options(
                &self.state.device,
                &view,
                sampler,
            ));
        }
    }
}

impl Drop for GpuPreview {
    fn drop(&mut self) {
        if let Some(id) = self.texture {
            self.state.renderer.write().free_texture(&id);
        }
    }
}
