use super::*;

fn app() -> (egui::Context, EditorApp) {
    let context = egui::Context::default();
    let app = EditorApp::with_context(&context, Vec::new(), false, None);
    (context, app)
}

fn frame(context: &egui::Context, app: &mut EditorApp) {
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
