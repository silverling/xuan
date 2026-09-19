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
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Xuan")
            .with_decorations(false)
            .with_transparent(true)
            .with_icon(icon)
            .with_app_id("org.xuan.Editor")
            .with_inner_size([1280.0, 860.0])
            .with_min_inner_size([850.0, 560.0]),
        renderer: eframe::Renderer::Wgpu,
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
