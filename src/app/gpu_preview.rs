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

    pub fn render(&mut self, document: &Document, size: [u32; 2], nearest: bool) {
        self.compositor.render(document, size);
        let view = self.compositor.display_view();
        let filter = if nearest {
            wgpu::FilterMode::Nearest
        } else {
            wgpu::FilterMode::Linear
        };
        let mut renderer = self.state.renderer.write();
        if let Some(id) = self.texture {
            renderer.update_egui_texture_from_wgpu_texture(&self.state.device, &view, filter, id);
        } else {
            self.texture =
                Some(renderer.register_native_texture(&self.state.device, &view, filter));
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
