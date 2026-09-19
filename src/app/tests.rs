use super::*;

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
    for panel in ["levels", "hue", "curves", "export", "new"] {
        app.dialog = None;
        app.effect = None;
        app.export_format = "jpg".into();
        app.command(panel);
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
