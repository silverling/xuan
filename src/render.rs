use image::{GrayImage, Rgba, RgbaImage};
use rayon::prelude::*;

use crate::{
    blend::composite,
    document::{Document, Layer, Point},
};

pub fn sample(image: &RgbaImage, unit: Point) -> [f32; 4] {
    if !(0.0..1.0).contains(&unit.x) || !(0.0..1.0).contains(&unit.y) {
        return [0.0; 4];
    }
    let x = unit.x * image.width() as f32 - 0.5;
    let y = unit.y * image.height() as f32 - 0.5;
    let fx = x.fract().rem_euclid(1.0);
    let fy = y.fract().rem_euclid(1.0);
    let mut result = [0.0; 4];
    for (dy, wy) in [(0, 1.0 - fy), (1, fy)] {
        for (dx, wx) in [(0, 1.0 - fx), (1, fx)] {
            let px = (x.floor() as i32 + dx).clamp(0, image.width() as i32 - 1) as u32;
            let py = (y.floor() as i32 + dy).clamp(0, image.height() as i32 - 1) as u32;
            let pixel = image.get_pixel(px, py).0.map(|v| v as f32 / 255.0);
            let weight = wx * wy;
            for i in 0..3 {
                result[i] += pixel[i] * pixel[3] * weight;
            }
            result[3] += pixel[3] * weight;
        }
    }
    if result[3] > 0.0 {
        for i in 0..3 {
            result[i] /= result[3];
        }
    }
    result
}

pub fn mask_sample(mask: &GrayImage, unit: Point) -> f32 {
    if !(0.0..1.0).contains(&unit.x) || !(0.0..1.0).contains(&unit.y) {
        return 0.0;
    }
    let x = (unit.x * mask.width() as f32) as u32;
    let y = (unit.y * mask.height() as f32) as u32;
    mask.get_pixel(x.min(mask.width() - 1), y.min(mask.height() - 1))[0] as f32 / 255.0
}

pub fn own_mask(layer: &Layer, point: Point) -> f32 {
    layer
        .mask
        .as_ref()
        .filter(|m| m.enabled)
        .map_or(1.0, |mask| {
            mask_sample(
                &mask.pixels,
                mask.placement.unwrap_or(layer.transform).inverse(point),
            )
        })
}

pub fn layer_alpha(document: &Document, layer: &Layer, point: Point, depth: usize) -> f32 {
    if depth > 256 {
        return 0.0;
    }
    let mut alpha = layer.pixels.as_ref().map_or(0.0, |image| {
        sample(image, layer.transform.inverse(point))[3]
    });
    alpha *= layer.opacity * own_mask(layer, point);
    if let Some(source) = layer
        .clip_to
        .and_then(|id| document.layers.iter().find(|l| l.id == id))
    {
        alpha *= layer_alpha(document, source, point, depth + 1);
    }
    alpha
}

pub fn inherited_coverage(document: &Document, layer: &Layer, point: Point) -> f32 {
    if !layer.visible {
        return 0.0;
    }
    let mut coverage = 1.0;
    let mut parent = layer.parent;
    for _ in 0..64 {
        let Some(group) = parent.and_then(|id| document.layers.iter().find(|l| l.id == id)) else {
            break;
        };
        if !group.visible {
            return 0.0;
        }
        coverage *= own_mask(group, point) * group.opacity;
        parent = group.parent;
    }
    coverage
}

/// Depth-first bottom-to-top traversal keeps each folder's subtree together.
pub fn paint_order(document: &Document) -> Vec<&Layer> {
    fn visit<'a>(
        document: &'a Document,
        parent: Option<uuid::Uuid>,
        out: &mut Vec<&'a Layer>,
        depth: usize,
    ) {
        if depth > 64 {
            return;
        }
        for layer in document
            .layers
            .iter()
            .filter(|layer| layer.parent == parent)
        {
            if layer.group {
                visit(document, Some(layer.id), out, depth + 1);
            } else {
                out.push(layer);
            }
        }
    }
    let mut result = Vec::new();
    visit(document, None, &mut result, 0);
    result
}

pub fn render(document: &Document) -> RgbaImage {
    render_scaled(document, document.width, document.height)
}

pub fn render_scaled(document: &Document, width: u32, height: u32) -> RgbaImage {
    let mut output = RgbaImage::new(width, height);
    let layers = paint_order(document);
    output
        .as_mut()
        .par_chunks_exact_mut(4)
        .enumerate()
        .for_each(|(index, target)| {
            let x = index as u32 % width;
            let y = index as u32 / width;
            let point = Point::new(
                (x as f32 + 0.5) * document.width as f32 / width as f32,
                (y as f32 + 0.5) * document.height as f32 / height as f32,
            );
            let mut pixel = [0.0; 4];
            for layer in &layers {
                let coverage = inherited_coverage(document, layer, point);
                if coverage == 0.0 {
                    continue;
                }
                if let Some(adjustment) = &layer.adjustment {
                    let adjusted = crate::effects::adjust(pixel, adjustment, point);
                    let mut amount = coverage * layer.opacity * own_mask(layer, point);
                    if let Some(source) = layer
                        .clip_to
                        .and_then(|id| document.layers.iter().find(|l| l.id == id))
                    {
                        amount *= layer_alpha(document, source, point, 0);
                    }
                    for i in 0..3 {
                        pixel[i] += (adjusted[i] - pixel[i]) * amount;
                    }
                    continue;
                }
                if let Some(image) = &layer.pixels {
                    let mut source = sample(image, layer.transform.inverse(point));
                    source[3] = layer_alpha(document, layer, point, 0) * coverage;
                    pixel = composite(pixel, source, layer.blend);
                }
            }
            target.copy_from_slice(&pixel.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
        });
    output
}

pub fn hit_test(document: &Document, point: Point) -> Option<uuid::Uuid> {
    paint_order(document)
        .into_iter()
        .rev()
        .find(|layer| {
            !layer.locked
                && inherited_coverage(document, layer, point)
                    * layer_alpha(document, layer, point, 0)
                    > 0.05
        })
        .map(|l| l.id)
}

pub fn flatten_white(image: &RgbaImage) -> image::RgbImage {
    image::RgbImage::from_fn(image.width(), image.height(), |x, y| {
        let Rgba([r, g, b, a]) = *image.get_pixel(x, y);
        image::Rgb(
            [r, g, b]
                .map(|v| ((u32::from(v) * u32::from(a) + 255 * (255 - u32::from(a))) / 255) as u8),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::Mask;
    use std::sync::Arc;

    #[test]
    fn folder_visibility_mask_and_clipping_compose() {
        let mut doc = Document::new(2, 1).unwrap();
        doc.layers.clear();
        let base = Layer::image(
            "Base",
            RgbaImage::from_fn(2, 1, |x, _| Rgba([255, 0, 0, if x == 0 { 255 } else { 0 }])),
        );
        let mut group = Layer::blank("Folder", 2, 1);
        group.group = true;
        group.mask = Some(Mask {
            pixels: Arc::new(GrayImage::from_pixel(1, 1, image::Luma([128]))),
            ..Mask::white()
        });
        let mut top = Layer::image(
            "Clipped",
            RgbaImage::from_pixel(2, 1, Rgba([0, 0, 255, 255])),
        );
        top.clip_to = Some(base.id);
        top.parent = Some(group.id);
        doc.layers = vec![base, group, top];
        let image = render(&doc);
        assert_eq!(image.get_pixel(0, 0).0, [127, 0, 128, 255]);
        assert_eq!(image.get_pixel(1, 0).0, [0, 0, 0, 0]);
        doc.layers[1].visible = false;
        assert_eq!(render(&doc).get_pixel(0, 0).0, [255, 0, 0, 255]);
    }
}
