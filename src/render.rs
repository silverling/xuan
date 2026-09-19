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

/// Filter premultiplied colors so transparent edges never acquire dark halos.
pub fn resize_quality(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    if let Some(result) = crate::gpu::resize_rgba(image, width, height) {
        return result;
    }
    let premultiplied = image::ImageBuffer::from_fn(image.width(), image.height(), |x, y| {
        let p = image.get_pixel(x, y).0.map(|v| v as f32 / 255.0);
        Rgba([p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]])
    });
    let filtered = image::imageops::resize(
        &premultiplied,
        width,
        height,
        image::imageops::FilterType::Lanczos3,
    );
    RgbaImage::from_fn(width, height, |x, y| {
        let p = filtered.get_pixel(x, y).0;
        let alpha = p[3].clamp(0.0, 1.0);
        Rgba([
            (p[0] / alpha.max(0.00001) * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8,
            (p[1] / alpha.max(0.00001) * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8,
            (p[2] / alpha.max(0.00001) * 255.0)
                .round()
                .clamp(0.0, 255.0) as u8,
            (alpha * 255.0).round() as u8,
        ])
    })
}

pub fn source_size(document: &Document, layer: &Layer, size: [u32; 2]) -> [u32; 2] {
    let Some(image) = &layer.pixels else {
        return [1, 1];
    };
    let corners = layer.transform.corners().map(|p| {
        Point::new(
            p.x * size[0] as f32 / document.width as f32,
            p.y * size[1] as f32 / document.height as f32,
        )
    });
    let width = corners[0]
        .distance(corners[1])
        .max(corners[3].distance(corners[2]));
    let height = corners[0]
        .distance(corners[3])
        .max(corners[1].distance(corners[2]));
    let target = |length: f32, original: u32| {
        // Quantization keeps cached textures stable during small transform changes.
        let factor = (original as f32 / length.max(1.0)).log2().floor().max(0.0);
        ((original as f32 / 2.0_f32.powf(factor)) as u32).max(1)
    };
    [target(width, image.width()), target(height, image.height())]
}

pub fn render(document: &Document) -> RgbaImage {
    render_scaled(document, document.width, document.height)
}

pub fn render_scaled(document: &Document, width: u32, height: u32) -> RgbaImage {
    if let Some(result) = crate::gpu::compose(document, width, height) {
        return result;
    }
    let mut filtered = document.clone();
    for layer in &mut filtered.layers {
        if let Some(pixels) = &layer.pixels {
            let [w, h] = source_size(document, layer, [width, height]);
            if (w, h) != pixels.dimensions() {
                layer.pixels = Some(std::sync::Arc::new(resize_quality(pixels, w, h)));
            }
        }
    }
    render_pixels(&filtered, width, height)
}

/// Small UI thumbnails must not resize every full-resolution source on each edit.
/// Supersample the thumbnail itself; exports still use the filtered renderer above.
pub fn render_thumbnail(document: &Document, width: u32, height: u32) -> RgbaImage {
    resize_quality(
        &render_pixels(document, width * 2, height * 2),
        width,
        height,
    )
}

fn render_pixels(document: &Document, width: u32, height: u32) -> RgbaImage {
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
            let pixel = composite_at(document, &layers, point);
            target.copy_from_slice(&pixel.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8));
        });
    output
}

pub fn pixel_at(document: &Document, point: Point) -> [f32; 4] {
    composite_at(document, &paint_order(document), point)
}

fn composite_at(document: &Document, layers: &[&Layer], point: Point) -> [f32; 4] {
    let mut pixel = [0.0; 4];
    for layer in layers {
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
    pixel
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

/// Select by transformed layer bounds, ignoring pixel, mask, and opacity coverage.
pub fn hit_test_bounds(document: &Document, point: Point) -> Option<uuid::Uuid> {
    paint_order(document)
        .into_iter()
        .rev()
        .find(|layer| {
            if layer.locked || !layer.visible || layer.adjustment.is_some() {
                return false;
            }
            let unit = layer.transform.inverse(point);
            if !(0.0..1.0).contains(&unit.x) || !(0.0..1.0).contains(&unit.y) {
                return false;
            }
            let mut parent = layer.parent;
            for _ in 0..64 {
                let Some(group) = parent.and_then(|id| document.layers.iter().find(|l| l.id == id))
                else {
                    break;
                };
                if !group.visible {
                    return false;
                }
                parent = group.parent;
            }
            true
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
    fn bounds_hit_testing_follows_rotation_flips_and_perspective() {
        let mut document = Document::new(100, 100).unwrap();
        let bottom = document.layers[0].id;
        let mut top = Layer::image("Transparent", RgbaImage::new(40, 20));
        top.transform.x = 30.0;
        top.transform.y = 40.0;
        top.transform.rotation = 35.0;
        top.transform.flip_x = true;
        top.transform.warp = Some([
            Point::new(0.1, 0.1),
            Point::new(0.9, 0.0),
            Point::new(1.0, 1.0),
            Point::new(0.0, 0.9),
        ]);
        let id = top.id;
        let inside = top.transform.point(Point::new(0.25, 0.5));
        let outside = top.transform.point(Point::new(0.5, -0.2));
        document.insert(top);

        assert_eq!(hit_test(&document, inside), None);
        assert_eq!(hit_test_bounds(&document, inside), Some(id));
        assert_eq!(hit_test_bounds(&document, outside), Some(bottom));
        assert_eq!(hit_test_bounds(&document, Point::new(101.0, 50.0)), None);
    }

    #[test]
    fn bounds_hit_testing_respects_stacking_visibility_and_locks() {
        let mut document = Document::new(100, 100).unwrap();
        let bottom = document.layers[0].id;
        let mut folder = Layer::blank("Folder", 100, 100);
        folder.group = true;
        folder.opacity = 0.0;
        let mut top = Layer::blank("Masked", 100, 100);
        top.parent = Some(folder.id);
        top.opacity = 0.0;
        top.mask = Some(Mask {
            pixels: Arc::new(GrayImage::new(1, 1)),
            ..Mask::white()
        });
        top.clip_to = Some(bottom);
        let id = top.id;
        document.layers.extend([folder, top]);
        let point = Point::new(50.0, 50.0);

        assert_eq!(hit_test_bounds(&document, point), Some(id));
        document.layers[2].locked = true;
        assert_eq!(hit_test_bounds(&document, point), Some(bottom));
        document.layers[2].locked = false;
        document.layers[2].visible = false;
        assert_eq!(hit_test_bounds(&document, point), Some(bottom));
        document.layers[2].visible = true;
        document.layers[1].visible = false;
        assert_eq!(hit_test_bounds(&document, point), Some(bottom));
        document.layers[1].visible = true;
        document.layers[2].adjustment = Some(crate::document::Adjustment::Invert);
        assert_eq!(hit_test_bounds(&document, point), Some(bottom));
    }

    #[test]
    fn thumbnails_preserve_layer_placement_and_transparent_color() {
        let mut document = Document::new(64, 48).unwrap();
        let mut layer = Layer::image(
            "Thumbnail",
            RgbaImage::from_pixel(256, 128, Rgba([255, 0, 0, 128])),
        );
        layer.transform = crate::document::Transform::new(32, 16);
        layer.transform.x = 16.0;
        layer.transform.y = 16.0;
        document.layers = vec![layer];
        let thumbnail = render_thumbnail(&document, 32, 24);
        assert_eq!(thumbnail.dimensions(), (32, 24));
        assert_eq!(thumbnail.get_pixel(16, 12).0, [255, 0, 0, 128]);
        assert_eq!(thumbnail.get_pixel(0, 0)[3], 0);
        assert_eq!(thumbnail.get_pixel(31, 23)[3], 0);
        for pixel in thumbnail.pixels().filter(|p| p[3] > 0) {
            assert_eq!(&pixel.0[..3], &[255, 0, 0]);
        }
    }

    #[test]
    fn downsampling_filters_fine_detail_and_preserves_transparent_edge_color() {
        let checker = RgbaImage::from_fn(64, 64, |x, y| {
            let value = if (x + y) % 2 == 0 { 0 } else { 255 };
            Rgba([value, value, value, 255])
        });
        let mut doc = Document::new(4, 4).unwrap();
        let mut layer = Layer::image("checker", checker);
        layer.transform = crate::document::Transform::new(4, 4);
        doc.insert(layer);
        assert!(render(&doc).pixels().all(|p| (125..=130).contains(&p[0])));
        let edge = RgbaImage::from_fn(16, 16, |x, _| {
            if x < 8 {
                Rgba([255, 0, 0, 255])
            } else {
                Rgba([0; 4])
            }
        });
        let resized = resize_quality(&edge, 4, 4);
        for pixel in resized.pixels().filter(|p| p[3] > 0) {
            assert_eq!(pixel[0], 255);
        }
    }

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
