use super::*;
use crate::document::{Mask, Transform};
use image::Luma;
use std::sync::Arc;

fn solid(name: &str, color: [u8; 4]) -> Layer {
    Layer::image(name, RgbaImage::from_pixel(4, 1, Rgba(color)))
}

fn mask(values: [u8; 4]) -> Layer {
    let mut layer = Layer::mask("Mask", 4, 1);
    layer.mask.as_mut().unwrap().pixels =
        Arc::new(GrayImage::from_raw(4, 1, values.to_vec()).unwrap());
    layer
}

fn assert_pixels(document: &Document, expected: [[u8; 4]; 4]) {
    let image = render_pixels(document, 4, 1);
    for (x, expected) in expected.into_iter().enumerate() {
        assert_eq!(image.get_pixel(x as u32, 0).0, expected, "pixel {x}");
        assert_eq!(
            pixel_at(document, Point::new(x as f32 + 0.5, 0.5)).map(|v| (v * 255.0).round() as u8),
            expected,
        );
    }
}

#[test]
fn standalone_masks_apply_to_the_composite_below_and_stack() {
    let mut document = Document::new(4, 1).unwrap();
    let mut above = solid("Above mask", [0, 0, 255, 255]);
    above.transform = Transform::new(1, 1);
    above.transform.x = 3.0;
    document.layers = vec![
        solid("Bottom", [255, 0, 0, 255]),
        solid("Below mask", [0, 255, 0, 255]),
        mask([0, 128, 255, 0]),
        above,
    ];
    assert_pixels(
        &document,
        [[0; 4], [0, 255, 0, 128], [0, 255, 0, 255], [0, 0, 255, 255]],
    );
    document.layers.insert(3, mask([128; 4]));
    assert_pixels(
        &document,
        [[0; 4], [0, 255, 0, 64], [0, 255, 0, 128], [0, 0, 255, 255]],
    );
    assert_eq!(hit_test(&document, Point::new(0.5, 0.5)), None);
    assert_eq!(
        hit_test_bounds(&document, Point::new(0.5, 0.5)),
        Some(document.layers[1].id)
    );
}

#[test]
fn mask_visibility_enable_opacity_and_placement_control_the_effect() {
    let mut document = Document::new(4, 1).unwrap();
    document.layers = vec![solid("Below", [255, 0, 0, 255]), mask([0; 4])];
    assert_pixels(&document, [[0; 4]; 4]);
    document.layers[1].opacity = 0.5;
    assert_pixels(&document, [[255, 0, 0, 128]; 4]);
    document.layers[1].opacity = 0.0;
    assert_pixels(&document, [[255, 0, 0, 255]; 4]);
    document.layers[1].opacity = 1.0;
    document.layers[1].visible = false;
    assert_pixels(&document, [[255, 0, 0, 255]; 4]);
    document.layers[1].visible = true;
    document.layers[1].mask.as_mut().unwrap().enabled = false;
    assert_pixels(&document, [[255, 0, 0, 255]; 4]);
    document.layers[1].mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(1, 1, Luma([255]))),
        placement: Some(Transform {
            x: 1.0,
            ..Transform::new(2, 1)
        }),
        ..Mask::white()
    });
    assert_pixels(
        &document,
        [[0; 4], [255, 0, 0, 255], [255, 0, 0, 255], [0; 4]],
    );
}

#[test]
fn grouped_mask_excludes_outside_layers_and_upper_siblings() {
    let mut document = Document::new(4, 1).unwrap();
    let outside = solid("Outside", [255, 0, 0, 255]);
    let mut group = Layer::blank("Group", 4, 1);
    group.group = true;
    let mut lower = solid("Lower", [0, 255, 0, 255]);
    let mut upper = solid("Upper", [0, 0, 255, 255]);
    let mut scoped = mask([0, 128, 255, 0]);
    let mut above = solid("Above mask", [255, 255, 0, 255]);
    above.transform = Transform::new(1, 1);
    above.transform.x = 3.0;
    for layer in [&mut lower, &mut upper, &mut scoped, &mut above] {
        layer.parent = Some(group.id);
    }
    document.layers = vec![outside, group, lower, upper, scoped, above];
    assert_pixels(
        &document,
        [
            [255, 0, 0, 255],
            [127, 0, 128, 255],
            [0, 0, 255, 255],
            [255, 255, 0, 255],
        ],
    );
    assert_eq!(
        hit_test(&document, Point::new(0.5, 0.5)),
        Some(document.layers[0].id)
    );
    document.layers[1].visible = false;
    assert_pixels(&document, [[255, 0, 0, 255]; 4]);
}

#[test]
fn nested_group_masks_and_adjustments_keep_their_scope() {
    let mut document = Document::new(4, 1).unwrap();
    let mut outer = Layer::blank("Outer", 4, 1);
    outer.group = true;
    outer.opacity = 0.5;
    outer.mask = Some(Mask {
        pixels: Arc::new(GrayImage::from_pixel(1, 1, Luma([128]))),
        ..Mask::white()
    });
    let mut inner = Layer::blank("Inner", 4, 1);
    inner.group = true;
    inner.parent = Some(outer.id);
    let mut content = solid("Content", [255, 0, 0, 255]);
    content.parent = Some(inner.id);
    let mut adjustment = Layer::blank("Invert", 4, 1);
    adjustment.adjustment = Some(crate::document::Adjustment::Invert);
    adjustment.parent = Some(inner.id);
    let mut inner_mask = mask([128; 4]);
    inner_mask.parent = Some(inner.id);
    let mut outer_mask = mask([128; 4]);
    outer_mask.parent = Some(outer.id);
    document.layers = vec![outer, inner, content, adjustment, inner_mask, outer_mask];
    // Existing group coverage applies to both the content and the adjustment;
    // standalone masks then reduce the resulting alpha within their scopes.
    assert_pixels(&document, [[191, 64, 64, 16]; 4]);
    document.layers.push(mask([128; 4]));
    assert_pixels(&document, [[191, 64, 64, 8]; 4]);
    document.layers[5].mask.as_mut().unwrap().enabled = false;
    assert_pixels(&document, [[191, 64, 64, 16]; 4]);
}

#[test]
fn white_group_mask_preserves_pass_through_blending_and_adjustments() {
    let mut document = Document::new(4, 1).unwrap();
    let outside = solid("Outside", [80, 160, 220, 128]);
    let mut group = Layer::blank("Group", 4, 1);
    group.group = true;
    group.opacity = 0.7;
    let mut content = solid("Content", [200, 130, 90, 190]);
    content.parent = Some(group.id);
    let mut adjustment = Layer::blank("Invert", 4, 1);
    adjustment.parent = Some(group.id);
    adjustment.adjustment = Some(crate::document::Adjustment::Invert);
    adjustment.opacity = 0.4;
    let mut white = mask([255; 4]);
    white.parent = Some(group.id);
    document.layers = vec![outside, group, content, adjustment, white];
    for blend in crate::blend::BlendMode::ALL {
        document.layers[2].blend = blend;
        document.layers[4].visible = false;
        let expected = render_pixels(&document, 4, 1);
        document.layers[4].visible = true;
        assert_eq!(render_pixels(&document, 4, 1), expected, "{}", blend.name());
    }
    document.layers[4].mask.as_mut().unwrap().pixels = Arc::new(GrayImage::new(1, 1));
    assert_pixels(&document, [[80, 160, 220, 128]; 4]);
}
