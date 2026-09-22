use std::{collections::HashMap, sync::Arc};

use anyhow::{Result, ensure};
use image::{GrayImage, Luma, RgbaImage};
use uuid::Uuid;

use crate::{
    document::{Document, Layer, Point, Transform, validate_size},
    render, selection,
};

pub fn transform_box(document: &Document, mask_target: bool) -> Option<Transform> {
    let active = document.active()?;
    if mask_target {
        return Some(
            active
                .mask
                .as_ref()
                .and_then(|m| m.placement)
                .unwrap_or(active.transform),
        );
    }
    if document.selected.len() <= 1 && !active.group {
        return Some(active.transform);
    }
    let targets = document.transform_targets();
    let mut min = Point::new(f32::MAX, f32::MAX);
    let mut max = Point::new(f32::MIN, f32::MIN);
    for layer in document
        .layers
        .iter()
        .filter(|l| targets.contains(&l.id) && !l.group)
    {
        for point in layer.transform.corners() {
            min.x = min.x.min(point.x);
            min.y = min.y.min(point.y);
            max.x = max.x.max(point.x);
            max.y = max.y.max(point.y);
        }
    }
    if min.x > max.x {
        return Some(active.transform);
    }
    Some(Transform {
        x: min.x,
        y: min.y,
        width: (max.x - min.x).max(1.0),
        height: (max.y - min.y).max(1.0),
        ..Transform::new(1, 1)
    })
}

pub fn apply_transform(document: &mut Document, new: Transform, mask_target: bool) -> Result<()> {
    ensure!(new.valid(), "Invalid transform");
    let Some(old) = transform_box(document, mask_target) else {
        return Ok(());
    };
    let targets = if mask_target {
        document.active.into_iter().collect()
    } else {
        document.transform_targets()
    };
    for layer in &mut document.layers {
        if !targets.contains(&layer.id) || layer.locked {
            continue;
        }
        if mask_target {
            if let Some(mask) = &mut layer.mask {
                mask.placement = Some(new);
                mask.linked = false;
            }
        } else {
            let transform = if targets.len() == 1 {
                new
            } else {
                layer.transform.following(old, new)
            };
            layer.set_transform(transform);
        }
    }
    Ok(())
}

pub fn duplicate(document: &mut Document) {
    let targets = document.transform_targets();
    let ids: HashMap<_, _> = targets.iter().map(|id| (*id, Uuid::new_v4())).collect();
    let mut copies = Vec::new();
    for layer in &document.layers {
        if !targets.contains(&layer.id) {
            continue;
        }
        let mut copy = layer.clone();
        copy.id = ids[&layer.id];
        copy.name = format!("{} copy", layer.name);
        copy.parent = copy.parent.map(|id| ids.get(&id).copied().unwrap_or(id));
        copy.clip_to = copy.clip_to.map(|id| ids.get(&id).copied().unwrap_or(id));
        copies.push(copy);
    }
    if copies.is_empty() {
        return;
    }
    let index = document
        .layers
        .iter()
        .rposition(|l| targets.contains(&l.id))
        .unwrap()
        + 1;
    document.active = document.active.and_then(|id| ids.get(&id).copied());
    document.selected = document
        .selected
        .iter()
        .filter_map(|id| ids.get(id).copied())
        .collect();
    document.layers.splice(index..index, copies);
}

pub fn copy_layers(source: &Document, destination: &mut Document, root: Uuid) -> Result<()> {
    let targets = source.descendants(root);
    let ids: HashMap<_, _> = targets.iter().map(|id| (*id, Uuid::new_v4())).collect();
    let anchor = source
        .layers
        .iter()
        .find(|l| l.id == root)
        .ok_or_else(|| anyhow::anyhow!("The dragged layer no longer exists"))?
        .transform
        .center();
    let offset = Point::new(
        destination.width as f32 * 0.5 - anchor.x,
        destination.height as f32 * 0.5 - anchor.y,
    );
    let mut copies = Vec::new();
    for layer in source.layers.iter().filter(|l| targets.contains(&l.id)) {
        let mut copy = layer.clone();
        if let Some(clip) = layer.clip_to.filter(|id| !targets.contains(id))
            && let Some(pixels) = &layer.pixels
        {
            let base = source.layers.iter().find(|l| l.id == clip).unwrap();
            let baked = crate::gpu::bake_alpha(source, base, pixels, layer.transform)
                .unwrap_or_else(|| {
                    let mut baked = (**pixels).clone();
                    let (width, height) = baked.dimensions();
                    for (x, y, pixel) in baked.enumerate_pixels_mut() {
                        let point = layer.transform.point(Point::new(
                            (x as f32 + 0.5) / width as f32,
                            (y as f32 + 0.5) / height as f32,
                        ));
                        pixel[3] = (pixel[3] as f32 * render::layer_alpha(source, base, point, 0))
                            .round() as u8;
                    }
                    baked
                });
            copy.pixels = Some(Arc::new(baked));
            copy.shape = None;
            copy.text = None;
            copy.raw = None;
        }
        copy.id = ids[&layer.id];
        copy.parent = layer.parent.and_then(|id| ids.get(&id).copied());
        copy.clip_to = layer.clip_to.and_then(|id| ids.get(&id).copied());
        copy.transform.x += offset.x;
        copy.transform.y += offset.y;
        if let Some(placement) = copy.mask.as_mut().and_then(|m| m.placement.as_mut()) {
            placement.x += offset.x;
            placement.y += offset.y;
        }
        copies.push(copy);
    }
    destination.layers.extend(copies);
    destination.select(ids[&root], false);
    destination.validate()
}

pub fn group(document: &mut Document) {
    let parent = document.active().and_then(|l| l.parent);
    let mut group = Layer::blank("Group", document.width, document.height);
    group.group = true;
    group.parent = parent;
    let id = group.id;
    for layer in &mut document.layers {
        if document.selected.contains(&layer.id) && layer.parent == parent {
            layer.parent = Some(id);
        }
    }
    let index = document
        .layers
        .iter()
        .rposition(|l| l.parent == Some(id))
        .map_or(document.layers.len(), |i| i + 1);
    document.layers.insert(index, group);
    document.select(id, false);
}

pub fn ungroup(document: &mut Document) {
    let Some(group) = document.active().filter(|l| l.group).cloned() else {
        return;
    };
    for layer in &mut document.layers {
        if layer.parent == Some(group.id) {
            layer.parent = group.parent;
        }
    }
    document.layers.retain(|l| l.id != group.id);
    document.active = document.layers.last().map(|l| l.id);
    document.selected = document.active.into_iter().collect();
}

pub fn merge_selected(document: &mut Document, down: bool) -> Result<()> {
    let mut targets = document.transform_targets();
    if down && targets.len() == 1 {
        let index = document
            .layers
            .iter()
            .position(|l| Some(l.id) == document.active)
            .unwrap_or(0);
        if index > 0 {
            let parent = document.layers[index].parent;
            if let Some(layer) = document.layers[..index]
                .iter()
                .rev()
                .find(|l| l.parent == parent)
            {
                targets.extend(document.descendants(layer.id));
            }
        }
    }
    // Baking a standalone mask must include its entire lower stack, otherwise
    // removing it would reveal layers that were previously masked out.
    for (index, mask) in document.layers.iter().enumerate().rev() {
        if mask.standalone_mask && targets.contains(&mask.id) {
            for layer in document.layers[..index]
                .iter()
                .filter(|l| l.parent == mask.parent)
            {
                targets.extend(document.descendants(layer.id));
            }
        }
    }
    ensure!(
        targets.len() >= 2 || document.active().is_some_and(|l| l.group),
        "Select at least two layers, or a layer above another"
    );
    let parents: std::collections::HashSet<_> = document
        .layers
        .iter()
        .filter(|layer| {
            targets.contains(&layer.id) && !layer.parent.is_some_and(|id| targets.contains(&id))
        })
        .map(|layer| layer.parent)
        .collect();
    let parent = if parents.len() == 1 {
        *parents.iter().next().unwrap()
    } else {
        None
    };
    let mut isolated = document.clone();
    for layer in &mut isolated.layers {
        if !targets.contains(&layer.id) {
            if layer.group && parent.is_some() {
                // The merged layer inherits these ancestors after rasterization.
                layer.visible = true;
                layer.opacity = 1.0;
                layer.mask = None;
            } else if !layer.group {
                layer.visible = false;
            }
        }
    }
    let pixels = render::render(&isolated);
    let mut merged = Layer::image(
        document
            .active()
            .map_or("Merged".into(), |l| l.name.clone()),
        pixels,
    );
    merged.parent = parent;
    let insert = document
        .layers
        .iter()
        .rposition(|l| targets.contains(&l.id))
        .unwrap_or(0);
    let remaining_below = document.layers[..insert]
        .iter()
        .filter(|l| !targets.contains(&l.id))
        .count();
    document.layers.retain(|l| !targets.contains(&l.id));
    for layer in &mut document.layers {
        if layer.clip_to.is_some_and(|id| targets.contains(&id)) {
            layer.clip_to = Some(merged.id);
        }
    }
    document.select(merged.id, false);
    document
        .layers
        .insert(remaining_below.min(document.layers.len()), merged);
    Ok(())
}

pub fn canvas_size(
    document: &mut Document,
    width: u32,
    height: u32,
    anchor: [f32; 2],
) -> Result<()> {
    validate_size(width, height)?;
    let dx = (width as f32 - document.width as f32) * anchor[0];
    let dy = (height as f32 - document.height as f32) * anchor[1];
    for layer in &mut document.layers {
        layer.transform.x += dx;
        layer.transform.y += dy;
        if let Some(placement) = layer.mask.as_mut().and_then(|m| m.placement.as_mut()) {
            placement.x += dx;
            placement.y += dy;
        }
    }
    document.width = width;
    document.height = height;
    document.selection = None;
    Ok(())
}

pub fn crop(document: &mut Document, start: Point, end: Point) -> Result<()> {
    let left = start.x.min(end.x).round();
    let top = start.y.min(end.y).round();
    let width = (end.x - start.x).abs().round() as u32;
    let height = (end.y - start.y).abs().round() as u32;
    validate_size(width, height)?;
    for layer in &mut document.layers {
        layer.transform.x -= left;
        layer.transform.y -= top;
        if let Some(placement) = layer.mask.as_mut().and_then(|m| m.placement.as_mut()) {
            placement.x -= left;
            placement.y -= top;
        }
    }
    document.width = width;
    document.height = height;
    document.selection = None;
    Ok(())
}

pub fn image_size(document: &mut Document, width: u32, height: u32) -> Result<()> {
    validate_size(width, height)?;
    let old = Transform::new(document.width, document.height);
    let new = Transform::new(width, height);
    let scale = |t: &mut Transform| *t = t.following(old, new);
    for layer in &mut document.layers {
        scale(&mut layer.transform);
        if let Some(placement) = layer.mask.as_mut().and_then(|m| m.placement.as_mut()) {
            scale(placement);
        }
    }
    document.width = width;
    document.height = height;
    if let Some(selection) = &document.selection {
        document.selection = Some(Arc::new(crate::gpu::resize_gray(selection, width, height)));
    }
    Ok(())
}

pub fn flip_canvas(document: &mut Document, horizontal: bool) {
    let flip = |t: &mut Transform| {
        if horizontal {
            t.x = document.width as f32 - t.x - t.width;
            t.flip_x = !t.flip_x;
        } else {
            t.y = document.height as f32 - t.y - t.height;
            t.flip_y = !t.flip_y;
        }
        t.rotation = -t.rotation;
    };
    for layer in &mut document.layers {
        flip(&mut layer.transform);
        if let Some(placement) = layer.mask.as_mut().and_then(|m| m.placement.as_mut()) {
            flip(placement);
        }
    }
    if let Some(selection) = &document.selection {
        document.selection = Some(Arc::new(if horizontal {
            image::imageops::flip_horizontal(&**selection)
        } else {
            image::imageops::flip_vertical(&**selection)
        }));
    }
}

pub fn copy_pixels(document: &Document, merged: bool) -> Option<(RgbaImage, Point)> {
    let image = if merged {
        render::render(document)
    } else {
        let layer = document.active()?;
        let mut isolated = document.clone();
        isolated.layers = vec![Layer {
            parent: None,
            visible: true,
            clip_to: None,
            ..layer.clone()
        }];
        render::render(&isolated)
    };
    let (left, top, right, bottom) = if let Some(mask) = &document.selection {
        selection::bounds(mask)?
    } else {
        (0, 0, document.width, document.height)
    };
    let mut image =
        image::imageops::crop_imm(&image, left, top, right - left, bottom - top).to_image();
    if let Some(mask) = &document.selection {
        for (x, y, pixel) in image.enumerate_pixels_mut() {
            pixel[3] =
                (u16::from(pixel[3]) * u16::from(mask.get_pixel(x + left, y + top)[0]) / 255) as u8;
        }
    }
    Some((image, Point::new(left as f32, top as f32)))
}

pub fn selection_from_layer(document: &mut Document, mask_target: bool) {
    let Some(layer) = document.active() else {
        return;
    };
    if let Some(mask) = crate::gpu::coverage_image(
        document,
        layer,
        [document.width, document.height],
        if mask_target {
            crate::gpu::CoverageMode::Mask
        } else {
            crate::gpu::CoverageMode::Alpha
        },
        None,
    ) {
        document.selection = Some(Arc::new(mask));
        return;
    }
    let mask = GrayImage::from_fn(document.width, document.height, |x, y| {
        let point = Point::new(x as f32 + 0.5, y as f32 + 0.5);
        let value = if mask_target {
            render::own_mask(layer, point)
        } else {
            render::layer_alpha(document, layer, point, 0)
        };
        Luma([(value * 255.0).round() as u8])
    });
    document.selection = Some(Arc::new(mask));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merging_a_standalone_mask_bakes_all_lower_siblings() {
        let mut document = Document::new(4, 2).unwrap();
        document.layers[0].pixels = Some(Arc::new(RgbaImage::from_pixel(
            4,
            2,
            image::Rgba([255, 0, 0, 255]),
        )));
        document.insert(Layer::image(
            "Top",
            RgbaImage::from_pixel(4, 2, image::Rgba([0, 255, 0, 255])),
        ));
        group(&mut document);
        document.insert(Layer::image(
            "Middle",
            RgbaImage::from_pixel(4, 2, image::Rgba([0, 0, 255, 255])),
        ));
        let mut mask = Layer::mask("Mask", 4, 2);
        mask.mask.as_mut().unwrap().pixels = Arc::new(GrayImage::from_pixel(4, 2, Luma([128])));
        document.insert(mask);
        let before = render::render(&document);
        merge_selected(&mut document, true).unwrap();
        document.validate().unwrap();
        assert_eq!(render::render(&document), before);
        assert_eq!(document.layers.len(), 3);
        assert!(!document.layers.iter().any(|l| l.standalone_mask));
    }

    #[test]
    fn linked_placed_masks_follow_transforms_and_unlinked_masks_stay_put() {
        let mut doc = Document::new(20, 20).unwrap();
        doc.layers[0].mask = Some(crate::document::Mask {
            placement: Some(Transform {
                x: 4.0,
                ..Transform::new(20, 20)
            }),
            ..crate::document::Mask::white()
        });
        let mut moved = doc.layers[0].transform;
        moved.x = 10.0;
        apply_transform(&mut doc, moved, false).unwrap();
        assert_eq!(
            doc.layers[0].mask.as_ref().unwrap().placement.unwrap().x,
            14.0
        );
        doc.layers[0].mask.as_mut().unwrap().linked = false;
        moved.x = 15.0;
        apply_transform(&mut doc, moved, false).unwrap();
        assert_eq!(
            doc.layers[0].mask.as_ref().unwrap().placement.unwrap().x,
            14.0
        );
    }

    #[test]
    fn resizing_rotated_layers_scales_all_corners() {
        let mut doc = Document::new(100, 100).unwrap();
        doc.layers[0].transform.rotation = 35.0;
        let corners = doc.layers[0].transform.corners();
        image_size(&mut doc, 200, 50).unwrap();
        for (old, new) in corners.into_iter().zip(doc.layers[0].transform.corners()) {
            assert!(new.distance(Point::new(old.x * 2.0, old.y * 0.5)) < 0.001);
        }
    }

    #[test]
    fn merging_inside_a_masked_folder_applies_ancestor_coverage_once() {
        let mut doc = Document::new(4, 4).unwrap();
        doc.layers.clear();
        let mut folder = Layer::blank("folder", 4, 4);
        folder.group = true;
        folder.opacity = 0.5;
        let mut a = Layer::image(
            "a",
            RgbaImage::from_pixel(4, 4, image::Rgba([255, 0, 0, 255])),
        );
        a.parent = Some(folder.id);
        let mut b = Layer::image(
            "b",
            RgbaImage::from_pixel(4, 4, image::Rgba([0, 0, 255, 0])),
        );
        b.parent = Some(folder.id);
        doc.layers = vec![a.clone(), b.clone(), folder];
        doc.active = Some(b.id);
        doc.selected = [a.id, b.id].into_iter().collect();
        let before = render::render(&doc);
        merge_selected(&mut doc, false).unwrap();
        assert_eq!(render::render(&doc), before);
        doc.validate().unwrap();
    }

    #[test]
    fn group_rotation_moves_children_about_a_shared_center() {
        let mut doc = Document::new(100, 100).unwrap();
        doc.layers.clear();
        let mut left = Layer::blank("Left", 10, 10);
        left.transform.x = 10.0;
        left.transform.y = 20.0;
        let mut right = Layer::blank("Right", 10, 10);
        right.transform.x = 70.0;
        right.transform.y = 20.0;
        doc.selected = [left.id, right.id].into_iter().collect();
        doc.active = Some(left.id);
        doc.layers = vec![left, right];
        let old = transform_box(&doc, false).unwrap();
        let before = doc
            .layers
            .iter()
            .map(|l| l.transform.corners())
            .collect::<Vec<_>>();
        let mut new = old;
        new.rotation = 90.0;
        apply_transform(&mut doc, new, false).unwrap();
        for (layer, corners) in doc.layers.iter().zip(before) {
            for (before, after) in corners.into_iter().zip(layer.transform.corners()) {
                assert!(new.point(old.inverse(before)).distance(after) < 0.001);
            }
        }
        doc.validate().unwrap();
    }

    #[test]
    fn resize_and_crop_preserve_source_resolution_and_undo_assets() {
        let mut doc = Document::new(100, 80).unwrap();
        doc.layers[0] = Layer::image("Image", RgbaImage::new(100, 80));
        let original = doc.layers[0].pixels.clone().unwrap();
        image_size(&mut doc, 50, 40).unwrap();
        assert!(Arc::ptr_eq(
            &original,
            doc.layers[0].pixels.as_ref().unwrap()
        ));
        assert_eq!(doc.layers[0].transform.width, 50.0);
        crop(&mut doc, Point::new(10.0, 5.0), Point::new(40.0, 35.0)).unwrap();
        assert_eq!((doc.width, doc.height), (30, 30));
        assert_eq!(doc.layers[0].transform.x, -10.0);
    }

    #[test]
    fn duplicate_folder_remaps_child_and_clipping_references() {
        let mut doc = Document::new(2, 2).unwrap();
        doc.layers[0].group = true;
        let parent = doc.layers[0].id;
        doc.insert(Layer::blank("Child", 2, 2));
        doc.select(parent, false);
        duplicate(&mut doc);
        doc.validate().unwrap();
        assert_eq!(doc.layers.len(), 4);
        assert_ne!(doc.active, Some(parent));
        assert_eq!(doc.descendants(doc.active.unwrap()).len(), 2);
    }
}
