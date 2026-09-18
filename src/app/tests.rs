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
) {
    let mut events = vec![egui::Event::PointerMoved(pos)];
    if let Some(pressed) = pressed {
        events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers,
        });
    }
    let _ = context.run(
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
    );
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
