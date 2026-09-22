use std::sync::Arc;

use image::{GrayImage, Luma, Rgba, RgbaImage};

use crate::{
    document::{Adjustment, Document, Layer, Point},
    effects::Filter,
    operations,
};

use super::render;

fn mask(owner: &Layer, value: u8) -> Layer {
    let mut mask = Layer::mask("Mask", 4, 4);
    mask.parent = Some(owner.id);
    mask.transform = owner.transform;
    mask.mask.as_mut().unwrap().pixels = Arc::new(GrayImage::from_pixel(1, 1, Luma([value])));
    mask
}

#[test]
fn attached_effects_run_bottom_up_and_only_affect_their_image() {
    let mut doc = Document::new(8, 4).unwrap();
    let background = Layer::image(
        "Background",
        RgbaImage::from_pixel(8, 4, Rgba([0, 0, 255, 255])),
    );
    let mut owner = Layer::image(
        "Image",
        RgbaImage::from_pixel(4, 4, Rgba([64, 64, 64, 255])),
    );
    owner.opacity = 0.5;
    let original = owner.pixels.clone();
    let mut exposure = Layer::blank("Exposure", 4, 4);
    exposure.parent = Some(owner.id);
    exposure.adjustment = Some(Adjustment::Exposure {
        exposure: 1.0,
        offset: 0.0,
        gamma: 1.0,
    });
    let mut invert = Layer::blank("Invert", 4, 4);
    invert.parent = Some(owner.id);
    invert.adjustment = Some(Adjustment::Invert);
    let masks = [mask(&owner, 128), mask(&owner, 128)];
    doc.select(owner.id, false);
    doc.layers = vec![
        background,
        owner,
        exposure,
        invert,
        masks[0].clone(),
        masks[1].clone(),
    ];
    doc.validate().unwrap();
    let rendered = render(&doc);
    assert_eq!(rendered.get_pixel(6, 2).0, [0, 0, 255, 255]);
    // Exposure doubles linear-light values before inversion and both masks.
    assert_eq!(rendered.get_pixel(2, 2).0, [21, 21, 244, 255]);
    assert_eq!(doc.layers[1].pixels, original);

    doc.layers.swap(2, 3);
    assert_ne!(render(&doc).get_pixel(2, 2), rendered.get_pixel(2, 2));
    doc.layers[1].visible = false;
    assert_eq!(render(&doc).get_pixel(2, 2).0, [0, 0, 255, 255]);
    doc.layers[1].visible = true;
    for layer in &mut doc.layers[2..] {
        layer.visible = false;
    }
    assert_eq!(render(&doc).get_pixel(2, 2).0, [32, 32, 160, 255]);
}

#[test]
fn mask_and_blur_order_changes_the_result_and_round_trips() {
    let mut doc = Document::new(8, 8).unwrap();
    let owner = Layer::image("Image", RgbaImage::from_pixel(8, 8, Rgba([255; 4])));
    let mut mask = mask(&owner, 255);
    mask.mask.as_mut().unwrap().pixels = Arc::new(GrayImage::from_fn(8, 8, |x, _| {
        Luma([if x < 4 { 255 } else { 0 }])
    }));
    let mut blur = Layer::blank("Blur", 8, 8);
    blur.parent = Some(owner.id);
    blur.filter = Some(Filter::GaussianBlur { radius: 1.0 });
    doc.select(owner.id, false);
    doc.layers = vec![owner, mask, blur];
    let blurred_mask = render(&doc);
    assert!(blurred_mask.get_pixel(4, 4)[3] > 0);
    doc.layers.swap(1, 2);
    assert_eq!(render(&doc).get_pixel(4, 4)[3], 0);
    assert_eq!(super::hit_test(&doc, Point::new(4.5, 4.5)), None);
    operations::selection_from_layer(&mut doc, false);
    assert_eq!(doc.selection.as_ref().unwrap().get_pixel(4, 4)[0], 0);
    doc.selection = None;
    assert_eq!(
        operations::copy_pixels(&doc, false).unwrap().0,
        render(&doc)
    );

    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("effects.xuan");
    crate::io::save(&doc, &path).unwrap();
    let loaded = crate::io::load(&path).unwrap();
    assert_eq!(loaded.layers[1].filter, doc.layers[1].filter);
    assert_eq!(loaded.layers[2].parent, doc.active);
    assert_eq!(render(&loaded), render(&doc));
    let mut archive = zip::ZipArchive::new(std::fs::File::open(path).unwrap()).unwrap();
    let manifest: serde_json::Value =
        serde_json::from_reader(archive.by_name("manifest.json").unwrap()).unwrap();
    assert_eq!(manifest["version"], 4);
}

#[test]
fn image_effects_follow_duplicate_delete_and_linked_transforms() {
    let mut doc = Document::new(4, 4).unwrap();
    let owner = Layer::image("Image", RgbaImage::from_pixel(4, 4, Rgba([255; 4])));
    let linked = mask(&owner, 255);
    let mut unlinked = mask(&owner, 128);
    unlinked.mask.as_mut().unwrap().linked = false;
    let mut moved = owner.transform;
    moved.x = 3.0;
    doc.select(owner.id, false);
    doc.layers = vec![owner, linked, unlinked];
    operations::apply_transform(&mut doc, moved, false).unwrap();
    assert_eq!(doc.layers[1].transform.x, 3.0);
    assert_eq!(doc.layers[2].transform.x, 0.0);
    operations::duplicate(&mut doc);
    assert_eq!(doc.layers.len(), 6);
    assert_eq!(doc.layers[4].parent, doc.active);
    assert_eq!(doc.layers[5].parent, doc.active);
    doc.validate().unwrap();
    doc.delete_selected();
    assert_eq!(doc.layers.len(), 3);
    doc.validate().unwrap();
}

#[test]
fn standalone_filters_process_the_lower_stack_and_can_be_disabled() {
    let mut doc = Document::new(8, 8).unwrap();
    let pixels = RgbaImage::from_fn(8, 8, |x, _| Rgba([if x < 4 { 255 } else { 0 }, 0, 0, 255]));
    let mut blur = Layer::blank("Blur", 8, 8);
    blur.filter = Some(Filter::GaussianBlur { radius: 1.0 });
    let mut top = Layer::image("Top", RgbaImage::from_pixel(1, 1, Rgba([0, 255, 0, 255])));
    top.transform.x = 4.0;
    top.transform.y = 4.0;
    doc.select(top.id, false);
    doc.layers = vec![Layer::image("Image", pixels.clone()), blur, top];
    doc.validate().unwrap();
    let filtered = render(&doc);
    assert!(filtered.get_pixel(4, 3)[0] > 0);
    assert_eq!(filtered.get_pixel(4, 4).0, [0, 255, 0, 255]);
    doc.layers[1].visible = false;
    assert_eq!(render(&doc).get_pixel(4, 3), pixels.get_pixel(4, 3));
}

#[test]
fn legacy_masks_become_children_without_changing_pixels_or_placement() {
    let mut doc = Document::new(8, 8).unwrap();
    let mut owner = Layer::image(
        "Legacy",
        RgbaImage::from_pixel(4, 4, Rgba([50, 100, 150, 255])),
    );
    owner.transform.x = 2.0;
    owner.transform.y = 1.0;
    owner.mask = mask(&owner, 128).mask;
    doc.select(owner.id, false);
    doc.layers = vec![owner];
    let before = render(&doc);
    doc.promote_image_masks();
    doc.validate().unwrap();
    assert_eq!(doc.layers.len(), 2);
    assert_eq!(doc.layers[1].parent, doc.active);
    assert!(doc.layers[0].mask.is_none());
    assert_eq!(render(&doc), before);
}
