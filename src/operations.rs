use std::{collections::HashMap, sync::Arc};

use anyhow::{Result, ensure};
use image::{GrayImage, Luma, RgbaImage};
use uuid::Uuid;

use crate::{
    document::{Document, Layer, Point, Transform, validate_size},
    render, selection,
};

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
    ensure!(
        targets.len() >= 2 || document.active().is_some_and(|l| l.group),
        "Select at least two layers, or a layer above another"
    );
    let mut isolated = document.clone();
    for layer in &mut isolated.layers {
        if !targets.contains(&layer.id) && !layer.group {
            layer.visible = false;
        }
    }
    let pixels = render::render(&isolated);
    let mut merged = Layer::image(
        document
            .active()
            .map_or("Merged".into(), |l| l.name.clone()),
        pixels,
    );
    merged.parent = document
        .active()
        .and_then(|l| l.parent)
        .filter(|p| !targets.contains(p));
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
    let sx = width as f32 / document.width as f32;
    let sy = height as f32 / document.height as f32;
    let scale = |t: &mut Transform| {
        t.x *= sx;
        t.y *= sy;
        t.width *= sx;
        t.height *= sy;
    };
    for layer in &mut document.layers {
        scale(&mut layer.transform);
        if let Some(placement) = layer.mask.as_mut().and_then(|m| m.placement.as_mut()) {
            scale(placement);
        }
    }
    document.width = width;
    document.height = height;
    if let Some(selection) = &document.selection {
        document.selection = Some(Arc::new(image::imageops::resize(
            &**selection,
            width,
            height,
            image::imageops::FilterType::Triangle,
        )));
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
