mod app;

use std::path::PathBuf;

fn main() -> eframe::Result {
    let mut paths = Vec::new();
    let mut demo = false;
    let mut screenshot = None;
    let mut panel = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--demo" => demo = true,
            "--screenshot" => screenshot = args.next().map(PathBuf::from),
            "--screenshot-panel" => panel = args.next(),
            "--version" | "-V" => {
                println!("xuan {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" | "-h" => {
                println!(
                    "xuan —  native Linux image compositor\n\nUsage: xuan [IMAGE|PROJECT ...] [--demo] [--screenshot PATH] [--screenshot-panel levels|hue|curves|export|brush|selection|gradient|shape|text|new]\n\nProjects use .xuan; original .comp directory packages can also be opened."
                );
                return Ok(());
            }
            _ => paths.push(PathBuf::from(arg)),
        }
    }
    let icon = image::load_from_memory(include_bytes!("../assets/icon.png"))
        .expect("bundled application icon")
        .to_rgba8();
    let icon = egui::IconData {
        width: icon.width(),
        height: icon.height(),
        rgba: icon.into_raw(),
    };
    let mut setup = eframe::egui_wgpu::WgpuSetupCreateNew::default();
    let default_descriptor = setup.device_descriptor.clone();
    setup.device_descriptor = std::sync::Arc::new(move |adapter| {
        let mut descriptor = default_descriptor(adapter);
        // Full-resolution float processing needs more than wgpu's portable 128 MiB
        // storage binding default. Request only the limits the adapter supports.
        let limits = adapter.limits();
        if limits.max_storage_buffers_per_shader_stage >= 4 {
            descriptor.required_limits.max_storage_buffer_binding_size =
                limits.max_storage_buffer_binding_size;
            descriptor.required_limits.max_buffer_size = limits.max_buffer_size;
            descriptor.required_limits.max_texture_dimension_2d = limits.max_texture_dimension_2d;
        }
        descriptor
    });
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Xuan")
            .with_decorations(false)
            .with_transparent(true)
            .with_icon(icon)
            .with_app_id("me.silverl.xuan")
            .with_inner_size([1280.0, 860.0])
            .with_min_inner_size([850.0, 560.0]),
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            wgpu_setup: eframe::egui_wgpu::WgpuSetup::CreateNew(setup),
            ..Default::default()
        },
        persist_window: screenshot.is_none(),
        ..Default::default()
    };
    eframe::run_native(
        "Xuan",
        options,
        Box::new(move |cc| {
            let mut app = app::EditorApp::new(cc, paths, demo, screenshot);
            if let Some(panel) = panel {
                app.preview_panel(&panel);
            }
            Ok(Box::new(app))
        }),
    )
}
