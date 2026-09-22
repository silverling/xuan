use super::*;

#[test]
fn pressure_interpolates_size_and_opacity_and_preserves_selection_and_history_pixels() {
    let mut document = Document::new(80, 40).unwrap();
    let before = document.clone();
    let brush = Brush {
        diameter: 20.0,
        hardness: 1.0,
        color: [255, 0, 0, 255],
        ..Default::default()
    };
    let small = Brush {
        diameter: 4.0,
        opacity: 0.2,
        ..brush.clone()
    };
    stroke_varying(
        &mut document,
        Point::new(10.0, 20.0),
        Point::new(60.0, 20.0),
        &small,
        &brush,
        StrokeOptions {
            mode: PaintMode::Paint,
            mask_target: false,
            source: None,
            clone_offset: Point::default(),
        },
    )
    .unwrap();
    let pixels = render::render(&document);
    assert!(pixels.get_pixel(12, 20)[3] < pixels.get_pixel(55, 20)[3]);
    assert_eq!(pixels.get_pixel(12, 26)[3], 0);
    assert!(pixels.get_pixel(55, 26)[3] > 0);
    assert!(render::render(&before).pixels().all(|pixel| pixel[3] == 0));

    document.selection = Some(Arc::new(selection::rectangle(
        80,
        40,
        Point::new(0.0, 0.0),
        Point::new(40.0, 40.0),
        false,
    )));
    let original = document.clone();
    stroke_varying(
        &mut document,
        Point::new(10.0, 20.0),
        Point::new(60.0, 20.0),
        &small,
        &brush,
        StrokeOptions {
            mode: PaintMode::Erase,
            mask_target: false,
            source: None,
            clone_offset: Point::default(),
        },
    )
    .unwrap();
    let erased = render::render(&document);
    assert!(erased.get_pixel(20, 20)[3] < render::render(&original).get_pixel(20, 20)[3]);
    assert_eq!(erased.get_pixel(55, 20), pixels.get_pixel(55, 20));
}

#[test]
fn stationary_pressure_changes_use_the_new_sample() {
    let mut document = Document::new(40, 40).unwrap();
    let brush = Brush {
        diameter: 20.0,
        hardness: 1.0,
        ..Default::default()
    };
    let small = Brush {
        diameter: 2.0,
        opacity: 0.1,
        ..brush.clone()
    };
    stroke_varying(
        &mut document,
        Point::new(20.0, 20.0),
        Point::new(20.0, 20.0),
        &small,
        &brush,
        StrokeOptions {
            mode: PaintMode::Paint,
            mask_target: false,
            source: None,
            clone_offset: Point::default(),
        },
    )
    .unwrap();
    assert_eq!(render::render(&document).get_pixel(25, 20)[3], 255);
}

#[test]
fn tilt_flattens_and_rotates_the_brush_footprint() {
    for (tilt, painted, empty) in [
        ([60.0, 0.0], (27, 20), (20, 27)),
        ([0.0, 60.0], (20, 27), (27, 20)),
    ] {
        let mut document = Document::new(40, 40).unwrap();
        let brush = Brush {
            diameter: 20.0,
            hardness: 1.0,
            tilt,
            ..Default::default()
        };
        stroke(
            &mut document,
            Point::new(20.0, 20.0),
            Point::new(20.0, 20.0),
            &brush,
            StrokeOptions {
                mode: PaintMode::Paint,
                mask_target: false,
                source: None,
                clone_offset: Point::default(),
            },
        )
        .unwrap();
        let pixels = render::render(&document);
        assert_eq!(pixels.get_pixel(painted.0, painted.1)[3], 255);
        assert_eq!(pixels.get_pixel(empty.0, empty.1)[3], 0);
    }
}
