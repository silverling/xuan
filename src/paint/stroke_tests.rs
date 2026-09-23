use super::*;

fn options(mode: PaintMode, mask_target: bool) -> StrokeOptions<'static> {
    StrokeOptions {
        mode,
        mask_target,
        source: None,
        clone_offset: Point::default(),
    }
}

fn compare(a: &Document, b: &Document, mask: bool) {
    let pixels = |document: &Document| {
        let layer = document.active().unwrap();
        if mask {
            layer.mask.as_ref().unwrap().pixels.as_raw().clone()
        } else {
            render::render(document).into_raw()
        }
    };
    let a = pixels(a);
    let b = pixels(b);
    assert_eq!(a.len(), b.len());
    assert!(a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 1));
}

#[test]
fn low_opacity_fast_strokes_match_dense_strokes_without_dark_joins() {
    let brush = Brush {
        diameter: 53.0,
        hardness: 0.83,
        opacity: 0.3,
        color: [31, 57, 93, 173],
        ..Default::default()
    };
    for mask in [false, true] {
        for mode in [PaintMode::Paint, PaintMode::Erase] {
            for background in [[0; 4], [231, 195, 141, 255], [120, 90, 60, 127]] {
                let mut original = Document::new(240, 120).unwrap();
                original.active_mut().unwrap().pixels =
                    Some(Arc::new(RgbaImage::from_pixel(240, 120, Rgba(background))));
                original.selection = Some(Arc::new(GrayImage::from_fn(240, 120, |x, _| {
                    Luma([if x < 40 {
                        0
                    } else if x < 140 {
                        127
                    } else {
                        255
                    }])
                })));
                let start = Point::new(30.0, 60.0);
                let end = Point::new(210.0, 60.0);
                let mut expected = original.clone();
                stroke(&mut expected, start, end, &brush, options(mode, mask)).unwrap();
                for samples in [vec![30, 48, 100, 177, 210], (30..=210).collect()] {
                    let mut actual = original.clone();
                    let mut stroke = Stroke::default();
                    let mut previous = start;
                    // Include a stationary release and a return pass in the same gesture.
                    for x in samples.into_iter().chain([210, 210, 30]) {
                        let point = Point::new(x as f32, 60.0);
                        stroke
                            .segment(
                                &mut actual,
                                previous,
                                point,
                                &brush,
                                &brush,
                                options(mode, mask),
                            )
                            .unwrap();
                        previous = point;
                    }
                    compare(&actual, &expected, mask);
                }
            }
        }
    }
}

#[test]
fn stroke_pressure_uses_peak_coverage_and_new_strokes_build_up() {
    let mut document = Document::new(80, 80).unwrap();
    let original = document.clone();
    let point = Point::new(40.0, 40.0);
    let mut brush = Brush {
        diameter: 30.0,
        hardness: 1.0,
        ..Default::default()
    };
    let mut stroke = Stroke::default();
    for (pressure, expected) in [(0.1, 26), (0.3, 77), (0.6, 153), (0.2, 153)] {
        let previous = brush.clone();
        brush.opacity = pressure;
        stroke
            .segment(
                &mut document,
                point,
                point,
                &previous,
                &brush,
                options(PaintMode::Paint, false),
            )
            .unwrap();
        assert_eq!(
            document
                .active()
                .unwrap()
                .pixels
                .as_ref()
                .unwrap()
                .get_pixel(40, 40)[3],
            expected
        );
    }
    assert!(render::render(&original).pixels().all(|p| p[3] == 0));
    let mut next = Stroke::default();
    next.segment(
        &mut document,
        point,
        point,
        &brush,
        &brush,
        options(PaintMode::Paint, false),
    )
    .unwrap();
    assert_eq!(
        document
            .active()
            .unwrap()
            .pixels
            .as_ref()
            .unwrap()
            .get_pixel(40, 40)[3],
        173
    );
}

#[test]
fn stroke_coverage_follows_expanding_transformed_layers() {
    let brush = Brush {
        diameter: 25.0,
        opacity: 0.3,
        ..Default::default()
    };
    let mut original = Document::new(240, 160).unwrap();
    let mut layer = Layer::image(
        "Small",
        RgbaImage::from_pixel(32, 32, Rgba([235, 170, 80, 255])),
    );
    layer.transform.x = 100.0;
    layer.transform.y = 60.0;
    layer.transform.rotation = 25.0;
    layer.transform.flip_x = true;
    original.insert(layer);
    let start = Point::new(190.0, 100.0);
    let end = Point::new(40.0, 40.0);
    let mut expected = original.clone();
    stroke(
        &mut expected,
        start,
        end,
        &brush,
        options(PaintMode::Paint, false),
    )
    .unwrap();
    let mut actual = original.clone();
    let mut stroke = Stroke::default();
    let mut previous = start;
    for i in 0..=30 {
        let point = Point::new(start.x - i as f32 * 5.0, start.y - i as f32 * 2.0);
        stroke
            .segment(
                &mut actual,
                previous,
                point,
                &brush,
                &brush,
                options(PaintMode::Paint, false),
            )
            .unwrap();
        previous = point;
    }
    compare(&actual, &expected, false);
    assert_eq!(
        original
            .active()
            .unwrap()
            .pixels
            .as_ref()
            .unwrap()
            .dimensions(),
        (32, 32)
    );
}
