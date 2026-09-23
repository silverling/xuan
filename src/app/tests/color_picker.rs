use egui::{
    Color32, Context, Event, FullOutput, Modifiers, PointerButton, Pos2, Rect, Shape, Vec2,
};

use super::{color_picker, theme};

fn frame(
    context: &Context,
    id: &str,
    color: &mut [u8; 4],
    events: Vec<Event>,
) -> (FullOutput, bool) {
    let mut changed = false;
    let output = context.run(
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(400.0, 600.0))),
            events,
            ..Default::default()
        },
        |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.push_id(id, |ui| changed = color_picker(ui, color));
            });
        },
    );
    (output, changed)
}

fn pointer(pos: Pos2, pressed: bool) -> Vec<Event> {
    vec![
        Event::PointerMoved(pos),
        Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        },
    ]
}

fn map_rect(output: &FullOutput) -> Rect {
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            Shape::Mesh(mesh) if mesh.vertices.len() > 1000 => Some(mesh.calc_bounds()),
            _ => None,
        })
        .expect("saturation/value map")
}

fn hue_rect(output: &FullOutput) -> Rect {
    let map = map_rect(output);
    output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            Shape::Mesh(mesh) => Some(mesh.calc_bounds()),
            _ => None,
        })
        .find(|rect| rect.top() > map.bottom() && (rect.width() - map.width()).abs() < 0.1)
        .expect("hue bar")
}

fn hue_marker(output: &FullOutput) -> Pos2 {
    let hue = hue_rect(output);
    output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            Shape::Path(path)
                if path.points.len() == 3 && (path.points[0].y - hue.center().y).abs() < 0.1 =>
            {
                Some(path.points[0])
            }
            _ => None,
        })
        .expect("hue triangle")
}

#[test]
fn hue_drag_persists_for_black_gray_white_and_transparent_colors() {
    for initial in [[0, 0, 0, 255], [128, 128, 128, 255], [255; 4], [0; 4]] {
        let context = Context::default();
        theme::apply(&context);
        let mut color = initial;
        let (output, _) = frame(&context, "foreground", &mut color, vec![]);
        let hue = hue_rect(&output);
        let start = Pos2::new(hue.left() + hue.width() * 0.25, hue.center().y);
        let end = Pos2::new(hue.left() + hue.width() * 0.5, hue.center().y);
        frame(&context, "foreground", &mut color, pointer(start, true));
        frame(
            &context,
            "foreground",
            &mut color,
            vec![Event::PointerMoved(end)],
        );
        frame(&context, "foreground", &mut color, pointer(end, false));
        let (output, changed) = frame(&context, "foreground", &mut color, vec![]);
        assert_eq!(color, initial);
        assert!(!changed, "hue alone should not report a change to gray RGB");
        assert!((hue_marker(&output).x - end.x).abs() < 0.1);

        // Raising saturation and brightness must use the hue chosen earlier.
        let map = map_rect(&output);
        let cyan = map.right_top() + Vec2::new(-1.0, 1.0);
        frame(&context, "foreground", &mut color, pointer(cyan, true));
        frame(&context, "foreground", &mut color, pointer(cyan, false));
        frame(&context, "foreground", &mut color, vec![]);
        assert!(
            color[0] < 30 && color[1] > 240 && color[2] > 240,
            "{color:?}"
        );
        assert_eq!(color[3], initial[3]);
    }
}

#[test]
fn picker_state_is_independent_and_tracks_external_color_changes() {
    let context = Context::default();
    theme::apply(&context);
    let mut foreground = [0, 0, 0, 255];
    let mut background = foreground;
    let (output, _) = frame(&context, "foreground", &mut foreground, vec![]);
    let hue = hue_rect(&output);
    let cyan = Pos2::new(hue.center().x, hue.center().y);
    frame(&context, "foreground", &mut foreground, pointer(cyan, true));
    frame(
        &context,
        "foreground",
        &mut foreground,
        pointer(cyan, false),
    );

    let (output, _) = frame(&context, "background", &mut background, vec![]);
    assert!((hue_marker(&output).x - hue.left()).abs() < 0.1);
    let (output, _) = frame(&context, "foreground", &mut foreground, vec![]);
    assert!((hue_marker(&output).x - cyan.x).abs() < 0.1);

    // An eyedropper, reset or another color well may replace the stored RGBA.
    foreground = [0, 0, 255, 0];
    let (output, changed) = frame(&context, "foreground", &mut foreground, vec![]);
    assert!(!changed);
    assert_eq!(foreground, [0, 0, 255, 0]);
    let blue_x = hue.left() + hue.width() * 2.0 / 3.0;
    assert!((hue_marker(&output).x - blue_x).abs() < 0.1);
}

#[test]
fn map_marker_stays_small_without_changing_other_circles_or_map_width() {
    for scale in [1.0, 1.75] {
        let context = Context::default();
        theme::apply(&context);
        context.set_pixels_per_point(scale);
        let mut color = [0, 0, 0, 255];
        let (output, _) = frame(&context, "foreground", &mut color, vec![]);
        let map = map_rect(&output);
        assert!((map.width() - 260.0).abs() < 1.0);
        let marker = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                Shape::Circle(circle) if circle.center.distance(map.left_bottom()) < 0.1 => {
                    Some(circle)
                }
                _ => None,
            })
            .expect("map selection circle");
        assert_eq!(marker.radius, 5.0);
        assert_eq!(marker.fill, Color32::BLACK);
        assert_eq!(marker.stroke.color, Color32::WHITE);
        assert!(
            output.shapes.iter().any(|shape| matches!(
                &shape.shape,
                Shape::Circle(circle) if circle.radius == 7.0 && circle.center.y < map.top()
            )),
            "blending radio buttons should retain their size"
        );
    }
}
