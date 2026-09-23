use super::*;
use crate::app::tablet::{Phase, Sample};

fn smoothing_app(strength: f32) -> (egui::Context, EditorApp) {
    let (context, mut app) = app();
    app.dimensions = [160, 160];
    app.new_document();
    app.set_tool(Tool::Brush);
    app.brush.diameter = 2.0;
    app.brush.hardness = 1.0;
    app.brush_smoothing = strength;
    let session = app.session_mut().unwrap();
    session.zoom = 1.0;
    session.fit = false;
    frame(&context, &mut app);
    (context, app)
}

fn screen(app: &EditorApp, x: f32, y: f32) -> Pos2 {
    app.canvas_rect.unwrap().min + Vec2::new(x, y) * app.session().unwrap().zoom
}

#[test]
fn low_opacity_fast_mouse_and_pen_strokes_stay_even_and_undo() {
    for smoothing in [0.0, 1.0] {
        for pen in [false, true] {
            let (context, mut app) = smoothing_app(smoothing);
            app.brush.diameter = 53.0;
            app.brush.hardness = 0.83;
            app.brush.opacity = 0.3;
            app.pressure_size = false;
            app.pressure_opacity = true;
            let draw = |app: &mut EditorApp| {
                for (x, phase) in [
                    (30.0, Phase::Down),
                    (48.0, Phase::Move),
                    (110.0, Phase::Move),
                    (130.0, Phase::Up),
                ] {
                    let position = screen(app, x, 64.0);
                    if pen {
                        app.pen_samples = vec![Sample {
                            position,
                            pressure: Some(if phase == Phase::Up { 0.0 } else { 1.0 }),
                            tilt: None,
                            eraser: false,
                            phase,
                        }];
                        frame(&context, app);
                    } else {
                        pointer_frame(
                            &context,
                            app,
                            position,
                            match phase {
                                Phase::Down => Some(true),
                                Phase::Up => Some(false),
                                _ => None,
                            },
                            egui::Modifiers::NONE,
                        );
                    }
                    // Idle frames must not add more paint at a stationary pointer.
                    frame(&context, app);
                }
                assert!(app.error.is_none(), "{:?}", app.error);
                assert!(app.gesture.is_none());
            };
            draw(&mut app);
            let first = render::render(&app.session().unwrap().document);
            for x in 30..=130 {
                assert_eq!(
                    first.get_pixel(x, 64)[3],
                    77,
                    "x={x}, pen={pen}, smoothing={smoothing}"
                );
            }
            draw(&mut app);
            let second = render::render(&app.session().unwrap().document);
            for x in 30..=130 {
                assert_eq!(second.get_pixel(x, 64)[3], 130);
            }
            assert_eq!(app.session().unwrap().history.names().count(), 2);
            app.command("undo");
            assert_eq!(render::render(&app.session().unwrap().document), first);
            app.command("redo");
            assert_eq!(render::render(&app.session().unwrap().document), second);
        }
    }
}

#[test]
fn smoothed_mouse_stroke_reduces_wobble_finishes_at_release_and_undoes() {
    for strength in [0.0, 1.0] {
        let (context, mut app) = smoothing_app(strength);
        for (x, y, pressed) in [
            (20.0, 64.0, Some(true)),
            (80.0, 64.0, None),
            (81.0, 72.0, None),
            (82.0, 56.0, None),
        ] {
            let position = screen(&app, x, y);
            pointer_frame(&context, &mut app, position, pressed, egui::Modifiers::NONE);
        }
        let tip = app.gesture.as_ref().unwrap().last;
        if strength == 0.0 {
            assert_eq!(tip, Point::new(82.0, 56.0));
        } else {
            assert!((tip.y - 64.0).abs() < 3.0, "{tip:?}");
            frame(&context, &mut app);
            assert_eq!(app.gesture.as_ref().unwrap().last, tip);
        }
        // The final position arrives only with the release event.
        let position = screen(&app, 120.0, 64.0);
        pointer_frame(
            &context,
            &mut app,
            position,
            Some(false),
            egui::Modifiers::NONE,
        );
        assert!(app.gesture.is_none());
        assert!(app.error.is_none(), "{:?}", app.error);
        let session = app.session().unwrap();
        assert_eq!(session.history.names().count(), 1);
        let painted = render::render(&session.document);
        if strength > 0.0 {
            assert_eq!(app.last_brush, Some(Point::new(120.0, 64.0)));
            assert!(painted.get_pixel(120, 64)[3] > 0);
        }
        app.command("undo");
        assert!(
            render::render(&app.session().unwrap().document)
                .pixels()
                .all(|p| p[3] == 0)
        );
        app.command("redo");
        assert_eq!(render::render(&app.session().unwrap().document), painted);
    }
}

#[test]
fn smoothed_mouse_stroke_retains_batched_moves_and_does_not_replay_release() {
    let draw = |batch| {
        let (context, mut app) = smoothing_app(1.0);
        let start = screen(&app, 20.0, 20.0);
        if batch < 2 {
            pointer_frame(&context, &mut app, start, Some(true), egui::Modifiers::NONE);
        }
        let positions =
            [(60.0, 110.0), (110.0, 20.0), (130.0, 60.0)].map(|(x, y)| screen(&app, x, y));
        if batch > 0 {
            let mut events = Vec::new();
            if batch == 2 {
                events.push(egui::Event::PointerButton {
                    pos: start,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            events.extend(positions.into_iter().map(egui::Event::PointerMoved));
            events.push(egui::Event::PointerButton {
                pos: positions[2],
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            });
            keyboard_frame(&context, &mut app, events, egui::Modifiers::NONE);
        } else {
            for position in positions {
                pointer_frame(&context, &mut app, position, None, egui::Modifiers::NONE);
            }
            pointer_frame(
                &context,
                &mut app,
                positions[2],
                Some(false),
                egui::Modifiers::NONE,
            );
        }
        assert!(app.error.is_none(), "{:?}", app.error);
        assert_eq!(app.session().unwrap().history.names().count(), 1);
        render::render(&app.session().unwrap().document)
    };
    let separate_frames = draw(0);
    assert_eq!(draw(1), separate_frames);
    assert_eq!(draw(2), separate_frames);
}

#[test]
fn smoothed_taps_shift_lines_and_cancellation_do_not_leak_between_strokes() {
    let (context, mut app) = smoothing_app(1.0);
    click_canvas(
        &context,
        &mut app,
        Point::new(20.0, 20.0),
        egui::Modifiers::NONE,
    );
    click_canvas(
        &context,
        &mut app,
        Point::new(120.0, 20.0),
        egui::Modifiers::SHIFT,
    );
    let pixels = render::render(&app.session().unwrap().document);
    for x in 20..=120 {
        assert!(pixels.get_pixel(x, 20)[3] > 0);
    }
    let original = pixels;
    for (x, pressed) in [(20.0, Some(true)), (100.0, None)] {
        let position = screen(&app, x, 60.0);
        pointer_frame(&context, &mut app, position, pressed, egui::Modifiers::NONE);
    }
    assert!(app.gesture.as_ref().unwrap().smoothing.is_some());
    app.cancel_gesture();
    let position = screen(&app, 100.0, 60.0);
    pointer_frame(
        &context,
        &mut app,
        position,
        Some(false),
        egui::Modifiers::NONE,
    );
    assert_eq!(render::render(&app.session().unwrap().document), original);
    click_canvas(
        &context,
        &mut app,
        Point::new(40.0, 120.0),
        egui::Modifiers::NONE,
    );
    let pixels = render::render(&app.session().unwrap().document);
    assert!(pixels.get_pixel(40, 120)[3] > 0);
    assert_eq!(pixels.get_pixel(70, 90)[3], 0);
    assert_eq!(app.session().unwrap().history.names().count(), 3);
}

#[test]
fn smoothed_tablet_release_preserves_contact_pressure_tilt_and_eraser() {
    for end_phase in [Phase::Up, Phase::Leave] {
        let (context, mut app) = smoothing_app(1.0);
        app.brush.diameter = 20.0;
        app.pressure_opacity = true;
        app.tilt_shape = true;
        let sample = |app: &EditorApp, x, phase, pressure, eraser| Sample {
            position: screen(app, x, 64.0),
            pressure: Some(pressure),
            tilt: Some([60.0, 0.0]),
            eraser,
            phase,
        };
        app.pen_samples = vec![
            sample(&app, 20.0, Phase::Down, 0.2, false),
            sample(&app, 80.0, Phase::Move, 0.5, false),
        ];
        frame(&context, &mut app);
        let gesture = app.gesture.as_ref().unwrap();
        assert!(gesture.last.x < 80.0);
        assert_eq!(gesture.brush.diameter, 10.0);
        assert_eq!(gesture.brush.opacity, 0.5);
        assert_eq!(gesture.brush.tilt, [60.0, 0.0]);
        let end = if end_phase == Phase::Up { 110.0 } else { 80.0 };
        app.pen_samples = vec![sample(&app, end, end_phase, 0.0, false)];
        frame(&context, &mut app);
        assert!(!app.pen_stroke);
        assert_eq!(app.last_brush, Some(Point::new(end, 64.0)));
        let pixels = render::render(&app.session().unwrap().document);
        assert!(pixels.get_pixel(end as u32, 64)[3] > 0);
        assert!(pixels.get_pixel(end as u32 + 3, 64)[3] > 0);
        assert_eq!(pixels.get_pixel(end as u32, 68)[3], 0);
        assert_eq!(app.session().unwrap().history.names().count(), 1);

        app.pen_samples = vec![
            sample(&app, 20.0, Phase::Down, 1.0, true),
            sample(&app, end, Phase::Move, 1.0, true),
            sample(&app, end, Phase::Up, 0.0, true),
        ];
        frame(&context, &mut app);
        assert_eq!(app.tool, Tool::Brush);
        assert_eq!(
            render::render(&app.session().unwrap().document).get_pixel(end as u32, 64)[3],
            0
        );
        app.command("undo");
        assert_eq!(render::render(&app.session().unwrap().document), pixels);
        app.command("undo");
        assert!(
            render::render(&app.session().unwrap().document)
                .pixels()
                .all(|p| p[3] == 0)
        );
    }
}

#[test]
fn smoothed_mask_strokes_respect_selection_and_undo() {
    let (context, mut app) = smoothing_app(1.0);
    let session = app.session_mut().unwrap();
    session.document.active_mut().unwrap().mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(160, 160, image::Luma([255]))),
        ..Mask::white()
    });
    session.document.selection = Some(Arc::new(xuan::selection::rectangle(
        160,
        160,
        Point::new(40.0, 0.0),
        Point::new(100.0, 160.0),
        false,
    )));
    app.mask_target = true;
    drag(
        &context,
        &mut app,
        Point::new(20.0, 64.0),
        Point::new(120.0, 64.0),
        egui::Modifiers::NONE,
    );
    let session = app.session().unwrap();
    let mask = &session
        .document
        .active()
        .unwrap()
        .mask
        .as_ref()
        .unwrap()
        .pixels;
    assert_eq!(mask.get_pixel(20, 64)[0], 255);
    assert_eq!(mask.get_pixel(60, 64)[0], 0);
    assert_eq!(mask.get_pixel(120, 64)[0], 255);
    assert_eq!(session.history.names().count(), 1);
    app.command("undo");
    assert!(
        app.session()
            .unwrap()
            .document
            .active()
            .unwrap()
            .mask
            .as_ref()
            .unwrap()
            .pixels
            .pixels()
            .all(|p| p[0] == 255)
    );
}
