use super::*;

// Tests that publish images share the desktop's system clipboard.
static CLIPBOARD_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[test]
fn heic_opens_as_a_document_and_imports_as_an_undoable_layer() {
    let (_, mut app) = app();
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/io/fixtures/rgb-strips.heic");
    app.open_path(&path, false);
    assert!(app.error.is_none(), "{:?}", app.error);
    let document = &app.session().unwrap().document;
    assert_eq!((document.width, document.height), (96, 32));
    let pixels = document.layers[0].pixels.clone();
    let sessions = app.sessions.len();

    app.open_path(&path, true);
    assert!(app.error.is_none(), "{:?}", app.error);
    assert_eq!(app.sessions.len(), sessions);
    assert_eq!(app.session().unwrap().document.layers.len(), 2);
    assert_eq!(
        app.session().unwrap().document.active().unwrap().pixels,
        pixels
    );
    app.command("undo");
    assert_eq!(app.session().unwrap().document.layers.len(), 1);
}

#[test]
fn text_tool_creates_edits_and_undoes_one_transaction() {
    let (context, mut app) = app();
    app.dimensions = [640, 480];
    app.new_document();
    keyboard_frame(
        &context,
        &mut app,
        vec![text_key(egui::Key::T, egui::Modifiers::NONE)],
        egui::Modifiers::NONE,
    );
    assert!(app.tool == Tool::Text);
    click_canvas(
        &context,
        &mut app,
        Point::new(40.0, 50.0),
        egui::Modifiers::NONE,
    );
    assert!(app.dialog == Some(Dialog::Text));
    assert_eq!(app.session().unwrap().document.layers.len(), 2);
    let id = app.session().unwrap().document.active.unwrap();
    assert!(
        (app.session()
            .unwrap()
            .document
            .active()
            .unwrap()
            .transform
            .x
            - 40.0)
            .abs()
            < 0.01
    );
    let style = &mut app.text_edit.as_mut().unwrap().style;
    style.content = "Editable text\nSecond line".into();
    style.bold = true;
    style.italic = true;
    style.underline = true;
    style.strikethrough = true;
    app.preview_text();
    frame(&context, &mut app);
    assert_eq!(app.session().unwrap().history.names().count(), 0);
    app.finish_text(true);
    let session = app.session().unwrap();
    let pixels = session.document.active().unwrap().pixels.clone();
    assert_eq!(session.history.names().count(), 1);
    assert!(
        session
            .document
            .active()
            .unwrap()
            .text
            .as_ref()
            .unwrap()
            .bold
    );

    app.start_text(Some(id), Point::default());
    app.text_edit.as_mut().unwrap().style.content = "Revised".into();
    app.preview_text();
    app.finish_text(true);
    assert_eq!(app.session().unwrap().document.active.unwrap(), id);
    assert_eq!(app.session().unwrap().document.layers.len(), 2);
    app.command("undo");
    assert_eq!(
        app.session().unwrap().document.active().unwrap().pixels,
        pixels
    );
    app.command("undo");
    assert_eq!(app.session().unwrap().document.layers.len(), 1);
    app.command("redo");
    assert_eq!(
        app.session().unwrap().document.active().unwrap().pixels,
        pixels
    );
}

fn text_key(key: egui::Key, modifiers: egui::Modifiers) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers,
    }
}

#[test]
fn font_picker_arrows_preview_filtered_fonts_without_editing_text() {
    let (context, mut app) = app();
    app.dimensions = [640, 480];
    app.new_document();
    app.start_text(None, Point::new(25.0, 35.0));
    frame(&context, &mut app);
    let original = app.text_edit.as_ref().unwrap().style.clone();
    let families = app.text_renderer.as_ref().unwrap().families().to_vec();
    let start = families
        .iter()
        .position(|family| family == &original.family)
        .unwrap();
    let position = layer_label(&context, &mut app, &original.family) + egui::vec2(5.0, 5.0);
    pointer_frame(
        &context,
        &mut app,
        position,
        Some(true),
        egui::Modifiers::NONE,
    );
    pointer_frame(
        &context,
        &mut app,
        position,
        Some(false),
        egui::Modifiers::NONE,
    );
    frame(&context, &mut app);
    assert!(egui::Popup::is_any_open(&context));

    for step in 1..=12 {
        keyboard_frame(
            &context,
            &mut app,
            vec![text_key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
            egui::Modifiers::NONE,
        );
        let style = &app.text_edit.as_ref().unwrap().style;
        assert_eq!(
            style.family,
            families[(start + step).min(families.len() - 1)]
        );
        assert_eq!(style.content, original.content);
        let layer = app.session().unwrap().document.active().unwrap();
        assert_eq!(layer.text.as_ref().unwrap(), style);
        assert!(egui::Popup::is_any_open(&context));
        assert_eq!(app.session().unwrap().history.names().count(), 0);
    }
    keyboard_frame(
        &context,
        &mut app,
        vec![text_key(egui::Key::ArrowUp, egui::Modifiers::NONE)],
        egui::Modifiers::NONE,
    );
    assert_eq!(
        app.text_edit.as_ref().unwrap().style.family,
        families[(start + 12).min(families.len() - 1).saturating_sub(1)]
    );
    let style = app.text_edit.as_ref().unwrap().style.clone();
    let expected = app.text_renderer.as_mut().unwrap().render(&style).unwrap();
    assert_eq!(
        app.session()
            .unwrap()
            .document
            .active()
            .unwrap()
            .pixels
            .as_deref(),
        Some(&expected)
    );

    keyboard_frame(
        &context,
        &mut app,
        vec![egui::Event::Text(original.family.clone())],
        egui::Modifiers::NONE,
    );
    keyboard_frame(
        &context,
        &mut app,
        vec![text_key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
        egui::Modifiers::NONE,
    );
    assert_eq!(
        app.text_edit.as_ref().unwrap().style.family,
        original.family
    );
    assert_eq!(
        app.text_edit.as_ref().unwrap().style.content,
        original.content
    );
    keyboard_frame(
        &context,
        &mut app,
        vec![egui::Event::Text(" no matching font".into())],
        egui::Modifiers::NONE,
    );
    keyboard_frame(
        &context,
        &mut app,
        vec![text_key(egui::Key::ArrowDown, egui::Modifiers::NONE)],
        egui::Modifiers::NONE,
    );
    assert_eq!(
        app.text_edit.as_ref().unwrap().style.family,
        original.family
    );
    keyboard_frame(
        &context,
        &mut app,
        vec![text_key(egui::Key::Escape, egui::Modifiers::NONE)],
        egui::Modifiers::NONE,
    );
    assert!(!egui::Popup::is_any_open(&context));
    assert!(app.dialog == Some(Dialog::Text));
    keyboard_frame(
        &context,
        &mut app,
        vec![text_key(egui::Key::Escape, egui::Modifiers::NONE)],
        egui::Modifiers::NONE,
    );
    assert!(app.dialog.is_none());
    assert_eq!(app.session().unwrap().document.layers.len(), 1);
}

#[test]
fn text_dialog_typing_apply_and_escape_do_not_trigger_canvas_shortcuts() {
    let (context, mut app) = app();
    app.dimensions = [640, 480];
    app.new_document();
    app.start_text(None, Point::new(25.0, 35.0));
    frame(&context, &mut app);
    keyboard_frame(
        &context,
        &mut app,
        vec![egui::Event::Text("Typing B and T".into())],
        egui::Modifiers::NONE,
    );
    assert_eq!(
        app.text_edit.as_ref().unwrap().style.content,
        "Typing B and T"
    );
    keyboard_frame(
        &context,
        &mut app,
        vec![text_key(egui::Key::Enter, egui::Modifiers::CTRL)],
        egui::Modifiers::CTRL,
    );
    assert!(app.dialog.is_none());
    let session = app.session().unwrap();
    assert_eq!(
        session
            .document
            .active()
            .unwrap()
            .text
            .as_ref()
            .unwrap()
            .content,
        "Typing B and T"
    );
    let original = session.document.active().unwrap().pixels.clone();
    let id = session.document.active.unwrap();
    app.start_text(Some(id), Point::default());
    frame(&context, &mut app);
    keyboard_frame(
        &context,
        &mut app,
        vec![egui::Event::Paste("Changed".into())],
        egui::Modifiers::CTRL,
    );
    keyboard_frame(
        &context,
        &mut app,
        vec![text_key(egui::Key::Escape, egui::Modifiers::NONE)],
        egui::Modifiers::NONE,
    );
    assert!(app.dialog.is_none());
    assert_eq!(
        app.session().unwrap().document.active().unwrap().pixels,
        original
    );
    assert_eq!(app.session().unwrap().history.names().count(), 1);

    app.start_text(None, Point::new(20.0, 20.0));
    frame(&context, &mut app);
    app.finish_text(false);
    assert_eq!(app.session().unwrap().document.layers.len(), 2);
    app.session_mut()
        .unwrap()
        .document
        .active_mut()
        .unwrap()
        .locked = true;
    app.start_text(Some(id), Point::default());
    assert!(app.dialog.is_none());
}

#[test]
fn text_preview_keeps_group_parent_and_rejects_invalid_edits() {
    let (_, mut app) = app();
    app.dimensions = [640, 480];
    app.new_document();
    app.command("group");
    let group = app.session().unwrap().document.active.unwrap();
    app.start_text(None, Point::new(10.0, 20.0));
    app.text_edit.as_mut().unwrap().style.content = "In a folder".into();
    app.preview_text();
    assert_eq!(
        app.session().unwrap().document.active().unwrap().parent,
        Some(group)
    );
    let pixels = app
        .session()
        .unwrap()
        .document
        .active()
        .unwrap()
        .pixels
        .clone();
    app.text_edit.as_mut().unwrap().style.size = f32::NAN;
    app.preview_text();
    app.finish_text(true);
    assert!(app.dialog == Some(Dialog::Text));
    assert_eq!(
        app.session().unwrap().document.active().unwrap().pixels,
        pixels
    );
    app.text_edit.as_mut().unwrap().style.size = 24.0;
    app.preview_text();
    app.finish_text(true);
    assert!(app.dialog.is_none());
    app.session().unwrap().document.validate().unwrap();
}

#[test]
#[ignore = "requires a GPU; optionally set XUAN_ZOOM_BENCH_IMAGE to an image path"]
fn benchmark_large_image_zoom() {
    let (context, mut app, state) = large_image_benchmark_app();
    app.tool = Tool::Hand;
    for _ in 0..3 {
        frame(&context, &mut app);
    }
    state
        .device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    let mut durations = Vec::new();
    for step in (0..12).chain((0..12).rev()).cycle().take(48) {
        app.session_mut().unwrap().zoom = 0.8_f32.powi(step);
        let start = std::time::Instant::now();
        let output = frame(&context, &mut app);
        let _ = context.tessellate(output.shapes, output.pixels_per_point);
        state
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        durations.push(start.elapsed().as_secs_f64() * 1000.0);
    }
    report_benchmark("Zoom UI + compositor", durations);
}

#[test]
#[ignore = "requires a GPU; optionally set XUAN_ZOOM_BENCH_IMAGE to an image path"]
fn benchmark_large_image_levels() {
    let (context, mut app, state) = large_image_benchmark_app();
    app.start_adjustment(
        Adjustment::LevelsChannels {
            ranges: [xuan::color::DEFAULT_LEVELS; 4],
        },
        true,
    );
    for _ in 0..3 {
        frame(&context, &mut app);
    }
    for refresh in [false, true] {
        let mut durations = Vec::new();
        for step in 0..24 {
            let edit = app.effect.as_mut().unwrap();
            if refresh {
                let Some(Adjustment::LevelsChannels { ranges }) = &mut edit.adjustment else {
                    unreachable!();
                };
                ranges[0][0] = step as f32;
                edit.refresh = true;
            }
            let start = std::time::Instant::now();
            let output = pointer_frame(
                &context,
                &mut app,
                Pos2::new(500.0 + step as f32, 250.0),
                None,
                egui::Modifiers::NONE,
            );
            let _ = context.tessellate(output.shapes, output.pixels_per_point);
            state
                .device
                .poll(wgpu::PollType::wait_indefinitely())
                .unwrap();
            durations.push(start.elapsed().as_secs_f64() * 1000.0);
        }
        report_benchmark(
            if refresh {
                "Levels live preview"
            } else {
                "Levels pointer movement"
            },
            durations,
        );
    }
    assert!(app.error.is_none(), "{:?}", app.error);
}

#[test]
#[ignore = "requires a GPU; optionally set XUAN_ZOOM_BENCH_IMAGE to an image path"]
fn benchmark_large_image_motion_blur() {
    let (context, mut app, state) = large_image_benchmark_app();
    eprintln!("Motion Blur adapter: {:?}", state.adapter.get_info());
    for _ in 0..3 {
        frame(&context, &mut app);
    }
    app.start_filter(Filter::MotionBlur {
        distance: 15.0,
        angle: 0.0,
    });
    for _ in 0..3 {
        frame(&context, &mut app);
    }
    state
        .device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    for distance in [15.0, 200.0] {
        let mut durations = Vec::new();
        for step in 0..24 {
            let edit = app.effect.as_mut().unwrap();
            edit.filter = Some(Filter::MotionBlur {
                distance,
                angle: step as f32 * 3.0,
            });
            edit.refresh = true;
            let start = std::time::Instant::now();
            // The dialog updates settings after drawing the canvas, so include
            // the following frame and GPU completion in end-to-end latency.
            frame(&context, &mut app);
            frame(&context, &mut app);
            state
                .device
                .poll(wgpu::PollType::wait_indefinitely())
                .unwrap();
            durations.push(start.elapsed().as_secs_f64() * 1000.0);
            assert!(!app.effect.as_ref().unwrap().filter_preview.busy());
            assert!(app.session().unwrap().motion_blur_preview.is_some());
        }
        report_benchmark(&format!("Motion Blur distance {distance}"), durations);
    }
    assert!(app.error.is_none(), "{:?}", app.error);
}

#[test]
#[ignore = "requires a GPU; optionally set XUAN_ZOOM_BENCH_IMAGE to an image path"]
fn benchmark_large_image_motion_blur_apply() {
    let (context, mut app, state) = large_image_benchmark_app();
    frame(&context, &mut app);
    state
        .device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    let session = app.session().unwrap();
    let original = &session.document;
    let pixels = original.active().unwrap().pixels.as_ref().unwrap();
    let worker = session.gpu.as_ref().unwrap().motion_blur_worker(pixels);
    let cancel = std::sync::atomic::AtomicBool::new(false);
    eprintln!(
        "GPU Apply adapter: {:?}, source {}x{}",
        state.adapter.get_info(),
        pixels.width(),
        pixels.height()
    );
    for distance in [15.0, 200.0] {
        let filter = Filter::MotionBlur {
            distance,
            angle: 35.0,
        };
        let padding = (distance * 0.5_f32).ceil() as u32 + 1;
        let start = std::time::Instant::now();
        let result = worker
            .render(pixels, distance, 35.0, padding, &cancel)
            .unwrap()
            .expect("GPU must execute the benchmark");
        let gpu_time = start.elapsed();
        let mut expected = original.clone();
        let start = std::time::Instant::now();
        xuan::effects::apply_filter(&mut expected, &filter, false).unwrap();
        let cpu_time = start.elapsed();
        let expected = expected.active().unwrap().pixels.as_ref().unwrap();
        assert_eq!(result.dimensions(), expected.dimensions());
        assert!(
            result
                .as_raw()
                .iter()
                .zip(expected.as_raw())
                .all(|(a, b)| a.abs_diff(*b) <= 1)
        );
        eprintln!(
            "Full-resolution Apply distance {distance}: GPU + readback {gpu_time:?}, CPU {cpu_time:?}, {:.1}x faster",
            cpu_time.as_secs_f64() / gpu_time.as_secs_f64()
        );
    }
}

#[test]
#[ignore = "requires a Vulkan or OpenGL compute adapter; run explicitly for native verification"]
fn motion_blur_preview_toggles_cancels_and_applies_full_resolution() {
    let (context, mut app, state) = large_image_benchmark_app();
    let adapter = state.adapter.get_info();
    eprintln!("Motion Blur adapter: {adapter:?}");
    // Software adapters intentionally use the asynchronous CPU preview.
    let gpu_preview = adapter.device_type != wgpu::DeviceType::Cpu;
    app.sessions = vec![Session::new(
        Document::new(32, 24).unwrap(),
        "Blur test".into(),
        None,
    )];
    app.brush.color = [210, 80, 40, 255];
    app.command("fill_fg");
    frame(&context, &mut app);
    assert_eq!(app.session().unwrap().gpu.is_some(), gpu_preview);
    let original = app.session().unwrap().document.clone();
    let pixels = original.active().unwrap().pixels.as_ref().unwrap();
    let revision = app.session().unwrap().history.revision;
    let filter = Filter::MotionBlur {
        distance: 20.0,
        angle: 35.0,
    };
    let mut expected = original.clone();
    xuan::effects::apply_filter(&mut expected, &filter, false).unwrap();
    app.start_filter(filter.clone());
    for preview in [true, false, true] {
        let edit = app.effect.as_mut().unwrap();
        edit.preview = preview;
        edit.refresh = true;
        frame(&context, &mut app);
        frame(&context, &mut app);
        state
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        if !gpu_preview {
            wait_for_filter_preview(&context, &mut app);
        }
        assert_eq!(
            app.session().unwrap().motion_blur_preview.is_some(),
            preview && gpu_preview
        );
        assert!(!app.effect.as_ref().unwrap().filter_preview.busy());
        assert_eq!(app.session().unwrap().history.revision, revision);
        let preview_pixels = app
            .session()
            .unwrap()
            .document
            .active()
            .unwrap()
            .pixels
            .as_ref()
            .unwrap();
        if preview && !gpu_preview {
            assert_eq!(
                preview_pixels,
                expected.active().unwrap().pixels.as_ref().unwrap()
            );
        } else {
            assert!(Arc::ptr_eq(pixels, preview_pixels));
        }
    }

    let apply = layer_label(&context, &mut app, "Apply") + Vec2::splat(5.0);
    pointer_frame(&context, &mut app, apply, Some(true), egui::Modifiers::NONE);
    pointer_frame(
        &context,
        &mut app,
        apply,
        Some(false),
        egui::Modifiers::NONE,
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while app.effect.is_some() {
        frame(&context, &mut app);
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert!(app.dialog.is_none());
    assert!(app.session().unwrap().motion_blur_preview.is_none());
    assert_eq!(app.session().unwrap().history.revision, revision + 1);
    assert_eq!(
        app.session().unwrap().document.active().unwrap().pixels,
        expected.active().unwrap().pixels
    );
    app.command("undo");
    assert_eq!(
        app.session().unwrap().document.active().unwrap().pixels,
        original.active().unwrap().pixels
    );
    app.command("redo");
    assert_eq!(
        app.session().unwrap().document.active().unwrap().pixels,
        expected.active().unwrap().pixels
    );

    app.start_filter(filter);
    frame(&context, &mut app);
    frame(&context, &mut app);
    if !gpu_preview {
        wait_for_filter_preview(&context, &mut app);
    }
    assert_eq!(
        app.session().unwrap().motion_blur_preview.is_some(),
        gpu_preview
    );
    keyboard_frame(
        &context,
        &mut app,
        vec![text_key(egui::Key::Escape, egui::Modifiers::NONE)],
        egui::Modifiers::NONE,
    );
    assert!(app.session().unwrap().motion_blur_preview.is_none());
    assert!(app.dialog.is_none());
    assert_eq!(app.session().unwrap().history.revision, revision + 1);
    assert_eq!(
        app.session().unwrap().document.active().unwrap().pixels,
        expected.active().unwrap().pixels
    );
    assert!(app.error.is_none(), "{:?}", app.error);
}

fn wait_for_filter_preview(context: &egui::Context, app: &mut EditorApp) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while app.effect.as_ref().unwrap().filter_preview.busy() {
        frame(context, app);
        assert!(
            std::time::Instant::now() < deadline,
            "Filter preview worker did not finish"
        );
        std::thread::yield_now();
    }
}

fn large_image_benchmark_app() -> (egui::Context, EditorApp, eframe::egui_wgpu::RenderState) {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .unwrap();
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let target_format = wgpu::TextureFormat::Rgba8Unorm;
    let renderer = eframe::egui_wgpu::Renderer::new(&device, target_format, Default::default());
    let state = eframe::egui_wgpu::RenderState {
        adapter,
        available_adapters: Vec::new(),
        device,
        queue,
        target_format,
        renderer: Arc::new(egui::mutex::RwLock::new(renderer)),
    };
    let pixels = std::env::var_os("XUAN_ZOOM_BENCH_IMAGE").map_or_else(
        || {
            RgbaImage::from_fn(3000, 3000, |x, y| {
                image::Rgba([x as u8, y as u8, (x + y) as u8, 255])
            })
        },
        |path| io::import_image(Path::new(&path)).unwrap(),
    );
    let mut document = Document::new(pixels.width(), pixels.height()).unwrap();
    document.layers = vec![Layer::image("Large image", pixels)];
    document.select(document.layers[0].id, false);
    let (context, mut app) = app();
    app.gpu_state = Some(state.clone());
    app.sessions
        .push(Session::new(document, "Zoom benchmark".into(), None));
    (context, app, state)
}

fn report_benchmark(name: &str, mut durations: Vec<f64>) {
    durations.sort_by(f64::total_cmp);
    let count = durations.len();
    eprintln!(
        "{name} ({count} frames): median {:.2} ms, p95 {:.2} ms, max {:.2} ms",
        durations[count / 2],
        durations[count * 95 / 100],
        durations[count - 1],
    );
}

#[test]
#[ignore = "requires a GPU; optionally set XUAN_ZOOM_BENCH_IMAGE to an image path"]
fn benchmark_large_image_editing() {
    let (context, mut app, state) = large_image_benchmark_app();
    let document = app.session().unwrap().document.clone();
    for tool in [
        Tool::Move,
        Tool::Marquee,
        Tool::Lasso,
        Tool::Brush,
        Tool::Erase,
    ] {
        app.sessions = vec![Session::new(
            document.clone(),
            "Editing benchmark".into(),
            None,
        )];
        app.tool = tool;
        app.snap = false;
        for _ in 0..3 {
            frame(&context, &mut app);
        }
        let rect = app.canvas_rect.unwrap();
        let start = rect.min + rect.size() * Vec2::new(0.25, 0.4);
        pointer_frame(&context, &mut app, start, Some(true), egui::Modifiers::NONE);
        let mut durations = Vec::new();
        let mut position = start;
        for step in 1..=24 {
            let progress = step as f32 / 24.0;
            position = start
                + rect.size()
                    * Vec2::new(
                        progress * 0.4,
                        (progress * std::f32::consts::TAU).sin() * 0.15,
                    );
            let now = std::time::Instant::now();
            let output = pointer_frame(&context, &mut app, position, None, egui::Modifiers::NONE);
            let _ = context.tessellate(output.shapes, output.pixels_per_point);
            state
                .device
                .poll(wgpu::PollType::wait_indefinitely())
                .unwrap();
            durations.push(now.elapsed().as_secs_f64() * 1000.0);
        }
        report_benchmark(tool.label(), durations);
        let now = std::time::Instant::now();
        pointer_frame(
            &context,
            &mut app,
            position,
            Some(false),
            egui::Modifiers::NONE,
        );
        for _ in 0..2 {
            frame(&context, &mut app);
        }
        state
            .device
            .poll(wgpu::PollType::wait_indefinitely())
            .unwrap();
        eprintln!(
            "{} release + settle: {:.2} ms",
            tool.label(),
            now.elapsed().as_secs_f64() * 1000.0
        );
        assert!(app.error.is_none(), "{:?}", app.error);
    }
}

#[test]
fn zoom_reuses_preview_and_thumbnails_until_document_changes() {
    let (context, mut app) = app();
    app.dimensions = [64, 48];
    app.new_document();
    for _ in 0..3 {
        frame(&context, &mut app);
    }
    let session = app.session().unwrap();
    let composite = session.composite.clone().unwrap();
    let texture = session.texture.as_ref().unwrap().id();
    let thumbnails: HashMap<_, _> = session
        .thumbnails
        .iter()
        .map(|(key, texture)| (*key, texture.id()))
        .collect();
    assert!(!thumbnails.is_empty());

    for zoom in [1.0, 0.51, 0.25, 0.05, 8.0, 0.01, 64.0] {
        app.session_mut().unwrap().zoom = zoom;
        let output = frame(&context, &mut app);
        let session = app.session().unwrap();
        assert_eq!(session.preview_size, [64, 48]);
        assert!(Arc::ptr_eq(session.composite.as_ref().unwrap(), &composite));
        assert!(
            !output
                .textures_delta
                .set
                .iter()
                .any(|(id, _)| *id == texture)
        );
        assert_eq!(session.thumbnails.len(), thumbnails.len());
        for (key, id) in &thumbnails {
            assert_eq!(session.thumbnails[key].id(), *id);
        }
    }

    app.command("fill_fg");
    frame(&context, &mut app);
    frame(&context, &mut app);
    let session = app.session().unwrap();
    assert!(!Arc::ptr_eq(
        session.composite.as_ref().unwrap(),
        &composite
    ));
    for (key, id) in thumbnails {
        assert_ne!(session.thumbnails[&key].id(), id);
    }
}

#[test]
fn selection_gestures_and_commands_reuse_the_composition_and_remain_undoable() {
    for tool in [Tool::Marquee, Tool::Lasso] {
        let (context, mut app) = app();
        app.dimensions = [64, 48];
        app.new_document();
        app.command("fill_fg");
        app.tool = tool;
        for _ in 0..3 {
            frame(&context, &mut app);
        }
        let session = app.session().unwrap();
        let composite = session.composite.clone().unwrap();
        let thumbnails: HashMap<_, _> = session
            .thumbnails
            .iter()
            .map(|(key, texture)| (*key, texture.id()))
            .collect();
        let origin = app.canvas_rect.unwrap().min;
        let zoom = session.zoom;
        let positions = [(10.0, 10.0), (40.0, 12.0), (45.0, 35.0)]
            .map(|(x, y)| origin + Vec2::new(x, y) * zoom);
        pointer_frame(
            &context,
            &mut app,
            positions[0],
            Some(true),
            egui::Modifiers::NONE,
        );
        for position in &positions[1..] {
            pointer_frame(&context, &mut app, *position, None, egui::Modifiers::NONE);
            frame(&context, &mut app);
            assert!(!app.session().unwrap().dirty_preview);
        }
        pointer_frame(
            &context,
            &mut app,
            positions[2],
            Some(false),
            egui::Modifiers::NONE,
        );
        frame(&context, &mut app);
        let session = app.session().unwrap();
        assert!(session.document.selection.is_some());
        assert!(Arc::ptr_eq(session.composite.as_ref().unwrap(), &composite));
        for (key, id) in &thumbnails {
            assert_eq!(session.thumbnails[key].id(), *id);
        }
        app.command("undo");
        assert!(app.session().unwrap().document.selection.is_none());
        app.command("redo");
        assert!(app.session().unwrap().document.selection.is_some());
        frame(&context, &mut app);
        let composite = app.session().unwrap().composite.clone().unwrap();
        for command in ["select_all", "invert_selection", "deselect"] {
            app.command(command);
            frame(&context, &mut app);
            assert!(Arc::ptr_eq(
                app.session().unwrap().composite.as_ref().unwrap(),
                &composite
            ));
        }
    }
}

#[test]
fn painting_refreshes_only_the_changed_layer_thumbnail() {
    let (context, mut app) = app();
    app.dimensions = [64, 48];
    app.new_document();
    app.command("fill_fg");
    app.command("duplicate");
    for _ in 0..3 {
        frame(&context, &mut app);
    }
    let session = app.session().unwrap();
    let base = session.document.layers[0].id;
    let painted = session.document.active.unwrap();
    let original_base = session.thumbnails[&(base, false)].id();
    let original_painted = session.thumbnails[&(painted, false)].id();
    app.tool = Tool::Brush;
    app.brush.color = [255, 0, 0, 255];
    app.brush.diameter = 4.0;
    drag(
        &context,
        &mut app,
        Point::new(12.0, 12.0),
        Point::new(30.0, 20.0),
        egui::Modifiers::NONE,
    );
    frame(&context, &mut app);
    let session = app.session().unwrap();
    assert_eq!(session.thumbnails[&(base, false)].id(), original_base);
    assert_ne!(session.thumbnails[&(painted, false)].id(), original_painted);
    assert_eq!(
        session.document.layers[0]
            .pixels
            .as_ref()
            .unwrap()
            .get_pixel(20, 16)
            .0,
        [0, 0, 0, 255]
    );
    assert_eq!(
        session
            .document
            .active()
            .unwrap()
            .pixels
            .as_ref()
            .unwrap()
            .get_pixel(20, 16)
            .0,
        [255, 0, 0, 255]
    );
    app.command("undo");
    frame(&context, &mut app);
    assert_eq!(
        app.session()
            .unwrap()
            .document
            .active()
            .unwrap()
            .pixels
            .as_ref()
            .unwrap()
            .get_pixel(20, 16)
            .0,
        [0, 0, 0, 255]
    );
}

fn app() -> (egui::Context, EditorApp) {
    let context = egui::Context::default();
    let app = EditorApp::with_context(&context, Vec::new(), false, None);
    (context, app)
}

fn keyboard_frame(
    context: &egui::Context,
    app: &mut EditorApp,
    events: Vec<egui::Event>,
    modifiers: egui::Modifiers,
) -> egui::FullOutput {
    context.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                Vec2::new(1280.0, 860.0),
            )),
            events,
            modifiers,
            ..Default::default()
        },
        |ctx| app.show(ctx),
    )
}

#[test]
fn native_clipboard_shortcuts_copy_cut_and_paste_selected_pixels() {
    let _clipboard_guard = CLIPBOARD_TEST_LOCK.lock().unwrap();
    let (context, mut app) = app();
    let mut document = Document::new(8, 6).unwrap();
    document.layers.clear();
    let source = RgbaImage::from_fn(8, 6, |x, _| {
        image::Rgba(if x < 4 { [255, 0, 0, 255] } else { [0; 4] })
    });
    document.insert(Layer::image(
        "Background",
        RgbaImage::from_pixel(8, 6, image::Rgba([0, 0, 255, 255])),
    ));
    document.insert(Layer::image("Source", source.clone()));
    let source_id = document.active.unwrap();
    app.sessions
        .push(Session::new(document, "Clipboard".into(), None));
    app.set_tool(Tool::Marquee);
    drag(
        &context,
        &mut app,
        Point::new(2.0, 1.0),
        Point::new(6.0, 4.0),
        egui::Modifiers::NONE,
    );
    assert_eq!(
        xuan::selection::bounds(
            app.session()
                .unwrap()
                .document
                .selection
                .as_deref()
                .unwrap()
        ),
        Some((2, 1, 6, 4))
    );
    let ctrl = egui::Modifiers {
        ctrl: true,
        command: true,
        ..Default::default()
    };

    // egui-winit emits clipboard events instead of C/X/V key presses.
    keyboard_frame(&context, &mut app, vec![egui::Event::Copy], ctrl);
    let (pixels, point) = app.clipboard.as_ref().expect("copy shortcut must run");
    assert_eq!(pixels.dimensions(), (4, 3));
    assert_eq!(*point, Point::new(2.0, 1.0));
    assert_eq!(pixels.get_pixel(0, 0).0, [255, 0, 0, 255]);
    assert_eq!(pixels.get_pixel(3, 0).0, [0; 4]);

    // An image-only system clipboard has no text payload.
    keyboard_frame(
        &context,
        &mut app,
        vec![egui::Event::Paste(String::new())],
        ctrl,
    );
    let document = &app.session().unwrap().document;
    assert_eq!(document.layers.len(), 3);
    let pasted = document.active().unwrap();
    assert_eq!(pasted.name, "Pasted image");
    assert_eq!(pasted.pixels.as_deref().unwrap().dimensions(), (4, 3));
    assert_eq!((pasted.transform.x, pasted.transform.y), (2.0, 1.0));
    assert_eq!(
        document
            .layers
            .iter()
            .find(|layer| layer.id == source_id)
            .unwrap()
            .pixels
            .as_deref()
            .unwrap(),
        &source
    );
    app.command("undo");
    assert_eq!(app.session().unwrap().document.layers.len(), 2);

    app.session_mut().unwrap().document.select(source_id, false);
    keyboard_frame(
        &context,
        &mut app,
        vec![egui::Event::Copy],
        ctrl | egui::Modifiers::SHIFT,
    );
    assert_eq!(
        app.clipboard.as_ref().unwrap().0.get_pixel(3, 0).0,
        [0, 0, 255, 255]
    );

    keyboard_frame(&context, &mut app, vec![egui::Event::Cut], ctrl);
    let document = &app.session().unwrap().document;
    let pixels = document
        .layers
        .iter()
        .find(|layer| layer.id == source_id)
        .unwrap()
        .pixels
        .as_deref()
        .unwrap();
    assert_eq!(pixels.get_pixel(2, 1).0[3], 0);
    assert_eq!(pixels.get_pixel(0, 0).0, [255, 0, 0, 255]);
    keyboard_frame(
        &context,
        &mut app,
        vec![egui::Event::Paste(String::new())],
        ctrl,
    );
    assert_eq!(app.session().unwrap().document.layers.len(), 3);
    assert_eq!(
        app.session()
            .unwrap()
            .document
            .active()
            .unwrap()
            .pixels
            .as_deref()
            .unwrap()
            .get_pixel(0, 0)
            .0,
        [255, 0, 0, 255]
    );
}

#[test]
fn marquee_copy_without_an_active_layer_replaces_the_previous_clipboard() {
    let _clipboard_guard = CLIPBOARD_TEST_LOCK.lock().unwrap();
    let (context, mut app) = app();
    let mut document = Document::new(64, 48).unwrap();
    let pixels = RgbaImage::from_pixel(64, 48, image::Rgba([31, 120, 200, 255]));
    let layer = Layer::image("Source", pixels);
    document.select(layer.id, false);
    document.layers = vec![layer];
    app.sessions
        .push(Session::new(document, "Marquee".into(), None));
    click_canvas(
        &context,
        &mut app,
        Point::new(-10.0, -10.0),
        egui::Modifiers::NONE,
    );
    assert!(app.session().unwrap().document.active.is_none());
    app.set_tool(Tool::Marquee);
    drag(
        &context,
        &mut app,
        Point::new(10.0, 8.0),
        Point::new(30.0, 20.0),
        egui::Modifiers::NONE,
    );
    app.clipboard = Some((RgbaImage::new(1, 1), Point::default()));
    keyboard_frame(
        &context,
        &mut app,
        vec![egui::Event::Copy],
        egui::Modifiers::CTRL,
    );
    let (copied, point) = app
        .clipboard
        .as_ref()
        .expect("Marquee copy must replace the old clipboard");
    assert_eq!(copied.dimensions(), (20, 12));
    assert_eq!(copied.get_pixel(0, 0).0, [31, 120, 200, 255]);
    assert_eq!(*point, Point::new(10.0, 8.0));
    keyboard_frame(
        &context,
        &mut app,
        vec![egui::Event::Paste(String::new())],
        egui::Modifiers::CTRL,
    );
    assert!(app.error.is_none(), "{:?}", app.error);
    let document = &app.session().unwrap().document;
    assert_eq!(document.layers.len(), 2);
    let pasted = document.active().unwrap();
    assert_eq!(pasted.pixels.as_deref().unwrap().dimensions(), (20, 12));
    assert_eq!((pasted.transform.x, pasted.transform.y), (10.0, 8.0));
    app.command("undo");
    assert_eq!(app.session().unwrap().document.layers.len(), 1);
}

#[test]
fn copy_and_cut_report_missing_targets_without_changing_pixels_or_clipboard() {
    let (_, mut app) = app();
    app.dimensions = [32, 24];
    app.new_document();
    app.command("fill_fg");
    let original = app.session().unwrap().document.layers[0].pixels.clone();
    let old_pixels = RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]));
    app.clipboard = Some((old_pixels.clone(), Point::default()));
    let document = &mut app.session_mut().unwrap().document;
    document.active = None;
    document.selected.clear();
    app.command("copy");
    assert_eq!(
        app.error.as_deref(),
        Some("Select a layer or make a selection before copying.")
    );
    assert_eq!(app.clipboard.as_ref().unwrap().0, old_pixels);

    app.error = None;
    app.command("select_all");
    app.command("cut");
    assert_eq!(
        app.error.as_deref(),
        Some("Select a layer before cutting pixels.")
    );
    assert_eq!(app.clipboard.as_ref().unwrap().0, old_pixels);
    assert_eq!(app.session().unwrap().document.layers[0].pixels, original);
}

#[test]
fn native_clipboard_shortcuts_leave_text_editing_to_the_focused_field() {
    let (context, mut app) = app();
    app.dimensions = [8, 6];
    app.new_document();
    let layer_id = app.session().unwrap().document.active.unwrap();
    let layer_count = app.session().unwrap().document.layers.len();
    app.start_layer_rename(layer_id);
    app.rename.as_mut().unwrap().name.clear();
    frame(&context, &mut app);
    assert!(context.wants_keyboard_input());
    keyboard_frame(
        &context,
        &mut app,
        vec![egui::Event::Paste("Layer name".into())],
        egui::Modifiers::CTRL,
    );
    assert_eq!(app.rename.as_ref().unwrap().name, "Layer name");
    for event in [
        egui::Event::Copy,
        egui::Event::Cut,
        egui::Event::Paste(String::new()),
    ] {
        keyboard_frame(&context, &mut app, vec![event], egui::Modifiers::CTRL);
        assert!(app.clipboard.is_none());
        assert_eq!(app.session().unwrap().document.layers.len(), layer_count);
    }
}

#[test]
fn clipboard_image_pixels_create_a_centered_layer_and_undo() {
    use super::clipboard::ClipboardContent;

    let (_, mut app) = app();
    app.dimensions = [20, 16];
    app.new_document();
    app.clipboard = Some((RgbaImage::new(1, 1), Point::new(7.0, 9.0)));
    app.mask_target = true;
    let pixels = RgbaImage::from_pixel(6, 4, image::Rgba([21, 87, 163, 127]));
    app.paste_content(ClipboardContent::Image(pixels.clone()));
    let document = &app.session().unwrap().document;
    assert_eq!(document.layers.len(), 2);
    let layer = document.active().unwrap();
    assert_eq!(layer.pixels.as_deref(), Some(&pixels));
    assert_eq!((layer.transform.x, layer.transform.y), (7.0, 6.0));
    assert!(!app.mask_target);
    assert!(app.clipboard.is_none());
    app.command("undo");
    assert_eq!(app.session().unwrap().document.layers.len(), 1);
    app.command("redo");
    assert_eq!(
        app.session()
            .unwrap()
            .document
            .active()
            .unwrap()
            .pixels
            .as_deref(),
        Some(&pixels)
    );
}

#[test]
fn clipboard_file_paste_imports_multiple_images_in_one_undo_step() {
    let temporary = tempfile::tempdir().unwrap();
    let files = [
        temporary.path().join("image one.png"),
        temporary.path().join("图片 #2.png"),
    ];
    let images = [
        RgbaImage::from_pixel(6, 4, image::Rgba([255, 0, 0, 255])),
        RgbaImage::from_pixel(4, 8, image::Rgba([0, 0, 255, 128])),
    ];
    for (path, pixels) in files.iter().zip(&images) {
        pixels.save(path).unwrap();
    }
    let original_bytes: Vec<_> = files
        .iter()
        .map(|path| std::fs::read(path).unwrap())
        .collect();
    let payload = format!(
        "cut\r\n{}\r\n{}\r\n",
        url::Url::from_file_path(&files[0]).unwrap(),
        url::Url::from_file_path(&files[1]).unwrap()
    );
    let (context, mut app) = app();
    app.dimensions = [20, 16];
    app.new_document();
    app.clipboard = Some((RgbaImage::new(1, 1), Point::default()));
    keyboard_frame(
        &context,
        &mut app,
        vec![egui::Event::Paste(payload)],
        egui::Modifiers::CTRL,
    );
    assert!(app.error.is_none(), "{:?}", app.error);
    let document = &app.session().unwrap().document;
    assert_eq!(document.layers.len(), 3);
    for (layer, pixels) in document.layers[1..].iter().zip(&images) {
        assert_eq!(layer.pixels.as_deref(), Some(pixels));
        assert_eq!(layer.transform.x, (20.0 - pixels.width() as f32) * 0.5);
        assert_eq!(layer.transform.y, (16.0 - pixels.height() as f32) * 0.5);
    }
    assert_eq!(document.layers[1].name, "image one");
    assert_eq!(document.layers[2].name, "图片 #2");
    assert!(app.clipboard.is_none());
    for (path, bytes) in files.iter().zip(&original_bytes) {
        assert_eq!(&std::fs::read(path).unwrap(), bytes);
    }
    app.command("undo");
    assert_eq!(app.session().unwrap().document.layers.len(), 1);
    app.command("redo");
    assert_eq!(app.session().unwrap().document.layers.len(), 3);
}

#[test]
fn clipboard_paste_creates_a_document_when_none_is_open() {
    use super::clipboard::ClipboardContent;

    let (_, mut app) = app();
    let pixels = RgbaImage::from_pixel(6, 4, image::Rgba([42, 69, 128, 255]));
    app.paste_content(ClipboardContent::Image(pixels.clone()));
    let document = &app.session().unwrap().document;
    assert_eq!((document.width, document.height), (6, 4));
    assert_eq!(document.active().unwrap().pixels.as_deref(), Some(&pixels));
}

#[test]
fn clipboard_open_creates_an_image_sized_document_without_changing_existing_tabs() {
    use super::clipboard::ClipboardContent;

    let (context, mut app) = app();
    let pixels = RgbaImage::from_pixel(6, 4, image::Rgba([21, 87, 163, 127]));
    app.clipboard = Some((pixels.clone(), Point::new(7.0, 9.0)));
    for expected_count in 1..=2 {
        app.mask_target = true;
        app.open_clipboard_content(ClipboardContent::Image(pixels.clone()))
            .unwrap();
        assert_eq!(app.sessions.len(), expected_count);
        assert_eq!(app.current, expected_count - 1);
        assert!(!app.mask_target);
        let session = app.session().unwrap();
        assert!(session.path.is_none());
        assert!(session.history.dirty());
        assert!(session.history.undo_name().is_none());
        for session in &app.sessions {
            let document = &session.document;
            assert_eq!((document.width, document.height), (6, 4));
            assert_eq!(document.layers.len(), 1);
            let layer = document.active().unwrap();
            assert_eq!(layer.pixels.as_deref(), Some(&pixels));
            assert_eq!((layer.transform.x, layer.transform.y), (0.0, 0.0));
            document.validate().unwrap();
        }
    }
    assert_ne!(app.sessions[0].document.id, app.sessions[1].document.id);
    app.command("close");
    assert_eq!(app.close_tab, Some(1));
    frame(&context, &mut app);
    assert_eq!(
        app.sessions.len(),
        2,
        "Unsaved clipboard images require a prompt"
    );
}

#[test]
fn clipboard_open_handles_empty_unavailable_and_invalid_images() {
    use super::clipboard::ClipboardContent;

    let (_, mut app) = app();
    app.open_clipboard_content(ClipboardContent::Unavailable)
        .unwrap();
    assert!(app.sessions.is_empty());
    assert!(app.status.contains("unavailable"));

    let pixels = RgbaImage::from_pixel(6, 4, image::Rgba([21, 87, 163, 127]));
    app.clipboard = Some((pixels.clone(), Point::new(7.0, 9.0)));
    app.open_clipboard_content(ClipboardContent::Unavailable)
        .unwrap();
    assert_eq!(
        app.session()
            .unwrap()
            .document
            .active()
            .unwrap()
            .pixels
            .as_deref(),
        Some(&pixels)
    );
    let original = serde_json::to_value(&app.session().unwrap().document).unwrap();

    app.open_clipboard_content(ClipboardContent::Empty).unwrap();
    assert!(app.clipboard.is_none());
    assert!(app.status.contains("does not contain an image"));
    app.open_clipboard_content(ClipboardContent::Unavailable)
        .unwrap();
    assert!(
        app.open_clipboard_content(ClipboardContent::Image(RgbaImage::new(0, 4)))
            .is_err()
    );
    assert_eq!(app.sessions.len(), 1);
    assert_eq!(
        serde_json::to_value(&app.session().unwrap().document).unwrap(),
        original
    );
}

#[test]
fn clipboard_open_files_creates_separate_tabs_and_reports_invalid_files() {
    use super::clipboard::ClipboardContent;

    let (_, mut app) = app();
    app.dimensions = [20, 16];
    app.new_document();
    let original = serde_json::to_value(&app.session().unwrap().document).unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let paths = [
        temporary.path().join("one.png"),
        temporary.path().join("two.png"),
    ];
    let pixels = RgbaImage::from_pixel(6, 4, image::Rgba([21, 87, 163, 127]));
    for path in &paths {
        pixels.save(path).unwrap();
    }
    app.open_clipboard_content(ClipboardContent::Files(paths.to_vec()))
        .unwrap();
    assert!(app.error.is_none(), "{:?}", app.error);
    assert_eq!(app.sessions.len(), 3);
    assert_eq!(
        serde_json::to_value(&app.sessions[0].document).unwrap(),
        original
    );
    for (session, title) in app.sessions[1..].iter().zip(["one", "two"]) {
        assert_eq!(session.title, title);
        assert_eq!((session.document.width, session.document.height), (6, 4));
        assert_eq!(session.document.layers.len(), 1);
        assert_eq!(
            session.document.active().unwrap().pixels.as_deref(),
            Some(&pixels)
        );
    }
    app.open_clipboard_content(ClipboardContent::Files(vec![
        temporary.path().join("missing.png"),
    ]))
    .unwrap();
    assert_eq!(app.sessions.len(), 3);
    assert!(app.error.as_ref().unwrap().contains("missing.png"));
}

#[test]
fn clipboard_invalid_files_do_not_partially_paste_or_reuse_cached_pixels() {
    use super::clipboard::ClipboardContent;

    let (_, mut app) = app();
    app.dimensions = [20, 16];
    app.new_document();
    let temporary = tempfile::tempdir().unwrap();
    let good = temporary.path().join("valid.png");
    let bad = temporary.path().join("invalid.png");
    RgbaImage::new(6, 4).save(&good).unwrap();
    std::fs::write(&bad, "Not an image").unwrap();
    app.clipboard = Some((RgbaImage::new(1, 1), Point::default()));
    app.paste_content(ClipboardContent::Files(vec![good, bad]));
    assert!(app.error.as_ref().unwrap().contains("invalid.png"));
    assert_eq!(app.session().unwrap().document.layers.len(), 1);
    assert!(app.clipboard.is_none());
    assert!(!app.session().unwrap().history.dirty());

    app.clipboard = Some((RgbaImage::new(1, 1), Point::default()));
    app.paste_content(ClipboardContent::Empty);
    app.paste_content(ClipboardContent::Unavailable);
    assert_eq!(app.session().unwrap().document.layers.len(), 1);
    assert!(app.clipboard.is_none());
}

#[test]
#[ignore = "requires an isolated desktop clipboard; run under Xvfb or Xephyr"]
fn system_clipboard_images_and_files_paste_from_another_process() {
    use std::io::{BufRead, Write};
    use std::process::{Command, Stdio};

    let pixels = RgbaImage::from_pixel(6, 4, image::Rgba([20, 80, 150, 127]));
    if let Some(path) = std::env::var_os("XUAN_CLIPBOARD_TEST_IMAGE") {
        let mut clipboard = arboard::Clipboard::new().unwrap();
        for line in std::io::stdin().lock().lines() {
            match line.unwrap().as_str() {
                "image" => clipboard
                    .set_image(arboard::ImageData {
                        width: pixels.width() as usize,
                        height: pixels.height() as usize,
                        bytes: std::borrow::Cow::Borrowed(pixels.as_raw()),
                    })
                    .unwrap(),
                "file" => clipboard.set().file_list(&[PathBuf::from(&path)]).unwrap(),
                "text" => clipboard.set_text("unrelated text").unwrap(),
                "inspect" => {
                    let image = clipboard
                        .get_image()
                        .expect("Copy must replace the old file list with image pixels");
                    assert_eq!((image.width, image.height), (4, 3));
                }
                _ => panic!("unexpected clipboard test command"),
            }
            println!("clipboard ready");
            std::io::stdout().flush().unwrap();
        }
        return;
    }

    struct Producer(std::process::Child);
    impl Drop for Producer {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    let _clipboard_guard = CLIPBOARD_TEST_LOCK.lock().unwrap();
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("external 图片.png");
    pixels.save(&path).unwrap();
    let mut producer = Producer(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "app::tests::system_clipboard_images_and_files_paste_from_another_process",
                "--ignored",
                "--nocapture",
            ])
            .env("XUAN_CLIPBOARD_TEST_IMAGE", &path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut input = producer.0.stdin.take().unwrap();
    let mut output = std::io::BufReader::new(producer.0.stdout.take().unwrap());
    let mut copy = |kind: &str| {
        writeln!(input, "{kind}").unwrap();
        input.flush().unwrap();
        loop {
            let mut line = String::new();
            assert!(
                output.read_line(&mut line).unwrap() > 0,
                "clipboard producer exited"
            );
            if line.trim() == "clipboard ready" {
                break;
            }
        }
    };

    let (context, mut app) = app();
    app.dimensions = [20, 16];
    app.new_document();
    for kind in ["image", "file"] {
        copy(kind);
        keyboard_frame(
            &context,
            &mut app,
            vec![egui::Event::Paste(String::new())],
            egui::Modifiers::CTRL,
        );
        assert!(app.error.is_none(), "{:?}", app.error);
        let layer = app.session().unwrap().document.active().unwrap();
        assert_eq!(layer.pixels.as_deref(), Some(&pixels));
        assert_eq!((layer.transform.x, layer.transform.y), (7.0, 6.0));
    }
    assert_eq!(app.session().unwrap().document.layers.len(), 3);
    assert_eq!(
        app.session().unwrap().document.active().unwrap().name,
        "external 图片"
    );
    app.command("paste");
    assert_eq!(app.session().unwrap().document.layers.len(), 4);
    copy("file");
    click_canvas(
        &context,
        &mut app,
        Point::new(-1.0, -1.0),
        egui::Modifiers::NONE,
    );
    assert!(app.session().unwrap().document.active.is_none());
    app.set_tool(Tool::Marquee);
    drag(
        &context,
        &mut app,
        Point::new(8.0, 6.0),
        Point::new(12.0, 9.0),
        egui::Modifiers::NONE,
    );
    keyboard_frame(
        &context,
        &mut app,
        vec![egui::Event::Copy],
        egui::Modifiers::CTRL,
    );
    assert!(app.error.is_none(), "{:?}", app.error);
    copy("inspect");
    keyboard_frame(
        &context,
        &mut app,
        vec![egui::Event::Paste(String::new())],
        egui::Modifiers::CTRL,
    );
    let document = &app.session().unwrap().document;
    assert_eq!(document.layers.len(), 5);
    let pasted = document.active().unwrap();
    assert_eq!(pasted.pixels.as_deref().unwrap().dimensions(), (4, 3));
    assert_eq!((pasted.transform.x, pasted.transform.y), (8.0, 6.0));
    copy("text");
    keyboard_frame(
        &context,
        &mut app,
        vec![egui::Event::Paste("unrelated text".into())],
        egui::Modifiers::CTRL,
    );
    assert_eq!(app.session().unwrap().document.layers.len(), 5);
    assert!(app.clipboard.is_none());

    // Exercise the File menu against another process's native clipboard.
    app.sessions.clear();
    for kind in ["image", "file"] {
        copy(kind);
        let count = app.sessions.len();
        for label in ["File", "Open Image from Clipboard"] {
            let position = layer_label(&context, &mut app, label) + Vec2::splat(5.0);
            pointer_frame(
                &context,
                &mut app,
                position,
                Some(true),
                egui::Modifiers::NONE,
            );
            pointer_frame(
                &context,
                &mut app,
                position,
                Some(false),
                egui::Modifiers::NONE,
            );
        }
        assert!(app.error.is_none(), "{:?}", app.error);
        assert!(!egui::Popup::is_any_open(&context));
        assert_eq!(app.sessions.len(), count + 1);
        let document = &app.session().unwrap().document;
        assert_eq!((document.width, document.height), (6, 4));
        assert_eq!(document.layers.len(), 1);
        assert_eq!(document.active().unwrap().pixels.as_deref(), Some(&pixels));
    }
    copy("text");
    app.command("open_clipboard");
    assert_eq!(app.sessions.len(), 2);
    assert!(app.status.contains("does not contain an image"));
}

fn frame(context: &egui::Context, app: &mut EditorApp) -> egui::FullOutput {
    let output = context.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                Vec2::new(1280.0, 860.0),
            )),
            ..Default::default()
        },
        |ctx| app.show(ctx),
    );
    assert!(!output.shapes.is_empty());
    output
}

fn pointer_frame(
    context: &egui::Context,
    app: &mut EditorApp,
    pos: Pos2,
    pressed: Option<bool>,
    modifiers: egui::Modifiers,
) -> egui::FullOutput {
    let mut events = vec![egui::Event::PointerMoved(pos)];
    if let Some(pressed) = pressed {
        events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        });
    }
    context.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                Vec2::new(1280.0, 860.0),
            )),
            events,
            modifiers,
            time: Some(app.frames as f64 / 60.0),
            ..Default::default()
        },
        |ctx| app.show(ctx),
    )
}

fn drag(
    context: &egui::Context,
    app: &mut EditorApp,
    from: Point,
    to: Point,
    modifiers: egui::Modifiers,
) {
    frame(context, app);
    let rect = app.canvas_rect.unwrap();
    let zoom = app.session().unwrap().zoom;
    let a = rect.min + Vec2::new(from.x, from.y) * zoom;
    let b = rect.min + Vec2::new(to.x, to.y) * zoom;
    pointer_frame(context, app, a, Some(true), modifiers);
    pointer_frame(context, app, a + (b - a) * 0.5, None, modifiers);
    pointer_frame(context, app, b, None, modifiers);
    pointer_frame(context, app, b, Some(false), modifiers);
}

fn click_canvas(
    context: &egui::Context,
    app: &mut EditorApp,
    point: Point,
    modifiers: egui::Modifiers,
) {
    frame(context, app);
    let pos =
        app.canvas_rect.unwrap().min + Vec2::new(point.x, point.y) * app.session().unwrap().zoom;
    pointer_frame(context, app, pos, None, modifiers);
    pointer_frame(context, app, pos, Some(true), modifiers);
    pointer_frame(context, app, pos, Some(false), modifiers);
}

fn layer_label(context: &egui::Context, app: &mut EditorApp, name: &str) -> Pos2 {
    frame(context, app)
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == name => Some(text.pos),
            _ => None,
        })
        .unwrap_or_else(|| panic!("Missing layer label: {name}"))
}

fn layer_eye(context: &egui::Context, app: &mut EditorApp, name: &str) -> Pos2 {
    let label = layer_label(context, app, name);
    frame(context, app)
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Circle(circle)
                if circle.radius == 2.0
                    && circle.center.x > label.x
                    && (label.y..label.y + 36.0).contains(&circle.center.y) =>
            {
                Some(circle.center)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("Missing visible layer eye: {name}"))
}

fn double_click_layer_name(context: &egui::Context, app: &mut EditorApp, name: &str) {
    let pos = layer_label(context, app, name) + Vec2::new(5.0, 5.0);
    for _ in 0..2 {
        pointer_frame(context, app, pos, Some(true), egui::Modifiers::NONE);
        pointer_frame(context, app, pos, Some(false), egui::Modifiers::NONE);
    }
    frame(context, app);
    assert!(app.rename.is_some(), "Double-click must rename {name}");
    assert!(context.wants_keyboard_input());
    assert!(app.dialog.is_none());
    assert!(app.develop.is_none());
}

#[test]
fn layer_name_double_click_renames_inline_with_one_undo_step() {
    let (context, mut app) = app();
    app.dimensions = [32, 24];
    app.new_document();
    let original = app
        .session()
        .unwrap()
        .document
        .active()
        .unwrap()
        .name
        .clone();
    let pos = layer_label(&context, &mut app, &original) + Vec2::new(5.0, 5.0);
    app.session_mut().unwrap().document.active = None;
    pointer_frame(&context, &mut app, pos, Some(true), egui::Modifiers::NONE);
    pointer_frame(&context, &mut app, pos, Some(false), egui::Modifiers::NONE);
    assert!(app.session().unwrap().document.active.is_some());
    assert!(app.rename.is_none());
    pointer_frame(&context, &mut app, pos, Some(true), egui::Modifiers::NONE);
    pointer_frame(&context, &mut app, pos, Some(false), egui::Modifiers::NONE);
    frame(&context, &mut app);
    assert!(context.wants_keyboard_input());
    assert!(app.dialog.is_none());
    let edited_pos = layer_label(&context, &mut app, &original);
    assert!(
        (edited_pos - pos).length() < 12.0,
        "Name must be edited in its row"
    );

    keyboard_frame(
        &context,
        &mut app,
        vec![egui::Event::Text("背景 – sky".into())],
        egui::Modifiers::NONE,
    );
    assert_eq!(app.rename.as_ref().unwrap().name, "背景 – sky");
    assert_eq!(
        app.session().unwrap().document.active().unwrap().name,
        original
    );
    assert_eq!(app.session().unwrap().history.names().count(), 0);
    keyboard_frame(
        &context,
        &mut app,
        vec![text_key(egui::Key::Enter, egui::Modifiers::NONE)],
        egui::Modifiers::NONE,
    );
    assert!(app.rename.is_none());
    assert!(!context.wants_keyboard_input());
    assert_eq!(
        app.session().unwrap().document.active().unwrap().name,
        "背景 – sky"
    );
    assert_eq!(
        app.session().unwrap().history.undo_name(),
        Some("Rename Layer")
    );
    assert_eq!(app.session().unwrap().history.names().count(), 1);
    app.command("undo");
    assert_eq!(
        app.session().unwrap().document.active().unwrap().name,
        original
    );
    app.command("redo");
    assert_eq!(
        app.session().unwrap().document.active().unwrap().name,
        "背景 – sky"
    );
}

#[test]
fn layer_name_rename_cancel_empty_and_unchanged_leave_history_clean() {
    for (replacement, key) in [
        (Some("Cancelled"), egui::Key::Escape),
        (Some("   "), egui::Key::Enter),
        (None, egui::Key::Enter),
    ] {
        let (context, mut app) = app();
        app.dimensions = [32, 24];
        app.new_document();
        let original = app
            .session()
            .unwrap()
            .document
            .active()
            .unwrap()
            .name
            .clone();
        double_click_layer_name(&context, &mut app, &original);
        if let Some(name) = replacement {
            keyboard_frame(
                &context,
                &mut app,
                vec![egui::Event::Text(name.into())],
                egui::Modifiers::NONE,
            );
        }
        keyboard_frame(
            &context,
            &mut app,
            vec![text_key(key, egui::Modifiers::NONE)],
            egui::Modifiers::NONE,
        );
        assert!(app.rename.is_none());
        assert!(!context.wants_keyboard_input());
        assert_eq!(
            app.session().unwrap().document.active().unwrap().name,
            original
        );
        assert_eq!(app.session().unwrap().history.names().count(), 0);
        assert!(app.error.is_none());
    }
}

#[test]
fn layer_name_rename_saves_on_click_away_for_special_layer_types() {
    for kind in ["Group", "Mask", "Adjustment", "Filter", "Text", "Locked"] {
        let (context, mut app) = app();
        app.dimensions = [32, 24];
        app.new_document();
        let name = format!("{kind} layer");
        let mut target = Layer::blank(&name, 32, 24);
        match kind {
            "Group" => target.group = true,
            "Mask" => target = Layer::mask(&name, 32, 24),
            "Adjustment" => target.adjustment = Some(Adjustment::Invert),
            "Filter" => target.filter = Some(Filter::GaussianBlur { radius: 1.0 }),
            "Text" => target.text = Some(xuan::text::TextStyle::default()),
            "Locked" => target.locked = true,
            _ => unreachable!(),
        }
        let id = target.id;
        let other = Layer::blank("Other", 32, 24);
        let other_id = other.id;
        let document = &mut app.session_mut().unwrap().document;
        document.layers = vec![other, target];
        document.select(other_id, false);
        let other_pos = layer_label(&context, &mut app, "Other") + Vec2::new(5.0, 5.0);
        double_click_layer_name(&context, &mut app, &name);
        keyboard_frame(
            &context,
            &mut app,
            vec![egui::Event::Text("Renamed".into())],
            egui::Modifiers::NONE,
        );
        pointer_frame(
            &context,
            &mut app,
            other_pos,
            Some(true),
            egui::Modifiers::NONE,
        );
        pointer_frame(
            &context,
            &mut app,
            other_pos,
            Some(false),
            egui::Modifiers::NONE,
        );
        assert!(app.rename.is_none());
        let session = app.session().unwrap();
        assert_eq!(session.document.active, Some(other_id));
        assert_eq!(
            session
                .document
                .layers
                .iter()
                .find(|l| l.id == id)
                .unwrap()
                .name,
            "Renamed"
        );
        assert_eq!(session.history.names().count(), 1);
        assert!(app.error.is_none(), "{:?}", app.error);
    }
}

fn drag_pointer(
    context: &egui::Context,
    app: &mut EditorApp,
    from: Pos2,
    to: Pos2,
    modifiers: egui::Modifiers,
) {
    pointer_frame(context, app, from, Some(true), modifiers);
    pointer_frame(context, app, from + Vec2::new(0.0, 10.0), None, modifiers);
    pointer_frame(context, app, to, None, modifiers);
    pointer_frame(context, app, to, Some(false), modifiers);
}

#[test]
fn layer_visibility_eye_toggles_preview_without_selecting_and_supports_undo() {
    let (context, mut app) = app();
    app.dimensions = [32, 24];
    app.new_document();
    let bottom = Layer::image(
        "Bottom",
        RgbaImage::from_pixel(32, 24, image::Rgba([0, 0, 255, 255])),
    );
    let mut top = Layer::image(
        "Top",
        RgbaImage::from_pixel(32, 24, image::Rgba([255, 0, 0, 255])),
    );
    top.locked = true;
    let selected = bottom.id;
    let document = &mut app.session_mut().unwrap().document;
    document.layers = vec![bottom, top];
    document.select(selected, false);

    let eye = layer_eye(&context, &mut app, "Top");
    let assert_visible = |app: &EditorApp, visible: bool| {
        let session = app.session().unwrap();
        assert_eq!(session.document.layers[1].visible, visible);
        assert_eq!(session.document.active, Some(selected));
        assert_eq!(session.document.selected, HashSet::from([selected]));
        assert_eq!(
            session.composite.as_ref().unwrap().get_pixel(16, 12).0,
            if visible {
                [255, 0, 0, 255]
            } else {
                [0, 0, 255, 255]
            }
        );
        assert!(app.rename.is_none());
        assert!(app.error.is_none(), "{:?}", app.error);
    };
    assert_visible(&app, true);

    pointer_frame(&context, &mut app, eye, Some(true), egui::Modifiers::NONE);
    pointer_frame(&context, &mut app, eye, Some(false), egui::Modifiers::NONE);
    assert_visible(&app, false);
    assert_eq!(
        app.session().unwrap().history.undo_name(),
        Some("Layer Visibility")
    );
    assert_eq!(app.session().unwrap().history.names().count(), 1);

    app.command("undo");
    frame(&context, &mut app);
    assert_visible(&app, true);
    app.command("redo");
    frame(&context, &mut app);
    assert_visible(&app, false);

    pointer_frame(&context, &mut app, eye, Some(true), egui::Modifiers::NONE);
    pointer_frame(&context, &mut app, eye, Some(false), egui::Modifiers::NONE);
    assert_visible(&app, true);
    assert_eq!(app.session().unwrap().history.names().count(), 2);
}

#[test]
fn collapsed_group_visibility_eye_toggles_children_in_preview() {
    let (context, mut app) = app();
    app.dimensions = [32, 24];
    app.new_document();
    let mut group = Layer::blank("Group", 32, 24);
    group.group = true;
    let group_id = group.id;
    let mut child = Layer::image(
        "Child",
        RgbaImage::from_pixel(32, 24, image::Rgba([255, 0, 0, 255])),
    );
    child.parent = Some(group_id);
    let child_id = child.id;
    let session = app.session_mut().unwrap();
    session.document.layers = vec![group, child];
    session.document.select(child_id, false);
    session.collapsed.insert(group_id);

    let eye = layer_eye(&context, &mut app, "Group");
    for visible in [false, true] {
        pointer_frame(&context, &mut app, eye, Some(true), egui::Modifiers::NONE);
        pointer_frame(&context, &mut app, eye, Some(false), egui::Modifiers::NONE);
        let session = app.session().unwrap();
        assert_eq!(session.document.layers[0].visible, visible);
        assert!(session.document.layers[1].visible);
        assert_eq!(session.document.active, Some(child_id));
        assert!(session.collapsed.contains(&group_id));
        assert_eq!(
            session.composite.as_ref().unwrap().get_pixel(16, 12).0,
            if visible { [255, 0, 0, 255] } else { [0; 4] }
        );
        assert!(app.rename.is_none());
        assert!(app.error.is_none(), "{:?}", app.error);
    }
}

#[test]
fn layer_rows_and_thumbnails_reorder_in_both_directions_and_undo() {
    let (context, mut app) = app();
    app.dimensions = [32, 24];
    app.new_document();
    let document = &mut app.session_mut().unwrap().document;
    document.layers = ["Bottom", "Middle", "Top"]
        .map(|name| Layer::blank(name, 32, 24))
        .into();
    document.select(document.layers[2].id, false);

    // Start on the row's padding, then drop below the last row.
    let top = layer_label(&context, &mut app, "Top") + Vec2::new(100.0, 30.0);
    let bottom = layer_label(&context, &mut app, "Bottom") + Vec2::new(5.0, 30.0);
    drag_pointer(&context, &mut app, top, bottom, egui::Modifiers::NONE);
    let names = |app: &EditorApp| {
        app.session()
            .unwrap()
            .document
            .layers
            .iter()
            .map(|layer| layer.name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&app), ["Top", "Bottom", "Middle"]);
    assert_eq!(app.session().unwrap().history.names().count(), 1);
    app.command("undo");
    assert_eq!(names(&app), ["Bottom", "Middle", "Top"]);
    app.command("redo");
    assert_eq!(names(&app), ["Top", "Bottom", "Middle"]);

    // Start on a thumbnail and drop above the first row.
    let bottom = layer_label(&context, &mut app, "Top") + Vec2::new(-24.0, 15.0);
    let top = layer_label(&context, &mut app, "Middle") + Vec2::new(5.0, 0.0);
    drag_pointer(&context, &mut app, bottom, top, egui::Modifiers::NONE);
    assert_eq!(names(&app), ["Bottom", "Middle", "Top"]);
    assert!(app.error.is_none(), "{:?}", app.error);
}

#[test]
fn layer_drop_indicator_stays_on_the_shared_boundary_between_rows() {
    let (context, mut app) = app();
    app.dimensions = [32, 24];
    app.new_document();
    let document = &mut app.session_mut().unwrap().document;
    document.layers = ["Bottom", "Middle", "Top"]
        .map(|name| Layer::blank(name, 32, 24))
        .into();
    document.select(document.layers[2].id, false);

    let top = layer_label(&context, &mut app, "Top") + Vec2::new(60.0, 15.0);
    let middle = layer_label(&context, &mut app, "Middle");
    let bottom = layer_label(&context, &mut app, "Bottom");
    pointer_frame(&context, &mut app, top, Some(true), egui::Modifiers::NONE);
    pointer_frame(
        &context,
        &mut app,
        top + Vec2::new(0.0, 10.0),
        None,
        egui::Modifiers::NONE,
    );
    let indicator = |output: egui::FullOutput| {
        let lines: Vec<_> = output
            .shapes
            .into_iter()
            .filter_map(|shape| match shape.shape {
                egui::Shape::LineSegment { points, stroke }
                    if stroke.color == theme::ACCENT
                        && stroke.width == 2.0
                        && points[0].y == points[1].y
                        && (middle.y..bottom.y + 36.0).contains(&points[0].y) =>
                {
                    Some(points)
                }
                _ => None,
            })
            .collect();
        assert_eq!(lines.len(), 1, "Expected one insertion line: {lines:?}");
        lines[0]
    };

    let below_middle = indicator(pointer_frame(
        &context,
        &mut app,
        middle + Vec2::new(60.0, 30.0),
        None,
        egui::Modifiers::NONE,
    ));
    let above_bottom = indicator(pointer_frame(
        &context,
        &mut app,
        bottom + Vec2::new(60.0, 0.0),
        None,
        egui::Modifiers::NONE,
    ));
    assert_eq!(below_middle, above_bottom);

    let boundary = Pos2::new(middle.x + 60.0, below_middle[0].y);
    for offset in [-1.0, 0.0, 1.0] {
        assert_eq!(
            indicator(pointer_frame(
                &context,
                &mut app,
                boundary + Vec2::new(0.0, offset),
                None,
                egui::Modifiers::NONE,
            )),
            below_middle,
        );
    }
    pointer_frame(
        &context,
        &mut app,
        boundary,
        Some(false),
        egui::Modifiers::NONE,
    );
    let document = &app.session().unwrap().document;
    let names: Vec<_> = document
        .layers
        .iter()
        .map(|layer| layer.name.as_str())
        .collect();
    assert_eq!(names, ["Bottom", "Top", "Middle"]);
    assert!(app.error.is_none(), "{:?}", app.error);
}

#[test]
fn layer_drops_nest_duplicate_and_reject_descendants() {
    let (context, mut app) = app();
    app.dimensions = [32, 24];
    app.new_document();
    app.command("group");
    let folder = app.session().unwrap().document.active.unwrap();
    app.session_mut()
        .unwrap()
        .document
        .active_mut()
        .unwrap()
        .name = "Folder".into();
    let loose = Layer::blank("Loose", 32, 24);
    let loose_id = loose.id;
    app.session_mut().unwrap().document.layers.push(loose);
    let from = layer_label(&context, &mut app, "Loose") + Vec2::new(5.0, 5.0);
    let into = layer_label(&context, &mut app, "Folder") + Vec2::new(5.0, 18.0);
    drag_pointer(&context, &mut app, from, into, egui::Modifiers::ALT);
    let document = &app.session().unwrap().document;
    assert_eq!(document.layers.len(), 4);
    let copy = document.active().unwrap();
    assert_eq!(copy.name, "Loose copy");
    assert_eq!(copy.parent, Some(folder));
    assert_eq!(
        document
            .layers
            .iter()
            .find(|layer| layer.id == loose_id)
            .unwrap()
            .parent,
        None
    );
    assert!(
        layer_label(&context, &mut app, "Loose copy").y
            < layer_label(&context, &mut app, "Layer 1").y
    );

    let revision = app.session().unwrap().history.revision;
    let from = layer_label(&context, &mut app, "Folder") + Vec2::new(5.0, 5.0);
    let descendant = layer_label(&context, &mut app, "Loose copy") + Vec2::new(5.0, 5.0);
    drag_pointer(&context, &mut app, from, descendant, egui::Modifiers::NONE);
    assert_eq!(app.session().unwrap().history.revision, revision);

    // Dropping on a folder's lower edge moves the child out, below the folder.
    let from = layer_label(&context, &mut app, "Loose copy") + Vec2::new(5.0, 5.0);
    let below = layer_label(&context, &mut app, "Folder") + Vec2::new(5.0, 34.0);
    drag_pointer(&context, &mut app, from, below, egui::Modifiers::NONE);
    let document = &app.session().unwrap().document;
    assert_eq!(document.active().unwrap().parent, None);
    assert!(
        document
            .layers
            .iter()
            .position(|layer| Some(layer.id) == document.active)
            .unwrap()
            < document
                .layers
                .iter()
                .position(|layer| layer.id == folder)
                .unwrap()
    );
    document.validate().unwrap();
    assert!(app.error.is_none(), "{:?}", app.error);
}

fn canvas_layers(app: &mut EditorApp) -> [Uuid; 2] {
    app.dimensions = [100, 80];
    app.new_document();
    let mut bottom = Layer::image(
        "Bottom",
        RgbaImage::from_pixel(50, 40, image::Rgba([255; 4])),
    );
    bottom.transform.x = 10.0;
    bottom.transform.y = 10.0;
    let mut top = Layer::image("Top", RgbaImage::from_pixel(20, 20, image::Rgba([255; 4])));
    top.transform.x = 20.0;
    top.transform.y = 20.0;
    // A hole in the top layer should select the visible layer below it.
    Arc::make_mut(top.pixels.as_mut().unwrap()).put_pixel(5, 5, image::Rgba([0; 4]));
    let ids = [bottom.id, top.id];
    let document = &mut app.session_mut().unwrap().document;
    document.layers = vec![bottom, top];
    document.select(ids[0], false);
    app.snap = false;
    ids
}

#[test]
fn canvas_clicks_select_visible_layers_and_deselect_empty_space() {
    let (context, mut app) = app();
    let [bottom, top] = canvas_layers(&mut app);
    assert!(app.auto_select);
    assert!(app.ignore_transparent_pixels);
    click_canvas(
        &context,
        &mut app,
        Point::new(30.0, 30.0),
        egui::Modifiers::NONE,
    );
    assert_eq!(app.session().unwrap().document.active, Some(top));
    click_canvas(
        &context,
        &mut app,
        Point::new(25.5, 25.5),
        egui::Modifiers::NONE,
    );
    assert_eq!(app.session().unwrap().document.active, Some(bottom));
    app.session_mut().unwrap().document.layers[1].visible = false;
    click_canvas(
        &context,
        &mut app,
        Point::new(30.0, 30.0),
        egui::Modifiers::NONE,
    );
    assert_eq!(app.session().unwrap().document.active, Some(bottom));
    app.mask_target = true;
    click_canvas(
        &context,
        &mut app,
        Point::new(80.0, 65.0),
        egui::Modifiers::NONE,
    );
    assert!(app.session().unwrap().document.active.is_none());
    assert!(app.session().unwrap().document.selected.is_empty());
    assert!(!app.mask_target);
    assert!(operations::transform_box(&app.session().unwrap().document, false).is_none());

    click_canvas(
        &context,
        &mut app,
        Point::new(30.0, 30.0),
        egui::Modifiers::NONE,
    );
    assert_eq!(app.session().unwrap().document.active, Some(bottom));
    click_canvas(
        &context,
        &mut app,
        Point::new(-5.0, 30.0),
        egui::Modifiers::NONE,
    );
    assert!(app.session().unwrap().document.active.is_none());
    assert!(app.session().unwrap().document.selected.is_empty());
    assert!(!app.session().unwrap().history.dirty());
}

#[test]
fn move_tool_can_select_and_drag_through_transparent_pixels() {
    let (context, mut app) = app();
    let [bottom, top] = canvas_layers(&mut app);
    app.ignore_transparent_pixels = false;

    let hole = Point::new(25.5, 25.5);
    click_canvas(&context, &mut app, hole, egui::Modifiers::NONE);
    assert_eq!(app.session().unwrap().document.active, Some(top));

    // Starting on an unselected layer's transparent pixel selects and moves it.
    app.session_mut().unwrap().document.select(bottom, false);
    drag(
        &context,
        &mut app,
        hole,
        Point::new(30.5, 30.5),
        egui::Modifiers::NONE,
    );
    let document = &app.session().unwrap().document;
    assert_eq!(document.active, Some(top));
    assert_eq!(document.layers[0].transform.x, 10.0);
    assert_eq!(document.layers[0].transform.y, 10.0);
    assert_eq!(document.layers[1].transform.x, 25.0);
    assert_eq!(document.layers[1].transform.y, 25.0);
    app.command("undo");
    assert_eq!(app.session().unwrap().document.layers[1].transform.x, 20.0);

    app.session_mut().unwrap().document.select(bottom, false);
    app.auto_select = false;
    click_canvas(&context, &mut app, hole, egui::Modifiers::NONE);
    assert_eq!(app.session().unwrap().document.active, Some(bottom));
    click_canvas(&context, &mut app, hole, egui::Modifiers::CTRL);
    assert_eq!(app.session().unwrap().document.active, Some(top));

    let option = frame(&context, &mut app)
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == "Ignore Transparent Pixels" => {
                Some(text.pos + Vec2::splat(5.0))
            }
            _ => None,
        })
        .expect("Move tool transparency option");
    pointer_frame(&context, &mut app, option, None, egui::Modifiers::NONE);
    pointer_frame(
        &context,
        &mut app,
        option,
        Some(true),
        egui::Modifiers::NONE,
    );
    pointer_frame(
        &context,
        &mut app,
        option,
        Some(false),
        egui::Modifiers::NONE,
    );
    assert!(app.ignore_transparent_pixels);
    click_canvas(&context, &mut app, hole, egui::Modifiers::CTRL);
    assert_eq!(app.session().unwrap().document.active, Some(bottom));
}

#[test]
fn canvas_selection_preserves_multiselect_and_respects_auto_select() {
    let (context, mut app) = app();
    let [bottom, top] = canvas_layers(&mut app);
    click_canvas(
        &context,
        &mut app,
        Point::new(30.0, 30.0),
        egui::Modifiers::SHIFT,
    );
    assert_eq!(
        app.session().unwrap().document.selected,
        HashSet::from([bottom, top])
    );
    drag(
        &context,
        &mut app,
        Point::new(30.0, 30.0),
        Point::new(35.0, 35.0),
        egui::Modifiers::NONE,
    );
    let document = &app.session().unwrap().document;
    assert_eq!(document.selected, HashSet::from([bottom, top]));
    assert_eq!(document.layers[0].transform.x, 15.0);
    assert_eq!(document.layers[1].transform.x, 25.0);
    app.command("undo");
    click_canvas(
        &context,
        &mut app,
        Point::new(30.0, 30.0),
        egui::Modifiers::SHIFT,
    );
    assert_eq!(
        app.session().unwrap().document.selected,
        HashSet::from([bottom])
    );
    assert_eq!(app.session().unwrap().document.active, Some(bottom));
    click_canvas(
        &context,
        &mut app,
        Point::new(15.0, 15.0),
        egui::Modifiers::SHIFT,
    );
    assert!(app.session().unwrap().document.active.is_none());
    assert!(app.session().unwrap().document.selected.is_empty());

    app.session_mut().unwrap().document.select(bottom, false);
    app.auto_select = false;
    click_canvas(
        &context,
        &mut app,
        Point::new(30.0, 30.0),
        egui::Modifiers::NONE,
    );
    assert_eq!(app.session().unwrap().document.active, Some(bottom));
    click_canvas(
        &context,
        &mut app,
        Point::new(30.0, 30.0),
        egui::Modifiers::CTRL,
    );
    assert_eq!(app.session().unwrap().document.active, Some(top));
}

#[test]
fn empty_canvas_drags_and_panning_do_not_move_or_select_layers() {
    let (context, mut app) = app();
    let [bottom, _] = canvas_layers(&mut app);
    drag(
        &context,
        &mut app,
        Point::new(80.0, 65.0),
        Point::new(85.0, 70.0),
        egui::Modifiers::NONE,
    );
    let document = &app.session().unwrap().document;
    assert!(document.active.is_none());
    assert_eq!(document.layers[0].transform.x, 10.0);
    assert!(!app.session().unwrap().history.dirty());

    app.session_mut().unwrap().document.select(bottom, false);
    frame(&context, &mut app);
    let pos = app.canvas_rect.unwrap().min + Vec2::new(30.0, 30.0) * app.session().unwrap().zoom;
    let _ = context.run(
        egui::RawInput {
            events: vec![egui::Event::Key {
                key: egui::Key::Space,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        },
        |ctx| app.show(ctx),
    );
    pointer_frame(&context, &mut app, pos, Some(true), egui::Modifiers::NONE);
    pointer_frame(&context, &mut app, pos, Some(false), egui::Modifiers::NONE);
    assert_eq!(app.session().unwrap().document.active, Some(bottom));
    drag_pointer(
        &context,
        &mut app,
        pos,
        pos + Vec2::new(30.0, 20.0),
        egui::Modifiers::NONE,
    );
    assert_eq!(app.session().unwrap().document.active, Some(bottom));
    assert!(app.session().unwrap().pan.length() > 20.0);
    assert!(!app.session().unwrap().history.dirty());
}

#[test]
fn panning_preserves_saved_state_and_history_throughout_the_drag() {
    for (tool, button, space) in [
        (Tool::Move, egui::PointerButton::Middle, false),
        (Tool::Move, egui::PointerButton::Primary, true),
        (Tool::Hand, egui::PointerButton::Primary, false),
        (Tool::Clone, egui::PointerButton::Middle, false),
    ] {
        for dirty in [false, true] {
            let (context, mut app) = app();
            app.dimensions = [64, 48];
            app.new_document();
            app.command("fill_fg");
            if !dirty {
                app.session_mut().unwrap().history.mark_saved();
            }
            app.command("fill_bg");
            app.command("undo");
            app.set_tool(tool);
            frame(&context, &mut app);
            let session = app.session().unwrap();
            let pan = session.pan;
            let revision = session.history.revision;
            let undo = session.history.undo_name().map(str::to_owned);
            let redo = session.history.redo_name().map(str::to_owned);
            let document = session.document.clone();
            let title = app.window_title.clone();
            let start = app.canvas_rect.unwrap().center();
            if space {
                keyboard_frame(
                    &context,
                    &mut app,
                    vec![text_key(egui::Key::Space, egui::Modifiers::NONE)],
                    egui::Modifiers::NONE,
                );
            }
            for (delta, pressed) in [
                (Vec2::ZERO, Some(true)),
                (Vec2::new(15.0, 10.0), None),
                (Vec2::new(30.0, 20.0), None),
                (Vec2::new(30.0, 20.0), Some(false)),
            ] {
                let pos = start + delta;
                let mut events = vec![egui::Event::PointerMoved(pos)];
                if let Some(pressed) = pressed {
                    events.push(egui::Event::PointerButton {
                        pos,
                        button,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    });
                }
                keyboard_frame(&context, &mut app, events, egui::Modifiers::NONE);
                let session = app.session().unwrap();
                assert_eq!(session.history.dirty(), dirty);
                assert_eq!(session.history.revision, revision);
                assert_eq!(session.history.undo_name(), undo.as_deref());
                assert_eq!(session.history.redo_name(), redo.as_deref());
                assert_eq!(session.history.names().count(), 1);
                assert_eq!(app.window_title, title);
                assert_eq!(session.pan, pan + delta);
                assert_eq!(session.document.active, document.active);
                assert_eq!(session.document.selected, document.selected);
                assert_eq!(session.document.layers.len(), document.layers.len());
                assert_eq!(session.document.layers[0].pixels, document.layers[0].pixels);
                assert_eq!(
                    session.document.layers[0].transform,
                    document.layers[0].transform
                );
                if pressed.is_none() {
                    assert!(app.gesture.as_ref().is_some_and(|gesture| gesture.panning));
                }
            }
            assert!(app.gesture.is_none());
            assert!(app.clone_source.is_none());
            assert!(app.clone_offset.is_none());
            app.command("redo");
            assert_eq!(app.session().unwrap().history.names().count(), 2);
        }
    }
}

#[test]
fn gradient_gestures_respect_the_mask_target_and_undo() {
    for radial in [false, true] {
        for mask_target in [true, false] {
            let (context, mut app) = app();
            app.dimensions = [64, 48];
            app.new_document();
            paint::fill(
                &mut app.session_mut().unwrap().document,
                [50, 100, 150, 255],
                false,
                false,
            )
            .unwrap();
            app.command("mask");
            let owner = app
                .session()
                .unwrap()
                .document
                .active()
                .unwrap()
                .parent
                .unwrap();
            let original_pixels = app
                .session()
                .unwrap()
                .document
                .layers
                .iter()
                .find(|l| l.id == owner)
                .unwrap()
                .pixels
                .clone();
            if !mask_target {
                app.session_mut().unwrap().document.select(owner, false);
            }
            app.mask_target = mask_target;
            app.set_tool(Tool::Gradient);
            app.radial = radial;
            app.brush.color = [0, 0, 0, 255];
            app.background = [255; 4];
            let before = app.session().unwrap().document.active().unwrap().clone();

            drag(
                &context,
                &mut app,
                Point::new(10.5, 20.5),
                Point::new(50.5, 20.5),
                egui::Modifiers::NONE,
            );

            assert!(app.error.is_none(), "{:?}", app.error);
            let session = app.session().unwrap();
            assert_eq!(session.history.undo_name(), Some("Gradient"));
            let after = session.document.active().unwrap().clone();
            if mask_target {
                assert_eq!(
                    session
                        .document
                        .layers
                        .iter()
                        .find(|l| l.id == owner)
                        .unwrap()
                        .pixels,
                    original_pixels
                );
                let mask = &after.mask.as_ref().unwrap().pixels;
                assert!(mask.get_pixel(10, 20)[0] <= 1);
                assert!((127..=129).contains(&mask.get_pixel(30, 20)[0]));
                assert!(mask.get_pixel(50, 20)[0] >= 254);
            } else {
                assert!(before.mask.is_none() && after.mask.is_none());
                let pixels = after.pixels.as_ref().unwrap();
                assert!(pixels.get_pixel(10, 20)[0] <= 1);
                assert!((127..=129).contains(&pixels.get_pixel(30, 20)[0]));
                assert!(pixels.get_pixel(50, 20)[0] >= 254);
            }

            for (command, expected) in [("undo", before), ("redo", after)] {
                app.command(command);
                let layer = app.session().unwrap().document.active().unwrap();
                assert_eq!(layer.pixels, expected.pixels);
                assert_eq!(
                    layer.mask.as_ref().map(|m| &m.pixels),
                    expected.mask.as_ref().map(|m| &m.pixels),
                );
            }
        }
    }
}

#[test]
fn pointer_brush_selection_and_pixel_move_are_undoable() {
    let (context, mut app) = app();
    app.dimensions = [64, 48];
    app.new_document();
    app.brush.diameter = 4.0;
    app.brush.hardness = 1.0;
    app.brush.color = [255, 0, 0, 255];
    app.set_tool(Tool::Brush);
    drag(
        &context,
        &mut app,
        Point::new(10.0, 20.0),
        Point::new(30.0, 20.0),
        egui::Modifiers::NONE,
    );
    assert!(app.error.is_none(), "{:?}", app.error);
    assert_eq!(
        render::render(&app.session().unwrap().document)
            .get_pixel(20, 20)
            .0,
        [255, 0, 0, 255]
    );
    app.set_tool(Tool::Marquee);
    drag(
        &context,
        &mut app,
        Point::new(8.0, 16.0),
        Point::new(33.0, 25.0),
        egui::Modifiers::NONE,
    );
    assert_eq!(
        xuan::selection::bounds(app.session().unwrap().document.selection.as_ref().unwrap()),
        Some((8, 16, 33, 25))
    );
    drag(
        &context,
        &mut app,
        Point::new(15.0, 20.0),
        Point::new(35.0, 30.0),
        egui::Modifiers::CTRL,
    );
    let doc = &app.session().unwrap().document;
    assert_eq!(doc.layers.len(), 2);
    assert_eq!(render::render(doc).get_pixel(40, 30).0, [255, 0, 0, 255]);
    app.command("undo");
    assert_eq!(app.session().unwrap().document.layers.len(), 1);
    assert_eq!(
        render::render(&app.session().unwrap().document)
            .get_pixel(20, 20)
            .0,
        [255, 0, 0, 255]
    );
}

#[test]
fn transform_handles_and_control_drag_distortion_change_geometry() {
    let (context, mut app) = app();
    app.dimensions = [64, 48];
    app.new_document();
    app.command("fill_fg");
    app.snap = false;
    app.lock_ratio = false;
    drag(
        &context,
        &mut app,
        Point::new(64.0, 48.0),
        Point::new(70.0, 52.0),
        egui::Modifiers::NONE,
    );
    assert!(
        (app.session()
            .unwrap()
            .document
            .active()
            .unwrap()
            .transform
            .width
            - 70.0)
            .abs()
            < 0.01
    );
    drag(
        &context,
        &mut app,
        Point::new(0.0, 0.0),
        Point::new(6.0, 8.0),
        egui::Modifiers::CTRL,
    );
    let transform = app.session().unwrap().document.active().unwrap().transform;
    assert!(transform.warp.is_some());
    assert!(
        transform
            .point(Point::new(0.0, 0.0))
            .distance(Point::new(6.0, 8.0))
            < 0.01
    );
}

#[test]
fn welcome_and_all_tool_panels_render_without_panics() {
    let (context, mut app) = app();
    frame(&context, &mut app);
    app.dimensions = [64, 48];
    app.new_document();
    app.command("fill_fg");
    for tool in Tool::ALL {
        app.set_tool(tool);
        frame(&context, &mut app);
    }
    assert!(app.error.is_none());
}

#[test]
fn image_children_collapse_and_effect_layers_attach_by_dragging() {
    let (context, mut app) = app();
    app.dimensions = [32, 24];
    app.new_document();
    app.command("fill_fg");
    let owner = app.session().unwrap().document.active.unwrap();
    for name in ["First mask", "Second mask"] {
        app.command("mask");
        let mask = app.session_mut().unwrap().document.active_mut().unwrap();
        mask.name = name.into();
        assert_eq!(mask.parent, Some(owner));
        assert!(mask.standalone_mask);
    }
    assert_eq!(app.session().unwrap().document.layers.len(), 3);
    let original = render::render(&app.session().unwrap().document);
    let owner_label = layer_label(&context, &mut app, "Layer 1");
    let chevron = frame(&context, &mut app)
        .shapes
        .into_iter()
        .find_map(|shape| match shape.shape {
            egui::Shape::Path(path)
                if path.points.len() == 3
                    && path.stroke.width == 1.5
                    && path.points[1].x < owner_label.x
                    && (owner_label.y..owner_label.y + 36.0).contains(&path.points[1].y) =>
            {
                Some(path.points[1])
            }
            _ => None,
        })
        .expect("Image disclosure chevron");
    for collapsed in [true, false] {
        pointer_frame(
            &context,
            &mut app,
            chevron,
            Some(true),
            egui::Modifiers::NONE,
        );
        pointer_frame(
            &context,
            &mut app,
            chevron,
            Some(false),
            egui::Modifiers::NONE,
        );
        let output = frame(&context, &mut app);
        let has_child = output.shapes.iter().any(|shape| {
            matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.text() == "First mask")
        });
        assert_eq!(has_child, !collapsed);
        assert_eq!(app.session().unwrap().collapsed.contains(&owner), collapsed);
        assert_eq!(render::render(&app.session().unwrap().document), original);
    }

    let filter = Filter::GaussianBlur { radius: 1.0 };
    app.start_filter_layer(filter.clone());
    frame(&context, &mut app);
    let apply = layer_label(&context, &mut app, "Apply") + Vec2::splat(5.0);
    pointer_frame(&context, &mut app, apply, Some(true), egui::Modifiers::NONE);
    pointer_frame(
        &context,
        &mut app,
        apply,
        Some(false),
        egui::Modifiers::NONE,
    );
    assert!(app.dialog.is_none());
    let filter_id = app.session().unwrap().document.active.unwrap();
    assert!(
        app.session()
            .unwrap()
            .document
            .active()
            .unwrap()
            .parent
            .is_none()
    );
    let from = layer_label(&context, &mut app, "Gaussian Blur") + Vec2::splat(5.0);
    let to = layer_label(&context, &mut app, "Layer 1") + Vec2::new(5.0, 18.0);
    drag_pointer(&context, &mut app, from, to, egui::Modifiers::NONE);
    assert_eq!(
        app.session().unwrap().document.active().unwrap().parent,
        Some(owner)
    );
    app.command("undo");
    assert!(
        app.session()
            .unwrap()
            .document
            .layers
            .iter()
            .find(|l| l.id == filter_id)
            .unwrap()
            .parent
            .is_none()
    );
    app.command("redo");
    assert_eq!(
        app.session().unwrap().document.active().unwrap().parent,
        Some(owner)
    );

    app.edit_filter_layer(filter_id);
    let edit = app.effect.as_mut().unwrap();
    edit.filter = Some(Filter::GaussianBlur { radius: 2.0 });
    edit.refresh = true;
    frame(&context, &mut app);
    keyboard_frame(
        &context,
        &mut app,
        vec![text_key(egui::Key::Escape, egui::Modifiers::NONE)],
        egui::Modifiers::NONE,
    );
    assert_eq!(
        app.session().unwrap().document.active().unwrap().filter,
        Some(filter)
    );

    app.start_adjustment(Adjustment::Invert, true);
    frame(&context, &mut app);
    let apply = layer_label(&context, &mut app, "Apply") + Vec2::splat(5.0);
    pointer_frame(&context, &mut app, apply, Some(true), egui::Modifiers::NONE);
    pointer_frame(
        &context,
        &mut app,
        apply,
        Some(false),
        egui::Modifiers::NONE,
    );
    assert!(
        app.session()
            .unwrap()
            .document
            .active()
            .unwrap()
            .parent
            .is_none()
    );
    let from = layer_label(&context, &mut app, "Invert") + Vec2::splat(5.0);
    let to = layer_label(&context, &mut app, "Layer 1") + Vec2::new(5.0, 18.0);
    drag_pointer(&context, &mut app, from, to, egui::Modifiers::NONE);
    assert_eq!(
        app.session().unwrap().document.active().unwrap().parent,
        Some(owner)
    );
    assert_eq!(
        app.session()
            .unwrap()
            .document
            .layers
            .iter()
            .filter(|l| l.parent == Some(owner))
            .count(),
        4
    );

    let from = layer_label(&context, &mut app, "Invert") + Vec2::splat(5.0);
    let to = layer_label(&context, &mut app, "First mask") + Vec2::new(5.0, 34.0);
    drag_pointer(&context, &mut app, from, to, egui::Modifiers::NONE);
    let document = &app.session().unwrap().document;
    let children: Vec<_> = document
        .layers
        .iter()
        .filter(|l| l.parent == Some(owner))
        .map(|l| l.name.as_str())
        .collect();
    assert_eq!(
        children,
        ["Invert", "First mask", "Second mask", "Gaussian Blur"]
    );
    app.command("move_out");
    assert!(
        app.session()
            .unwrap()
            .document
            .active()
            .unwrap()
            .parent
            .is_none()
    );
    app.session().unwrap().document.validate().unwrap();
    assert!(app.error.is_none(), "{:?}", app.error);
}

#[test]
fn standalone_mask_creation_editing_and_history_follow_layer_selection() {
    let (context, mut app) = app();
    app.dimensions = [64, 48];
    app.new_document();
    app.command("fill_fg");
    // An active layer still receives an attached mask.
    app.command("mask");
    assert_eq!(app.session().unwrap().document.layers.len(), 2);
    assert!(
        app.session()
            .unwrap()
            .document
            .active()
            .unwrap()
            .standalone_mask
    );
    assert_eq!(
        app.session().unwrap().document.active().unwrap().parent,
        Some(app.session().unwrap().document.layers[0].id)
    );
    app.command("undo");

    let empty = layer_label(&context, &mut app, "Layer 1") + Vec2::new(5.0, 100.0);
    for pressed in [true, false] {
        pointer_frame(
            &context,
            &mut app,
            empty,
            Some(pressed),
            egui::Modifiers::NONE,
        );
    }
    assert!(app.session().unwrap().document.active.is_none());
    let button = frame(&context, &mut app)
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Circle(circle) if circle.radius == 3.5 && circle.center.x > 1000.0 => {
                Some(circle.center)
            }
            _ => None,
        })
        .expect("Add layer mask button");
    for pressed in [true, false] {
        pointer_frame(
            &context,
            &mut app,
            button,
            Some(pressed),
            egui::Modifiers::NONE,
        );
    }
    let document = &app.session().unwrap().document;
    assert_eq!(document.layers.len(), 2);
    let id = document.active.unwrap();
    assert!(document.active().unwrap().standalone_mask);
    assert!(document.active().unwrap().parent.is_none());
    assert!(document.layers[0].mask.is_none());
    assert!(app.editing_mask());
    app.command("undo");
    assert!(app.session().unwrap().document.active.is_none());
    assert_eq!(app.session().unwrap().document.layers.len(), 1);
    app.command("redo");
    assert_eq!(app.session().unwrap().document.active, Some(id));

    // Clicking the row (rather than its thumbnail) must still edit mask pixels.
    let position = layer_label(&context, &mut app, "Mask");
    pointer_frame(
        &context,
        &mut app,
        position + Vec2::new(5.0, 5.0),
        Some(true),
        egui::Modifiers::NONE,
    );
    pointer_frame(
        &context,
        &mut app,
        position + Vec2::new(5.0, 5.0),
        Some(false),
        egui::Modifiers::NONE,
    );
    assert!(app.editing_mask());
    app.brush.color = [0, 0, 0, 255];
    app.command("fill_fg");
    assert_eq!(
        render::render(&app.session().unwrap().document).get_pixel(32, 24)[3],
        0
    );
    app.command("undo");
    app.set_tool(Tool::Gradient);
    app.background = [255; 4];
    drag(
        &context,
        &mut app,
        Point::new(10.5, 20.5),
        Point::new(50.5, 20.5),
        egui::Modifiers::NONE,
    );
    let layer = app.session().unwrap().document.active().unwrap();
    assert!(layer.pixels.is_none());
    let pixels = &layer.mask.as_ref().unwrap().pixels;
    assert!(pixels.get_pixel(10, 20)[0] <= 1);
    assert!((127..=129).contains(&pixels.get_pixel(30, 20)[0]));
    assert!(pixels.get_pixel(50, 20)[0] >= 254);
    app.set_tool(Tool::Move);
    keyboard_frame(
        &context,
        &mut app,
        vec![text_key(egui::Key::ArrowRight, egui::Modifiers::NONE)],
        egui::Modifiers::NONE,
    );
    assert_eq!(
        app.session()
            .unwrap()
            .document
            .active()
            .unwrap()
            .transform
            .x,
        1.0
    );
    app.command("delete_mask");
    assert_eq!(app.session().unwrap().document.layers.len(), 1);
    app.command("undo");
    assert!(app.editing_mask());
    app.session().unwrap().document.validate().unwrap();
    assert!(app.error.is_none(), "{:?}", app.error);
}

#[test]
fn standalone_mask_creation_uses_selection_and_supports_groups() {
    let (context, mut app) = app();
    app.dimensions = [16, 16];
    app.new_document();
    app.command("fill_fg");
    app.command("group");
    let group = app.session().unwrap().document.active;
    let selection = GrayImage::from_fn(16, 16, |x, _| image::Luma([if x < 8 { 255 } else { 0 }]));
    app.session_mut().unwrap().document.selection = Some(Arc::new(selection.clone()));
    app.command("new_mask_layer");
    let layer = app.session().unwrap().document.active().unwrap();
    assert_eq!(layer.parent, group);
    assert!(layer.standalone_mask);
    assert_eq!(*layer.mask.as_ref().unwrap().pixels, selection);
    let pixels = layer.mask.as_ref().unwrap().pixels.clone();
    app.command("mask");
    assert!(Arc::ptr_eq(
        &pixels,
        &app.session()
            .unwrap()
            .document
            .active()
            .unwrap()
            .mask
            .as_ref()
            .unwrap()
            .pixels
    ));
    app.command("deselect");
    app.command("move_out");
    assert!(
        app.session()
            .unwrap()
            .document
            .active()
            .unwrap()
            .parent
            .is_none()
    );
    assert!(app.editing_mask());
    app.command("duplicate");
    assert!(
        app.session()
            .unwrap()
            .document
            .active()
            .unwrap()
            .standalone_mask
    );
    app.command("clip");
    assert!(
        app.session()
            .unwrap()
            .document
            .active()
            .unwrap()
            .clip_to
            .is_none()
    );
    frame(&context, &mut app);
    app.session().unwrap().document.validate().unwrap();
    assert!(app.error.is_none(), "{:?}", app.error);
}

#[test]
fn layer_commands_and_tabs_have_independent_histories() {
    let (_, mut app) = app();
    app.dimensions = [16, 16];
    app.new_document();
    app.command("fill_fg");
    app.command("duplicate");
    assert_eq!(app.session().unwrap().document.layers.len(), 2);
    app.command("undo");
    assert_eq!(app.session().unwrap().document.layers.len(), 1);
    app.command("redo");
    assert_eq!(app.session().unwrap().document.layers.len(), 2);
    app.new_document();
    assert!(app.session().unwrap().history.undo_name().is_none());
    app.current = 0;
    assert!(app.session().unwrap().history.undo_name().is_some());
    app.command("mask");
    assert!(app.mask_target);
    app.command("delete_mask");
    assert!(!app.mask_target);
    app.session().unwrap().document.validate().unwrap();
}

#[test]
fn levels_reuses_original_histogram_source_until_dialog_closes() {
    for adjustment in [
        Adjustment::LevelsChannels {
            ranges: [xuan::color::DEFAULT_LEVELS; 4],
        },
        Adjustment::Levels {
            black: 0.0,
            gamma: 1.0,
            white: 255.0,
            output_black: 0.0,
            output_white: 255.0,
        },
    ] {
        for as_layer in [false, true] {
            let (context, mut app) = app();
            app.dimensions = [32, 24];
            app.new_document();
            app.brush.color = [180, 140, 100, 255];
            app.command("fill_fg");
            frame(&context, &mut app);
            let original = render::render(&app.session().unwrap().document);
            let expected_source = render::render_scaled(&app.session().unwrap().document, 256, 192);
            app.start_adjustment(adjustment.clone(), as_layer);
            assert!(app.effect.as_ref().unwrap().levels_source.is_none());
            frame(&context, &mut app);
            let source = app.effect.as_ref().unwrap().levels_source.as_ref().unwrap();
            assert_eq!(source, &expected_source);
            let source_pixels = source.as_ptr();

            for (channel, preview) in [(0, true), (1, true), (2, false), (3, true)] {
                let edit = app.effect.as_mut().unwrap();
                match edit.adjustment.as_mut().unwrap() {
                    Adjustment::LevelsChannels { ranges } => ranges[0][1] = 2.0,
                    Adjustment::Levels { gamma, .. } => *gamma = 2.0,
                    _ => unreachable!(),
                }
                edit.channel = channel;
                edit.preview = preview;
                edit.refresh = true;
                frame(&context, &mut app);
                pointer_frame(
                    &context,
                    &mut app,
                    Pos2::new(500.0 + channel as f32, 250.0),
                    None,
                    egui::Modifiers::NONE,
                );
                let source = app.effect.as_ref().unwrap().levels_source.as_ref().unwrap();
                assert_eq!(source.as_ptr(), source_pixels);
                assert_eq!(source, &expected_source);
                assert_eq!(
                    render::render(&app.session().unwrap().document) != original,
                    preview
                );
            }

            let apply = layer_label(&context, &mut app, "Apply") + Vec2::splat(5.0);
            pointer_frame(&context, &mut app, apply, Some(true), egui::Modifiers::NONE);
            pointer_frame(
                &context,
                &mut app,
                apply,
                Some(false),
                egui::Modifiers::NONE,
            );
            assert!(app.dialog.is_none());
            assert!(app.effect.is_none());
            let applied = render::render(&app.session().unwrap().document);
            app.command("undo");
            assert_eq!(render::render(&app.session().unwrap().document), original);
            app.command("redo");
            assert_eq!(render::render(&app.session().unwrap().document), applied);

            let expected_source = render::render_scaled(&app.session().unwrap().document, 256, 192);
            if as_layer {
                let target = app.session().unwrap().document.active.unwrap();
                app.edit_adjustment_layer(target);
            } else {
                app.start_adjustment(adjustment.clone(), false);
            }
            assert!(app.effect.as_ref().unwrap().levels_source.is_none());
            frame(&context, &mut app);
            assert_eq!(
                app.effect.as_ref().unwrap().levels_source.as_ref(),
                Some(&expected_source)
            );
            assert_ne!(
                expected_source.get_pixel(128, 96),
                &image::Rgba([180, 140, 100, 255])
            );
            let edit = app.effect.as_mut().unwrap();
            match edit.adjustment.as_mut().unwrap() {
                Adjustment::LevelsChannels { ranges } => ranges[0][1] = 3.0,
                Adjustment::Levels { gamma, .. } => *gamma = 3.0,
                _ => unreachable!(),
            }
            edit.refresh = true;
            frame(&context, &mut app);
            assert_ne!(render::render(&app.session().unwrap().document), applied);
            keyboard_frame(
                &context,
                &mut app,
                vec![text_key(egui::Key::Escape, egui::Modifiers::NONE)],
                egui::Modifiers::NONE,
            );
            assert!(app.dialog.is_none());
            assert!(app.effect.is_none());
            assert_eq!(render::render(&app.session().unwrap().document), applied);
        }
    }
}

#[test]
fn live_adjustment_cancel_restores_original_and_export_renders() {
    let (context, mut app) = app();
    app.dimensions = [16, 16];
    app.new_document();
    app.brush.color = [180, 140, 100, 255];
    app.command("fill_fg");
    let original = render::render(&app.session().unwrap().document);
    app.start_adjustment(
        Adjustment::Exposure {
            exposure: -2.0,
            offset: 0.0,
            gamma: 1.0,
        },
        false,
    );
    frame(&context, &mut app);
    assert_ne!(render::render(&app.session().unwrap().document), original);
    let session = app.session_mut().unwrap();
    session.history.cancel(&mut session.document);
    assert_eq!(render::render(&app.session().unwrap().document), original);
    app.effect = None;
    app.dialog = Some(Dialog::Export);
    frame(&context, &mut app);
    assert!(app.export_texture.is_some());
}

#[test]
fn background_jobs_commit_once_and_cancel_without_losing_edits() {
    use std::sync::atomic::Ordering;
    let (_, mut app) = app();
    app.dimensions = [8, 8];
    app.new_document();
    app.command("fill_fg");
    let revision = app.session().unwrap().history.revision;
    app.start_job("Worker edit", |document, _| {
        document.layers[0].name = "Worker result".into();
        Ok(())
    });
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while app.job.is_some() {
        app.poll_job();
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(
        app.session().unwrap().document.layers[0].name,
        "Worker result"
    );
    assert_eq!(app.session().unwrap().history.revision, revision + 1);
    app.command("undo");
    assert_eq!(app.session().unwrap().document.layers[0].name, "Layer 1");

    app.start_job("Cancelled edit", |document, cancel| {
        while !cancel.load(Ordering::Relaxed) {
            std::thread::yield_now();
        }
        document.layers.clear();
        Ok(())
    });
    app.job
        .as_ref()
        .unwrap()
        .cancel
        .store(true, Ordering::Relaxed);
    while app.job.is_some() {
        app.poll_job();
        assert!(std::time::Instant::now() < deadline);
        std::thread::yield_now();
    }
    assert_eq!(app.session().unwrap().document.layers.len(), 1);
    assert_eq!(app.session().unwrap().history.revision, revision);
}

#[test]
fn copying_layers_between_projects_keeps_source_and_undoes_in_destination() {
    let (_, mut app) = app();
    app.dimensions = [16, 16];
    app.new_document();
    app.command("fill_fg");
    app.command("group");
    let source = &app.session().unwrap().document;
    let drag = LayerDrag {
        project: source.id,
        layer: source.active.unwrap(),
    };
    let source_pixels = render::render(source);
    app.new_document();
    app.copy_layer_to_project(drag, 1);
    assert_eq!(app.sessions[1].document.layers.len(), 3);
    assert_eq!(render::render(&app.sessions[1].document), source_pixels);
    assert_eq!(render::render(&app.sessions[0].document), source_pixels);
    app.command("undo");
    assert_eq!(app.sessions[1].document.layers.len(), 1);
    assert_eq!(app.sessions[0].document.layers.len(), 2);
}

fn has_command(
    output: &egui::FullOutput,
    predicate: impl Fn(&egui::ViewportCommand) -> bool,
) -> bool {
    output
        .viewport_output
        .values()
        .any(|viewport| viewport.commands.iter().any(&predicate))
}

#[test]
fn idle_window_stops_requesting_repaints() {
    for with_document in [false, true] {
        let (context, mut app) = app();
        if with_document {
            app.dimensions = [64, 48];
            app.new_document();
        }
        // Let layout, font uploads, and opening animations settle.
        for _ in 0..30 {
            frame(&context, &mut app);
        }
        let output = frame(&context, &mut app);
        assert_eq!(
            output.viewport_output[&egui::ViewportId::ROOT].repaint_delay,
            std::time::Duration::MAX,
            "Idle window kept requesting repaints (document: {with_document})"
        );
    }
}

#[test]
fn window_title_updates_on_document_and_dirty_state_changes() {
    let (context, mut app) = app();
    let expect_title = |app: &mut EditorApp, title: &str| {
        let output = frame(&context, app);
        assert!(has_command(&output, |command| matches!(
            command,
            egui::ViewportCommand::Title(value) if value == title
        )));
        let output = frame(&context, app);
        assert!(!has_command(&output, |command| matches!(
            command,
            egui::ViewportCommand::Title(_)
        )));
    };

    expect_title(&mut app, "Xuan");
    app.dimensions = [64, 48];
    app.new_document();
    app.session_mut().unwrap().title = "Photo".into();
    expect_title(&mut app, "Photo —  Xuan");
    app.command("fill_fg");
    expect_title(&mut app, "Photo • —  Xuan");
    app.command("undo");
    expect_title(&mut app, "Photo —  Xuan");
    app.command("redo");
    expect_title(&mut app, "Photo • —  Xuan");
    app.session_mut().unwrap().history.mark_saved();
    expect_title(&mut app, "Photo —  Xuan");

    app.new_document();
    expect_title(&mut app, "Untitled —  Xuan");
    app.current = 0;
    expect_title(&mut app, "Photo —  Xuan");
    app.session_mut().unwrap().title = "Saved photo".into();
    expect_title(&mut app, "Saved photo —  Xuan");
    app.sessions.clear();
    expect_title(&mut app, "Xuan");
}

#[test]
fn menu_bar_hover_switches_only_while_a_menu_is_open() {
    let (context, mut app) = app();
    let file = layer_label(&context, &mut app, "File") + Vec2::splat(5.0);
    let help = layer_label(&context, &mut app, "Help") + Vec2::splat(5.0);
    let expect_menu = |app: &mut EditorApp, expected: Option<&str>| {
        let output = frame(&context, app);
        for label in ["Open Compositor Package…", "About Xuan"] {
            let visible = output.shapes.iter().any(|shape| {
                matches!(&shape.shape, egui::Shape::Text(text) if text.galley.text() == label)
            });
            assert_eq!(visible, expected == Some(label), "Menu item: {label}");
        }
        assert_eq!(egui::Popup::is_any_open(&context), expected.is_some());
    };

    pointer_frame(&context, &mut app, file, None, egui::Modifiers::NONE);
    expect_menu(&mut app, None);

    pointer_frame(&context, &mut app, file, Some(true), egui::Modifiers::NONE);
    pointer_frame(&context, &mut app, file, Some(false), egui::Modifiers::NONE);
    expect_menu(&mut app, Some("Open Compositor Package…"));

    // Switching works in either direction without another click.
    for (position, label) in [(help, "About Xuan"), (file, "Open Compositor Package…")] {
        pointer_frame(&context, &mut app, position, None, egui::Modifiers::NONE);
        expect_menu(&mut app, Some(label));
    }

    // Clicking the active heading closes it and ends hover switching.
    pointer_frame(&context, &mut app, file, Some(true), egui::Modifiers::NONE);
    pointer_frame(&context, &mut app, file, Some(false), egui::Modifiers::NONE);
    expect_menu(&mut app, None);
    pointer_frame(&context, &mut app, help, None, egui::Modifiers::NONE);
    expect_menu(&mut app, None);

    pointer_frame(&context, &mut app, help, Some(true), egui::Modifiers::NONE);
    pointer_frame(&context, &mut app, help, Some(false), egui::Modifiers::NONE);
    expect_menu(&mut app, Some("About Xuan"));
    keyboard_frame(
        &context,
        &mut app,
        vec![text_key(egui::Key::Escape, egui::Modifiers::NONE)],
        egui::Modifiers::NONE,
    );
    pointer_frame(&context, &mut app, file, None, egui::Modifiers::NONE);
    expect_menu(&mut app, None);

    // An unrelated popup must not activate the menu bar's hover behavior.
    let unrelated = egui::Id::new("unrelated_popup");
    egui::Popup::open_id(&context, unrelated);
    pointer_frame(&context, &mut app, help, None, egui::Modifiers::NONE);
    assert!(egui::Popup::is_id_open(&context, unrelated));
    egui::Popup::close_all(&context);
}

#[test]
fn client_titlebar_moves_resizes_and_preserves_unsaved_close_flow() {
    let (context, mut app) = app();
    frame(&context, &mut app);
    frame(&context, &mut app);
    let output = pointer_frame(
        &context,
        &mut app,
        Pos2::new(950.0, 20.0),
        Some(true),
        egui::Modifiers::NONE,
    );
    assert!(!has_command(&output, |c| matches!(
        c,
        egui::ViewportCommand::StartDrag
    )));
    let output = pointer_frame(
        &context,
        &mut app,
        Pos2::new(970.0, 25.0),
        None,
        egui::Modifiers::NONE,
    );
    assert!(has_command(&output, |c| matches!(
        c,
        egui::ViewportCommand::StartDrag
    )));
    pointer_frame(
        &context,
        &mut app,
        Pos2::new(970.0, 25.0),
        Some(false),
        egui::Modifiers::NONE,
    );

    // Resize from the undecorated left edge.
    pointer_frame(
        &context,
        &mut app,
        Pos2::new(1.0, 400.0),
        None,
        egui::Modifiers::NONE,
    );
    let output = pointer_frame(
        &context,
        &mut app,
        Pos2::new(1.0, 400.0),
        Some(true),
        egui::Modifiers::NONE,
    );
    assert!(has_command(&output, |c| matches!(
        c,
        egui::ViewportCommand::BeginResize(egui::ResizeDirection::West)
    )));
    pointer_frame(
        &context,
        &mut app,
        Pos2::new(1.0, 400.0),
        Some(false),
        egui::Modifiers::NONE,
    );

    for (x, maximize) in [(61.0, true), (41.0, false)] {
        pointer_frame(
            &context,
            &mut app,
            Pos2::new(x, 20.0),
            None,
            egui::Modifiers::NONE,
        );
        pointer_frame(
            &context,
            &mut app,
            Pos2::new(x, 20.0),
            Some(true),
            egui::Modifiers::NONE,
        );
        let output = pointer_frame(
            &context,
            &mut app,
            Pos2::new(x, 20.0),
            Some(false),
            egui::Modifiers::NONE,
        );
        assert!(has_command(&output, |c| if maximize {
            matches!(c, egui::ViewportCommand::Maximized(true))
        } else {
            matches!(c, egui::ViewportCommand::Minimized(true))
        }));
    }

    app.dimensions = [16, 16];
    app.new_document();
    app.command("fill_fg");
    frame(&context, &mut app);
    pointer_frame(
        &context,
        &mut app,
        Pos2::new(21.0, 20.0),
        Some(true),
        egui::Modifiers::NONE,
    );
    let output = pointer_frame(
        &context,
        &mut app,
        Pos2::new(21.0, 20.0),
        Some(false),
        egui::Modifiers::NONE,
    );
    assert!(app.close_app);
    assert!(!has_command(&output, |c| matches!(
        c,
        egui::ViewportCommand::Close
    )));
    assert_eq!(app.sessions.len(), 1);
}

#[test]
fn native_window_gestures_work_without_a_mouse_release_event() {
    let (context, mut app) = app();
    frame(&context, &mut app);
    frame(&context, &mut app);

    // Wayland can consume the release while the compositor moves or resizes
    // the window. Each new gesture must work without an intervening click.
    for resize in [false, false, true, true, false] {
        let start = if resize {
            Pos2::new(1.0, 400.0)
        } else {
            Pos2::new(950.0, 20.0)
        };
        pointer_frame(&context, &mut app, start, None, egui::Modifiers::NONE);
        let mut output =
            pointer_frame(&context, &mut app, start, Some(true), egui::Modifiers::NONE);
        if !resize {
            output = pointer_frame(
                &context,
                &mut app,
                start + egui::vec2(20.0, 5.0),
                None,
                egui::Modifiers::NONE,
            );
        }
        assert!(has_command(&output, |command| if resize {
            matches!(
                command,
                egui::ViewportCommand::BeginResize(egui::ResizeDirection::West)
            )
        } else {
            matches!(command, egui::ViewportCommand::StartDrag)
        }));

        // The pointer leaves and returns, but no button-up reaches the app.
        keyboard_frame(
            &context,
            &mut app,
            vec![egui::Event::PointerGone],
            egui::Modifiers::NONE,
        );
        frame(&context, &mut app);
    }

    // A normal control must also respond to the first click after a drag.
    let minimize = Pos2::new(41.0, 20.0);
    pointer_frame(&context, &mut app, minimize, None, egui::Modifiers::NONE);
    pointer_frame(
        &context,
        &mut app,
        minimize,
        Some(true),
        egui::Modifiers::NONE,
    );
    let output = pointer_frame(
        &context,
        &mut app,
        minimize,
        Some(false),
        egui::Modifiers::NONE,
    );
    assert!(has_command(&output, |command| matches!(
        command,
        egui::ViewportCommand::Minimized(true)
    )));
}

#[test]
fn titlebar_double_click_toggles_maximize_without_starting_a_drag() {
    for maximized in [false, true] {
        for x in [640.0, 950.0] {
            let (context, mut app) = app();
            frame(&context, &mut app);
            frame(&context, &mut app);
            for (index, pressed) in [true, false, true, false].into_iter().enumerate() {
                let pos = Pos2::new(x, 20.0);
                let output = context.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            Pos2::ZERO,
                            Vec2::new(1280.0, 860.0),
                        )),
                        viewports: [(
                            egui::ViewportId::ROOT,
                            egui::ViewportInfo {
                                maximized: Some(maximized),
                                ..Default::default()
                            },
                        )]
                        .into_iter()
                        .collect(),
                        events: vec![
                            egui::Event::PointerMoved(pos),
                            egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                        time: Some(1.0 + index as f64 * 0.05),
                        ..Default::default()
                    },
                    |ctx| app.show(ctx),
                );
                assert!(!has_command(&output, |c| matches!(
                    c,
                    egui::ViewportCommand::StartDrag
                )));
                let toggles: Vec<_> = output
                    .viewport_output
                    .values()
                    .flat_map(|viewport| &viewport.commands)
                    .filter_map(|command| match command {
                        egui::ViewportCommand::Maximized(value) => Some(*value),
                        _ => None,
                    })
                    .collect();
                if index == 3 {
                    assert_eq!(toggles, vec![!maximized]);
                } else {
                    assert!(toggles.is_empty());
                }
            }
        }
    }
}

#[test]
fn custom_controls_keep_keyboard_input_and_disabled_behavior() {
    let context = egui::Context::default();
    theme::apply(&context);
    let mut value = 0.5_f32;
    let mut enabled = true;
    let mut slider_id = egui::Id::NULL;
    let mut draw = |events: Vec<egui::Event>, enabled: bool| {
        let _ = context.run(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.add_enabled_ui(enabled, |ui| {
                        let response =
                            ui.add(widgets::Slider::new(&mut value, 0.0..=1.0).percentage());
                        slider_id = response.id;
                        response.request_focus();
                    });
                });
            },
        );
        value
    };
    draw(Vec::new(), enabled);
    let key = egui::Event::Key {
        key: egui::Key::ArrowRight,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    };
    let changed = draw(vec![key.clone()], enabled);
    assert!(changed > 0.5 && changed <= 1.0);
    enabled = false;
    let unchanged = draw(vec![key], enabled);
    assert_eq!(unchanged, changed);
}

fn wheel_events(pos: Pos2, delta: Vec2, modifiers: egui::Modifiers) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(pos),
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta,
            modifiers,
        },
    ]
}

fn wheel_control_frame(
    context: &egui::Context,
    events: Vec<egui::Event>,
    mut control: impl FnMut(&mut egui::Ui) -> egui::Response,
) -> (egui::Response, Vec2) {
    let mut result = None;
    let _ = context.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                Vec2::new(400.0, 160.0),
            )),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let scroll = egui::ScrollArea::both().show(ui, |ui| {
                    let response = control(ui);
                    ui.allocate_space(Vec2::splat(800.0));
                    response
                });
                result = Some((scroll.inner, scroll.state.offset));
            });
        },
    );
    result.unwrap()
}

#[test]
fn number_wheel_changes_clamps_and_consumes_panel_scrolling() {
    let context = egui::Context::default();
    let mut value = 10_u32;
    let mut draw = |events, enabled| {
        let (response, offset) = wheel_control_frame(&context, events, |ui| {
            ui.add_enabled(enabled, widgets::Number::new(&mut value).range(0..=20))
        });
        (value, response, offset)
    };
    let (_, response, _) = draw(Vec::new(), true);
    let pos = response.rect.center();
    draw(vec![egui::Event::PointerMoved(pos)], true);
    for (delta, expected, changed) in [
        (1.0, 11, true),
        (-2.0, 9, true),
        (100.0, 20, true),
        (1.0, 20, false),
        (-100.0, 0, true),
        (-1.0, 0, false),
    ] {
        let (value, response, offset) = draw(
            wheel_events(pos, Vec2::new(0.0, delta), egui::Modifiers::NONE),
            true,
        );
        assert_eq!(value, expected);
        assert_eq!(response.changed(), changed);
        assert_eq!(offset, Vec2::ZERO);
    }
    // The smoothing tail must neither edit again nor scroll the containing panel.
    for _ in 0..30 {
        let (value, response, offset) = draw(Vec::new(), true);
        assert_eq!(value, 0);
        assert!(!response.changed());
        assert_eq!(offset, Vec2::ZERO);
    }
    let (value, response, _) = draw(
        wheel_events(pos, Vec2::new(0.0, 1.0), egui::Modifiers::NONE),
        false,
    );
    assert_eq!(value, 0);
    assert!(!response.changed());
}

#[test]
fn slider_wheel_matches_number_steps_and_preserves_horizontal_scrolling() {
    for (range, percentage, logarithmic, initial, step) in [
        (0.0..=1.0, true, false, 0.5_f64, 0.01),
        (-5.0..=5.0, false, false, 0.0, 0.01),
        (0.1..=100.0, false, true, 10.0, 1.0),
    ] {
        let context = egui::Context::default();
        let mut value = initial;
        let mut draw = |events| {
            let (response, offset) = wheel_control_frame(&context, events, |ui| {
                let slider =
                    widgets::Slider::new(&mut value, range.clone()).logarithmic(logarithmic);
                ui.add(if percentage {
                    slider.percentage()
                } else {
                    slider
                })
            });
            (value, response, offset)
        };
        let (_, response, _) = draw(Vec::new());
        let rail = egui::pos2(response.rect.left() + 10.0, response.rect.center().y);
        let field = egui::pos2(response.rect.right() - 10.0, response.rect.center().y);
        draw(vec![egui::Event::PointerMoved(rail)]);
        let (value, response, offset) = draw(wheel_events(
            rail,
            Vec2::new(0.0, 1.0),
            egui::Modifiers::NONE,
        ));
        assert!((value - initial - step).abs() < 1e-6);
        assert!(response.changed());
        assert_eq!(offset, Vec2::ZERO);
        draw(vec![egui::Event::PointerMoved(field)]);
        let (value, response, offset) = draw(wheel_events(
            field,
            Vec2::new(0.0, -1.0),
            egui::Modifiers::NONE,
        ));
        assert!((value - initial).abs() < 1e-6);
        assert!(response.changed());
        assert_eq!(offset, Vec2::ZERO);
        let (value, response, offset) = draw(wheel_events(
            field,
            Vec2::new(-1.0, 0.0),
            egui::Modifiers::NONE,
        ));
        assert!((value - initial).abs() < 1e-6);
        assert!(!response.changed());
        assert!(offset.x > 0.0);
    }
}

#[test]
fn number_wheel_accumulates_small_deltas_and_keeps_focused_text_current() {
    let context = egui::Context::default();
    let mut value = 1.0_f64;
    let mut draw = |events| {
        let (response, _) = wheel_control_frame(&context, events, |ui| {
            ui.add(widgets::Number::new(&mut value).speed(0.01).max_decimals(2))
        });
        (value, response)
    };
    let (_, response) = draw(Vec::new());
    response.request_focus();
    let pos = response.rect.center();
    draw(vec![egui::Event::PointerMoved(pos)]);
    let line_height = context.options(|options| options.input_options.line_scroll_speed);
    for index in 0..10 {
        let (value, response) = draw(vec![
            egui::Event::PointerMoved(pos),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: Vec2::new(0.0, line_height / 10.0),
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        assert_eq!(value, if index == 9 { 1.01 } else { 1.0 });
        assert_eq!(response.changed(), index == 9);
    }
    let (value, response) = draw(Vec::new());
    assert_eq!(value, 1.01);
    assert_eq!(
        context
            .data(|data| data.get_temp::<String>(response.id))
            .as_deref(),
        Some("1.01"),
    );
    let (value, response) = draw(wheel_events(
        pos + Vec2::new(100.0, 0.0),
        Vec2::new(0.0, 1.0),
        egui::Modifiers::NONE,
    ));
    assert_eq!(value, 1.01);
    assert!(!response.changed());
}

#[test]
fn canvas_wheel_pans_horizontally_and_keeps_vertical_zoom_anchored() {
    for (delta, modifiers) in [
        (Vec2::new(-1.0, 0.0), egui::Modifiers::NONE),
        (Vec2::new(0.0, -1.0), egui::Modifiers::SHIFT),
        (Vec2::new(0.0, 1.0), egui::Modifiers::NONE),
    ] {
        let (context, mut app) = app();
        app.dimensions = [32, 24];
        app.new_document();
        frame(&context, &mut app);
        let pos = app.canvas_rect.unwrap().center() + Vec2::new(30.0, 20.0);
        pointer_frame(&context, &mut app, pos, None, modifiers);
        let before = app.session().unwrap();
        let zoom = before.zoom;
        let pan = before.pan;
        let point = (pos - app.canvas_rect.unwrap().min) / zoom;
        let _ = context.run(
            egui::RawInput {
                events: wheel_events(pos, delta, modifiers),
                modifiers,
                ..Default::default()
            },
            |ctx| app.show(ctx),
        );
        frame(&context, &mut app);
        let after = app.session().unwrap();
        if modifiers.shift || delta.x != 0.0 {
            assert!(after.pan.x < pan.x);
            assert_eq!(after.pan.y, pan.y);
            assert_eq!(after.zoom, zoom);
        } else {
            assert!(after.zoom > zoom);
            // Rendering uses the previous frame's view; settle the smoothing first.
            for _ in 0..30 {
                frame(&context, &mut app);
            }
            let after_point = (pos - app.canvas_rect.unwrap().min) / app.session().unwrap().zoom;
            assert!((after_point - point).length() < 0.001);
        }
    }
}

#[test]
fn floating_panels_stay_bounded_at_minimum_window_size() {
    let (context, mut app) = app();
    app.dimensions = [32, 24];
    app.new_document();
    app.command("fill_fg");
    app.command("levels");
    for _ in 0..5 {
        frame(&context, &mut app);
    }
    let full = context
        .memory(|memory| memory.area_rect(egui::Id::new("Levels")))
        .unwrap();
    assert!(
        full.height() > 530.0,
        "Levels should expand before scrolling: {full:?}"
    );
    for panel in ["levels", "hue", "curves", "export", "new", "text"] {
        app.dialog = None;
        app.effect = None;
        app.export_format = "jpg".into();
        if panel == "text" {
            app.start_text(None, Point::default());
            app.text_edit.as_mut().unwrap().style.content = "A long text document\n".repeat(60);
        } else {
            app.command(panel);
        }
        for _ in 0..5 {
            let _ = context.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        Pos2::ZERO,
                        Vec2::new(850.0, 560.0),
                    )),
                    ..Default::default()
                },
                |ctx| app.show(ctx),
            );
        }
        let title = match panel {
            "levels" => "Levels",
            "hue" => "Hue/Saturation",
            "curves" => "Curves",
            "export" => "Export image",
            "text" => "Text",
            _ => "New canvas",
        };
        let rect = context
            .memory(|memory| memory.area_rect(egui::Id::new(title)))
            .expect(title);
        assert!(rect.width() < 740.0, "{title} grew to {rect:?}");
        assert!(rect.height() <= 542.0, "{title} is too tall: {rect:?}");
        assert!(
            rect.top() >= 0.0 && rect.bottom() <= 560.0,
            "{title} clipped vertically: {rect:?}"
        );
        assert!(
            rect.left() >= 0.0 && rect.right() <= 850.0,
            "{title} clipped: {rect:?}"
        );
    }
}

#[test]
fn floating_panel_title_remains_draggable() {
    let (context, mut app) = app();
    app.dimensions = [16, 16];
    app.new_document();
    app.command("hue");
    for _ in 0..3 {
        frame(&context, &mut app);
    }
    let id = egui::Id::new("Hue/Saturation");
    let before = context.memory(|memory| memory.area_rect(id)).unwrap();
    let start = before.center_top() + Vec2::new(0.0, 15.0);
    let end = start + Vec2::new(50.0, 30.0);
    pointer_frame(&context, &mut app, start, None, egui::Modifiers::NONE);
    pointer_frame(&context, &mut app, start, Some(true), egui::Modifiers::NONE);
    pointer_frame(&context, &mut app, end, None, egui::Modifiers::NONE);
    pointer_frame(&context, &mut app, end, Some(false), egui::Modifiers::NONE);
    let after = context.memory(|memory| memory.area_rect(id)).unwrap();
    assert!(
        (after.min - before.min).length() > 20.0,
        "Panel didn't move: {before:?} → {after:?}"
    );
}

#[test]
fn double_click_raw_layer_opens_develop_and_rasterization_is_undoable() {
    let (context, mut app) = app();
    app.dimensions = [80, 60];
    app.new_document();
    let doc = &mut app.session_mut().unwrap().document;
    let layer = doc.active_mut().unwrap();
    layer.name = "Camera RAW".into();
    layer.pixels = Some(Arc::new(RgbaImage::from_pixel(
        80,
        60,
        image::Rgba([100, 90, 80, 255]),
    )));
    layer.raw = Some(xuan::raw::RawAsset {
        filename: "camera.NEF".into(),
        bytes: Arc::new(vec![1, 2, 3]),
        metadata: xuan::raw::RawMetadata {
            width: 80,
            height: 60,
            ..Default::default()
        },
        settings: xuan::raw::DevelopSettings::default(),
    });
    double_click_layer_name(&context, &mut app, "Camera RAW");
    keyboard_frame(
        &context,
        &mut app,
        vec![text_key(egui::Key::Escape, egui::Modifiers::NONE)],
        egui::Modifiers::NONE,
    );
    app.frames += 40;
    let pos = layer_label(&context, &mut app, "Camera RAW") + Vec2::new(5.0, 25.0);
    for _ in 0..2 {
        pointer_frame(&context, &mut app, pos, Some(true), egui::Modifiers::NONE);
        pointer_frame(&context, &mut app, pos, Some(false), egui::Modifiers::NONE);
    }
    assert!(app.develop.is_some());
    assert!(app.rename.is_none());
    app.cancel_develop();
    // Let the previous double-click expire before testing a second target.
    for _ in 0..40 {
        frame(&context, &mut app);
    }
    let output = frame(&context, &mut app);
    let session = app.session().unwrap();
    let thumbnail_id = session.thumbnails[&(session.document.active.unwrap(), false)].id();
    let thumbnail = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Mesh(mesh) if mesh.texture_id == thumbnail_id => {
                Some(mesh.calc_bounds().center())
            }
            _ => None,
        })
        .expect("RAW thumbnail is visible");
    pointer_frame(&context, &mut app, thumbnail, None, egui::Modifiers::NONE);
    for _ in 0..2 {
        pointer_frame(
            &context,
            &mut app,
            thumbnail,
            Some(true),
            egui::Modifiers::NONE,
        );
        pointer_frame(
            &context,
            &mut app,
            thumbnail,
            Some(false),
            egui::Modifiers::NONE,
        );
    }
    assert!(app.develop.is_some());
    app.cancel_develop();
    app.command("rasterize_raw");
    assert!(
        app.session()
            .unwrap()
            .document
            .active()
            .unwrap()
            .raw
            .is_none()
    );
    app.command("undo");
    assert!(
        app.session()
            .unwrap()
            .document
            .active()
            .unwrap()
            .raw
            .is_some()
    );
}
