mod app;

use std::path::PathBuf;

fn main() -> eframe::Result {
    let mut paths = Vec::new();
    let mut demo = false;
    let mut screenshot = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--demo" => demo = true,
            "--screenshot" => screenshot = args.next().map(PathBuf::from),
            "--help" | "-h" => {
                println!(
                    "xuan — native Linux image compositor\n\nUsage: xuan [IMAGE|PROJECT ...] [--demo] [--screenshot PATH]\n\nProjects use .xuan; original .comp directory packages can also be opened."
                );
                return Ok(());
            }
            _ => paths.push(PathBuf::from(arg)),
        }
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("xuan")
            .with_app_id("org.xuan.Editor")
            .with_inner_size([1280.0, 860.0])
            .with_min_inner_size([850.0, 560.0]),
        renderer: eframe::Renderer::Wgpu,
        ..Default::default()
    };
    eframe::run_native(
        "xuan",
        options,
        Box::new(move |cc| Ok(Box::new(app::EditorApp::new(cc, paths, demo, screenshot)))),
    )
}
