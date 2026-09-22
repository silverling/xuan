use super::*;
use egui::{Modifiers, RawInput, pos2};

fn pen(x: f32, down: bool) -> PenFrame {
    PenFrame {
        tool: 1,
        position: Some(pos2(x, 100.0)),
        proximity: true,
        buttons: [down, false, false],
        pressure: None,
        tilt: None,
        eraser: false,
        mouse_handoff: false,
    }
}

fn button(pos: Pos2, button: PointerButton, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

#[test]
fn hover_tip_contact_and_lift_generate_pointer_input_without_a_mouse() {
    let mut pointer = PenPointer::default();
    let mut input = RawInput::default();
    pointer.update(&mut input, &[pen(20.0, false)], 1.0, false);
    assert_eq!(input.events, [Event::PointerMoved(pos2(20.0, 100.0))]);
    input.events.clear();
    pointer.update(
        &mut input,
        &[pen(20.0, true), pen(50.0, true), pen(80.0, false)],
        1.0,
        false,
    );
    assert_eq!(
        input.events,
        [
            Event::PointerMoved(pos2(20.0, 100.0)),
            button(pos2(20.0, 100.0), PointerButton::Primary, true),
            Event::PointerMoved(pos2(50.0, 100.0)),
            Event::PointerMoved(pos2(80.0, 100.0)),
            button(pos2(80.0, 100.0), PointerButton::Primary, false),
        ]
    );
}

#[test]
fn leaving_or_disconnecting_releases_all_buttons_before_pointer_gone() {
    for disconnected in [false, true] {
        let mut pointer = PenPointer::default();
        let mut input = RawInput::default();
        let mut frame = pen(20.0, true);
        frame.buttons = [true; 3];
        pointer.update(&mut input, &[frame], 1.0, false);
        input.events.clear();
        frame.proximity = false;
        pointer.update(&mut input, &[frame], 1.0, disconnected);
        assert_eq!(
            input.events,
            [
                button(pos2(20.0, 100.0), PointerButton::Primary, false),
                button(pos2(20.0, 100.0), PointerButton::Secondary, false),
                button(pos2(20.0, 100.0), PointerButton::Middle, false),
                Event::PointerGone,
            ]
        );
        assert!(pointer.tool.is_none());
    }
}

#[test]
fn surface_coordinates_use_ui_zoom_and_preserve_modifiers() {
    let mut pointer = PenPointer::default();
    let mut input = RawInput {
        modifiers: Modifiers::ALT,
        ..Default::default()
    };
    pointer.update(&mut input, &[pen(40.0, true)], 2.0, false);
    assert_eq!(
        input.events,
        [
            Event::PointerMoved(pos2(20.0, 50.0)),
            Event::PointerButton {
                pos: pos2(20.0, 50.0),
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::ALT,
            },
        ]
    );
}

#[test]
fn mouse_input_is_unchanged_without_a_pen_and_resumes_after_proximity_out() {
    let mut pointer = PenPointer::default();
    let mouse = vec![
        Event::PointerMoved(pos2(4.0, 5.0)),
        button(pos2(4.0, 5.0), PointerButton::Primary, true),
    ];
    let mut input = RawInput {
        events: mouse.clone(),
        ..Default::default()
    };
    pointer.update(&mut input, &[], 1.0, false);
    assert_eq!(input.events, mouse);
    pointer.update(&mut input, &[pen(20.0, false)], 1.0, false);
    assert_eq!(input.events, [Event::PointerMoved(pos2(20.0, 100.0))]);

    input.events = vec![Event::Copy, Event::PointerGone];
    pointer.update(&mut input, &[], 1.0, false);
    assert_eq!(input.events, [Event::Copy]);
    pointer.update(
        &mut input,
        &[PenFrame {
            proximity: false,
            ..pen(20.0, false)
        }],
        1.0,
        false,
    );
    input.events = mouse.clone();
    pointer.update(&mut input, &[], 1.0, false);
    assert_eq!(input.events, mouse);
}

#[test]
fn focus_loss_ends_contact_until_the_pen_is_lifted() {
    let mut pointer = PenPointer::default();
    let mut input = RawInput::default();
    pointer.update(&mut input, &[pen(20.0, true)], 1.0, false);
    input.events = vec![Event::WindowFocused(false)];
    pointer.update(&mut input, &[], 1.0, false);
    assert!(
        input
            .events
            .contains(&button(pos2(20.0, 100.0), PointerButton::Primary, false))
    );
    input.events.clear();
    pointer.update(&mut input, &[pen(40.0, true)], 1.0, false);
    assert!(input.events.is_empty());
    pointer.update(&mut input, &[pen(40.0, false), pen(50.0, true)], 1.0, false);
    assert!(
        input
            .events
            .contains(&button(pos2(50.0, 100.0), PointerButton::Primary, true))
    );
}

#[test]
fn another_tool_cannot_interrupt_a_stroke() {
    let mut pointer = PenPointer::default();
    let mut input = RawInput::default();
    pointer.update(&mut input, &[pen(20.0, true)], 1.0, false);
    input.events.clear();
    pointer.update(
        &mut input,
        &[
            PenFrame {
                tool: 2,
                ..pen(40.0, true)
            },
            PenFrame {
                tool: 2,
                proximity: false,
                ..pen(40.0, false)
            },
        ],
        1.0,
        false,
    );
    assert!(input.events.is_empty());
    assert_eq!(pointer.tool, Some(1));
    assert!(pointer.buttons[0]);
}

#[test]
fn a_tip_without_a_position_waits_for_motion_and_invalid_coordinates_are_ignored() {
    let mut pointer = PenPointer::default();
    let mut input = RawInput::default();
    pointer.update(
        &mut input,
        &[
            PenFrame {
                position: None,
                ..pen(0.0, true)
            },
            pen(f32::NAN, true),
        ],
        1.0,
        false,
    );
    assert!(input.events.is_empty());
    pointer.update(&mut input, &[pen(20.0, true)], 1.0, false);
    assert!(
        input
            .events
            .contains(&button(pos2(20.0, 100.0), PointerButton::Primary, true))
    );
}

#[test]
fn tablet_taps_and_strokes_paint_and_undo_in_the_editor() {
    use crate::app::{EditorApp, Tool};

    let context = egui::Context::default();
    let mut app = EditorApp::with_context(&context, Vec::new(), false, None);
    app.dimensions = [128, 128];
    app.new_document();
    app.set_tool(Tool::Brush);
    app.brush.diameter = 8.0;
    app.brush.hardness = 1.0;
    let mut pointer = PenPointer::default();
    let mut run = |app: &mut EditorApp, frames: &[PenFrame]| {
        let mut input = RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                egui::vec2(1280.0, 860.0),
            )),
            time: Some(app.frames as f64 / 60.0),
            ..Default::default()
        };
        pointer.update(&mut input, frames, 1.0, false);
        app.pen_samples = std::mem::take(&mut pointer.samples);
        let _ = context.run(input, |ctx| app.show(ctx));
    };
    run(&mut app, &[]);
    let canvas = app.canvas_rect.unwrap();
    let zoom = app.session().unwrap().zoom;
    let sample = |x: f32, y: f32, down| PenFrame {
        position: Some(canvas.min + egui::vec2(x, y) * zoom),
        ..pen(0.0, down)
    };

    // Hover must not paint. A short tap arriving in one update still paints.
    run(&mut app, &[sample(20.0, 20.0, false)]);
    assert_eq!(app.session().unwrap().history.names().count(), 0);
    run(
        &mut app,
        &[sample(20.0, 20.0, true), sample(20.0, 20.0, false)],
    );
    assert_eq!(app.session().unwrap().history.names().count(), 1);
    let pixels = xuan::render::render(&app.session().unwrap().document);
    assert_eq!(pixels.get_pixel(20, 20)[3], 255);

    // A complete pressure stroke can arrive between two GUI frames. Retain its
    // bend and pressure changes rather than joining only the final positions.
    app.brush.diameter = 20.0;
    run(
        &mut app,
        &[
            PenFrame {
                pressure: Some(0.2),
                ..sample(20.0, 40.0, true)
            },
            PenFrame {
                pressure: Some(0.5),
                ..sample(50.0, 90.0, true)
            },
            PenFrame {
                pressure: Some(1.0),
                ..sample(90.0, 40.0, true)
            },
            PenFrame {
                pressure: Some(0.0),
                ..sample(90.0, 40.0, false)
            },
        ],
    );
    let pixels = xuan::render::render(&app.session().unwrap().document);
    assert_eq!(pixels.get_pixel(50, 90)[3], 255);
    assert_eq!(pixels.get_pixel(50, 40)[3], 0);
    assert_eq!(pixels.get_pixel(20, 47)[3], 0);
    assert_eq!(pixels.get_pixel(90, 47)[3], 255);
    app.command("undo");

    // An inverted pen erases without changing the selected tool or brush size.
    run(
        &mut app,
        &[
            PenFrame {
                eraser: true,
                pressure: Some(1.0),
                ..sample(20.0, 20.0, true)
            },
            PenFrame {
                eraser: true,
                pressure: Some(0.0),
                ..sample(20.0, 20.0, false)
            },
        ],
    );
    assert!(app.tool == Tool::Brush);
    assert_eq!(app.brush.diameter, 20.0);
    assert_eq!(
        xuan::render::render(&app.session().unwrap().document).get_pixel(20, 20)[3],
        0
    );
    app.command("undo");

    run(&mut app, &[sample(30.0, 60.0, true)]);
    run(&mut app, &[sample(60.0, 60.0, true)]);
    run(&mut app, &[sample(90.0, 60.0, true)]);
    run(&mut app, &[sample(90.0, 60.0, false)]);
    assert_eq!(app.session().unwrap().history.names().count(), 2);
    let pixels = xuan::render::render(&app.session().unwrap().document);
    for x in 30..90 {
        assert_eq!(pixels.get_pixel(x, 60)[3], 255);
    }
    app.command("undo");
    let pixels = xuan::render::render(&app.session().unwrap().document);
    assert_eq!(pixels.get_pixel(60, 60)[3], 0);
    assert_eq!(pixels.get_pixel(20, 20)[3], 255);

    // Releasing the middle pen button must finish panning, even though the
    // hovering pen immediately returns to the pressure-brush input path.
    let middle = |x| PenFrame {
        buttons: [false, false, true],
        ..sample(x, 60.0, false)
    };
    run(&mut app, &[middle(40.0)]);
    run(&mut app, &[middle(60.0)]);
    assert!(app.gesture.as_ref().is_some_and(|gesture| gesture.panning));
    run(&mut app, &[sample(60.0, 60.0, false)]);
    assert!(app.gesture.is_none());
    assert_eq!(app.session().unwrap().history.names().count(), 1);
    assert!(app.session().unwrap().pan.x > 0.0);
    let pan = app.session().unwrap().pan;
    let after_pan = |down| PenFrame {
        position: sample(60.0, 60.0, down).position.map(|pos| pos + pan),
        ..sample(60.0, 60.0, down)
    };
    run(&mut app, &[after_pan(true), after_pan(false)]);
    let pixels = xuan::render::render(&app.session().unwrap().document);
    assert_eq!(pixels.get_pixel(60, 60)[3], 255);
    assert_eq!(app.session().unwrap().history.names().count(), 2);

    // A quick tip tap with the middle button held is navigation, even when
    // both buttons have been released by the time the GUI processes the batch.
    run(
        &mut app,
        &[
            PenFrame {
                buttons: [true, false, true],
                ..after_pan(true)
            },
            after_pan(false),
        ],
    );
    assert_eq!(app.session().unwrap().history.names().count(), 2);
}

#[test]
fn pressure_controls_are_independent_and_missing_pressure_uses_the_selected_brush() {
    use crate::app::EditorApp;
    let ctx = egui::Context::default();
    let mut app = EditorApp::with_context(&ctx, Vec::new(), false, None);
    app.brush.diameter = 40.0;
    app.brush.opacity = 0.8;
    app.pen_sample = Some(Sample {
        position: Pos2::ZERO,
        pressure: Some(0.25),
        tilt: None,
        eraser: false,
        phase: Phase::Move,
    });
    assert_eq!(app.input_brush().diameter, 10.0);
    assert_eq!(app.input_brush().opacity, 0.8);
    app.pressure_size = false;
    app.pressure_opacity = true;
    assert_eq!(app.input_brush().diameter, 40.0);
    assert_eq!(app.input_brush().opacity, 0.2);
    app.pen_sample.as_mut().unwrap().pressure = None;
    assert_eq!(app.input_brush().opacity, 0.8);
    app.pen_sample.as_mut().unwrap().tilt = Some([60.0, -15.0]);
    assert_eq!(app.input_brush().tilt, [0.0; 2]);
    app.tilt_shape = true;
    assert_eq!(app.input_brush().tilt, [60.0, -15.0]);
    app.pen_sample = None;
    assert_eq!(app.input_brush().diameter, 40.0);
    assert_eq!(app.input_brush().tilt, [0.0; 2]);
}

#[test]
fn switching_to_a_mouse_releases_the_pen_and_preserves_the_mouse_click() {
    let mut pointer = PenPointer::default();
    let mut input = RawInput::default();
    pointer.update(&mut input, &[pen(20.0, false)], 1.0, false);
    let mouse = button(pos2(9.0, 8.0), PointerButton::Primary, true);
    input.events = vec![Event::PointerMoved(pos2(9.0, 8.0)), mouse.clone()];
    pointer.update(
        &mut input,
        &[PenFrame {
            mouse_handoff: true,
            proximity: false,
            ..pen(20.0, false)
        }],
        1.0,
        false,
    );
    assert_eq!(input.events.last(), Some(&mouse));
    assert!(pointer.sample.is_none());
}
