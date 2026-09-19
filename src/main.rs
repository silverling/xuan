mod app;

use std::path::PathBuf;

use clap::Parser;

#[derive(Debug, Parser)]
#[command(
    version,
    about,
    after_help = "Projects use .xuan; original .comp directory packages can also be opened."
)]
struct Args {
    /// Images or projects to open
    #[arg(value_name = "IMAGE|PROJECT")]
    paths: Vec<PathBuf>,

    /// Start with a sample composition
    #[arg(long)]
    demo: bool,

    /// Capture the window to a file and exit
    #[arg(long, value_name = "PATH")]
    screenshot: Option<PathBuf>,

    /// Open a panel for the screenshot
    #[arg(
        long,
        value_name = "PANEL",
        value_parser = ["levels", "hue", "curves", "export", "brush", "selection", "gradient", "shape", "text", "new"]
    )]
    screenshot_panel: Option<String>,
}

fn main() -> eframe::Result {
    let Args {
        paths,
        demo,
        screenshot,
        screenshot_panel,
    } = Args::parse();
    let icon = image::load_from_memory(include_bytes!(
        "../assets/icons/hicolor/256x256/apps/me.silverl.xuan.png"
    ))
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
            if let Some(panel) = screenshot_panel {
                app.preview_panel(&panel);
            }
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod cli_tests {
    use super::*;
    use clap::error::ErrorKind;

    #[test]
    fn launches_without_arguments() {
        let args = Args::try_parse_from(["xuan"]).unwrap();
        assert!(args.paths.is_empty());
        assert!(!args.demo);
        assert!(args.screenshot.is_none());
        assert!(args.screenshot_panel.is_none());
    }

    #[test]
    fn accepts_paths_interleaved_with_options() {
        let args = Args::try_parse_from([
            "xuan",
            "photo.png",
            "--demo",
            "composition.xuan",
            "--screenshot",
            "preview.png",
            "--screenshot-panel",
            "levels",
            "original.comp",
        ])
        .unwrap();

        assert_eq!(
            args.paths,
            ["photo.png", "composition.xuan", "original.comp"].map(PathBuf::from)
        );
        assert!(args.demo);
        assert_eq!(args.screenshot, Some(PathBuf::from("preview.png")));
        assert_eq!(args.screenshot_panel.as_deref(), Some("levels"));
    }

    #[test]
    fn accepts_option_like_paths_after_separator() {
        let args = Args::try_parse_from(["xuan", "--", "--demo", "-photo.png"]).unwrap();
        assert_eq!(args.paths, ["--demo", "-photo.png"].map(PathBuf::from));
        assert!(!args.demo);
    }

    #[cfg(unix)]
    #[test]
    fn accepts_non_utf8_paths() {
        use std::ffi::OsString;
        use std::os::unix::ffi::OsStringExt;

        let path = OsString::from_vec(b"photo-\xff.png".to_vec());
        let args = Args::try_parse_from([
            OsString::from("xuan"),
            path.clone(),
            OsString::from("--screenshot"),
            path.clone(),
        ])
        .unwrap();

        assert_eq!(args.paths, [PathBuf::from(&path)]);
        assert_eq!(args.screenshot, Some(PathBuf::from(path)));
    }

    #[test]
    fn rejects_invalid_arguments() {
        for (args, kind) in [
            (vec!["xuan", "--unknown"], ErrorKind::UnknownArgument),
            (vec!["xuan", "--screenshot"], ErrorKind::InvalidValue),
            (vec!["xuan", "--screenshot-panel"], ErrorKind::InvalidValue),
            (
                vec!["xuan", "--screenshot-panel", "unknown"],
                ErrorKind::InvalidValue,
            ),
            (
                vec!["xuan", "--screenshot", "--demo"],
                ErrorKind::InvalidValue,
            ),
        ] {
            let error = Args::try_parse_from(&args).unwrap_err();
            assert_eq!(error.kind(), kind, "arguments: {args:?}");
        }
    }

    #[test]
    fn displays_help_and_version() {
        for (flag, kind) in [
            ("--help", ErrorKind::DisplayHelp),
            ("-h", ErrorKind::DisplayHelp),
            ("--version", ErrorKind::DisplayVersion),
            ("-V", ErrorKind::DisplayVersion),
        ] {
            let error = Args::try_parse_from(["xuan", flag]).unwrap_err();
            assert_eq!(error.kind(), kind);
            assert_eq!(error.exit_code(), 0);
        }
    }
}
