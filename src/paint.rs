use std::sync::Arc;

use anyhow::{Result, bail, ensure};
use image::{GrayImage, Luma, Rgba, RgbaImage};

use crate::{
    blend::{BlendMode, composite},
    document::{Document, Layer, Mask, Point, validate_size},
    render, selection,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintMode {
    Paint,
    Erase,
    Clone,
    Blur,
    Heal,
    Smudge,
}

#[derive(Clone, Debug)]
pub struct Brush {
    pub diameter: f32,
    pub hardness: f32,
    pub opacity: f32,
    pub color: [u8; 4],
}

impl Default for Brush {
    fn default() -> Self {
        Self {
            diameter: 40.0,
            hardness: 0.8,
            opacity: 1.0,
            color: [235, 235, 235, 255],
        }
    }
}

pub fn ensure_pixels(layer: &mut Layer) -> Result<()> {
    ensure!(
        !layer.locked && !layer.group && layer.adjustment.is_none(),
        "Select an unlocked pixel layer to paint"
    );
    if layer.pixels.is_none() {
        let width = layer.transform.width.round() as u32;
        let height = layer.transform.height.round() as u32;
        validate_size(width, height)?;
        layer.pixels = Some(Arc::new(RgbaImage::new(width, height)));
    }
    Ok(())
}

pub fn prepare_mask(layer: &mut Layer) -> Result<()> {
    ensure!(!layer.locked, "The layer is locked");
    let width = layer
        .pixels
        .as_ref()
        .map_or(layer.transform.width as u32, |p| p.width());
    let height = layer
        .pixels
        .as_ref()
        .map_or(layer.transform.height as u32, |p| p.height());
    validate_size(width, height)?;
    let mask = layer.mask.get_or_insert_with(Mask::white);
    if mask.pixels.dimensions() != (width, height) {
        mask.pixels = Arc::new(image::imageops::resize(
            &*mask.pixels,
            width,
            height,
            image::imageops::FilterType::Triangle,
        ));
    }
    Ok(())
}

/// A segment covers the complete swept brush, preventing gaps at fast pointer speeds.
/// Optional source pixels are in document coordinates (used by clone and retouch tools).
pub fn stroke(
    document: &mut Document,
    from: Point,
    to: Point,
    brush: &Brush,
    mode: PaintMode,
    mask_target: bool,
    source: Option<&RgbaImage>,
    clone_offset: Point,
) -> Result<()> {
    let selection = document.selection.clone();
    let Some(layer) = document.active_mut() else {
        bail!("Select a layer first");
    };
    if mask_target {
        prepare_mask(layer)?;
    } else {
        ensure_pixels(layer)?;
    }
    let transform = if mask_target {
        layer
            .mask
            .as_ref()
            .and_then(|m| m.placement)
            .unwrap_or(layer.transform)
    } else {
        layer.transform
    };
    let (width, height) = if mask_target {
        layer.mask.as_ref().unwrap().pixels.dimensions()
    } else {
        layer.pixels.as_ref().unwrap().dimensions()
    };
    let radius = (brush.diameter * 0.5).max(0.5);
    let min = Point::new(from.x.min(to.x) - radius, from.y.min(to.y) - radius);
    let max = Point::new(from.x.max(to.x) + radius, from.y.max(to.y) + radius);
    let local = [min, Point::new(max.x, min.y), max, Point::new(min.x, max.y)]
        .map(|p| transform.inverse(p));
    let left = (local.iter().map(|p| p.x).fold(f32::MAX, f32::min) * width as f32)
        .floor()
        .max(0.0) as u32;
    let right = (local.iter().map(|p| p.x).fold(f32::MIN, f32::max) * width as f32)
        .ceil()
        .min(width as f32) as u32;
    let top = (local.iter().map(|p| p.y).fold(f32::MAX, f32::min) * height as f32)
        .floor()
        .max(0.0) as u32;
    let bottom = (local.iter().map(|p| p.y).fold(f32::MIN, f32::max) * height as f32)
        .ceil()
        .min(height as f32) as u32;
    let dx = to.x - from.x;
    let dy = to.y - from.y;
    let length_sq = dx * dx + dy * dy;
    for y in top..bottom {
        for x in left..right {
            let point = transform.point(Point::new(
                (x as f32 + 0.5) / width as f32,
                (y as f32 + 0.5) / height as f32,
            ));
            let t = if length_sq < 0.0001 {
                0.0
            } else {
                (((point.x - from.x) * dx + (point.y - from.y) * dy) / length_sq).clamp(0.0, 1.0)
            };
            let distance = point.distance(Point::new(from.x + t * dx, from.y + t * dy)) / radius;
            if distance > 1.0 {
                continue;
            }
            let softness = if distance <= brush.hardness {
                1.0
            } else {
                (1.0 - distance) / (1.0 - brush.hardness).max(0.001)
            };
            let amount =
                softness * brush.opacity * selection::coverage(selection.as_deref(), point);
            if amount <= 0.0 {
                continue;
            }
            if mask_target {
                let pixels = Arc::make_mut(&mut layer.mask.as_mut().unwrap().pixels);
                let pixel = pixels.get_pixel_mut(x, y);
                let target = if mode == PaintMode::Erase {
                    0.0
                } else {
                    f32::from(brush.color[0]) * 0.3
                        + f32::from(brush.color[1]) * 0.59
                        + f32::from(brush.color[2]) * 0.11
                };
                pixel[0] = (f32::from(pixel[0]) * (1.0 - amount) + target * amount).round() as u8;
                continue;
            }
            let pixels = Arc::make_mut(layer.pixels.as_mut().unwrap());
            let pixel = pixels.get_pixel_mut(x, y);
            let old = pixel.0.map(|v| v as f32 / 255.0);
            let mut color = brush.color.map(|v| v as f32 / 255.0);
            match mode {
                PaintMode::Erase => {
                    pixel[3] = (old[3] * (1.0 - amount) * 255.0).round() as u8;
                    continue;
                }
                PaintMode::Clone | PaintMode::Smudge => {
                    if let Some(source) = source {
                        color = render::sample(
                            source,
                            Point::new(
                                (point.x + clone_offset.x) / source.width() as f32,
                                (point.y + clone_offset.y) / source.height() as f32,
                            ),
                        );
                    } else {
                        continue;
                    }
                }
                PaintMode::Blur | PaintMode::Heal => {
                    if let Some(source) = source {
                        color = [0.0; 4];
                        let step = if mode == PaintMode::Heal {
                            radius.max(2.0)
                        } else {
                            2.0
                        };
                        let mut weight = 0.0;
                        for sy in -1..=1 {
                            for sx in -1..=1 {
                                if mode == PaintMode::Heal && sx == 0 && sy == 0 {
                                    continue;
                                }
                                let sample = render::sample(
                                    source,
                                    Point::new(
                                        (point.x + sx as f32 * step) / source.width() as f32,
                                        (point.y + sy as f32 * step) / source.height() as f32,
                                    ),
                                );
                                for i in 0..4 {
                                    color[i] += sample[i];
                                }
                                weight += 1.0;
                            }
                        }
                        color = color.map(|v| v / weight);
                        // Retouching preserves alpha and interpolates the original color.
                        for i in 0..3 {
                            pixel[i] = ((old[i] * (1.0 - amount) + color[i] * amount) * 255.0)
                                .round() as u8;
                        }
                        continue;
                    } else {
                        continue;
                    }
                }
                PaintMode::Paint => {}
            }
            color[3] *= amount;
            pixel.0 = composite(old, color, BlendMode::Normal)
                .map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
        }
    }
    Ok(())
}

pub fn fill(document: &mut Document, color: [u8; 4], erase: bool, mask_target: bool) -> Result<()> {
    let selection = document.selection.clone();
    let layer = document
        .active_mut()
        .ok_or_else(|| anyhow::anyhow!("Select a layer first"))?;
    if mask_target {
        prepare_mask(layer)?;
    } else {
        ensure_pixels(layer)?;
    }
    let transform = if mask_target {
        layer
            .mask
            .as_ref()
            .and_then(|m| m.placement)
            .unwrap_or(layer.transform)
    } else {
        layer.transform
    };
    if mask_target {
        let pixels = Arc::make_mut(&mut layer.mask.as_mut().unwrap().pixels);
        let (w, h) = pixels.dimensions();
        for (x, y, pixel) in pixels.enumerate_pixels_mut() {
            let point = transform.point(Point::new(
                (x as f32 + 0.5) / w as f32,
                (y as f32 + 0.5) / h as f32,
            ));
            let amount = selection::coverage(selection.as_deref(), point);
            let target = if erase { 0 } else { color[0] };
            pixel[0] = (pixel[0] as f32 * (1.0 - amount) + target as f32 * amount).round() as u8;
        }
    } else {
        let pixels = Arc::make_mut(layer.pixels.as_mut().unwrap());
        let (w, h) = pixels.dimensions();
        for (x, y, pixel) in pixels.enumerate_pixels_mut() {
            let point = transform.point(Point::new(
                (x as f32 + 0.5) / w as f32,
                (y as f32 + 0.5) / h as f32,
            ));
            let amount = selection::coverage(selection.as_deref(), point);
            if erase {
                pixel[3] = (pixel[3] as f32 * (1.0 - amount)).round() as u8;
            } else {
                let mut source = color.map(|v| v as f32 / 255.0);
                source[3] *= amount;
                pixel.0 = composite(pixel.0.map(|v| v as f32 / 255.0), source, BlendMode::Normal)
                    .map(|v| (v * 255.0).round() as u8);
            }
        }
    }
    Ok(())
}

pub fn gradient(
    document: &mut Document,
    start: Point,
    end: Point,
    foreground: [u8; 4],
    background: [u8; 4],
    radial: bool,
    opacity: f32,
) -> Result<()> {
    let selection = document.selection.clone();
    let layer = document
        .active_mut()
        .ok_or_else(|| anyhow::anyhow!("Select a layer first"))?;
    ensure_pixels(layer)?;
    let transform = layer.transform;
    let pixels = Arc::make_mut(layer.pixels.as_mut().unwrap());
    let (w, h) = pixels.dimensions();
    let dx = end.x - start.x;
    let dy = end.y - start.y;
    let length_sq = (dx * dx + dy * dy).max(0.01);
    for (x, y, pixel) in pixels.enumerate_pixels_mut() {
        let point = transform.point(Point::new(
            (x as f32 + 0.5) / w as f32,
            (y as f32 + 0.5) / h as f32,
        ));
        let t = if radial {
            point.distance(start) / length_sq.sqrt()
        } else {
            ((point.x - start.x) * dx + (point.y - start.y) * dy) / length_sq
        };
        let t = t.clamp(0.0, 1.0);
        let mut color = std::array::from_fn(|i| {
            (foreground[i] as f32 * (1.0 - t) + background[i] as f32 * t) / 255.0
        });
        color[3] *= opacity * selection::coverage(selection.as_deref(), point);
        pixel.0 = composite(pixel.0.map(|v| v as f32 / 255.0), color, BlendMode::Normal)
            .map(|v| (v * 255.0).round() as u8);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShapeKind {
    Rectangle,
    Ellipse,
    RoundedRectangle,
}

pub fn shape(
    start: Point,
    end: Point,
    kind: ShapeKind,
    color: [u8; 4],
    corner_radius: f32,
) -> Result<Layer> {
    let width = (end.x - start.x).abs().round().max(1.0) as u32;
    let height = (end.y - start.y).abs().round().max(1.0) as u32;
    validate_size(width, height)?;
    let radius = corner_radius.min(width.min(height) as f32 * 0.5);
    let image = RgbaImage::from_fn(width, height, |x, y| {
        // Four subpixel samples produce antialiased shape edges.
        let mut coverage = 0.0;
        for oy in [0.25, 0.75] {
            for ox in [0.25, 0.75] {
                let px = x as f32 + ox;
                let py = y as f32 + oy;
                let inside = match kind {
                    ShapeKind::Rectangle => true,
                    ShapeKind::Ellipse => {
                        ((px / width as f32 - 0.5) * 2.0).powi(2)
                            + ((py / height as f32 - 0.5) * 2.0).powi(2)
                            <= 1.0
                    }
                    ShapeKind::RoundedRectangle => {
                        let cx = px.clamp(radius, width as f32 - radius);
                        let cy = py.clamp(radius, height as f32 - radius);
                        (px - cx).hypot(py - cy) <= radius
                    }
                };
                if inside {
                    coverage += 0.25;
                }
            }
        }
        Rgba([
            color[0],
            color[1],
            color[2],
            (color[3] as f32 * coverage).round() as u8,
        ])
    });
    let mut layer = Layer::image(
        match kind {
            ShapeKind::Rectangle => "Rectangle",
            ShapeKind::Ellipse => "Ellipse",
            ShapeKind::RoundedRectangle => "Rounded rectangle",
        },
        image,
    );
    layer.transform.x = start.x.min(end.x);
    layer.transform.y = start.y.min(end.y);
    Ok(layer)
}

pub fn mask_from_selection(document: &Document, layer: &Layer) -> GrayImage {
    let (w, h) = layer
        .pixels
        .as_ref()
        .map_or((document.width, document.height), |p| p.dimensions());
    GrayImage::from_fn(w, h, |x, y| {
        let point = layer.transform.point(Point::new(
            (x as f32 + 0.5) / w as f32,
            (y as f32 + 0.5) / h as f32,
        ));
        Luma([(selection::coverage(document.selection.as_deref(), point) * 255.0).round() as u8])
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fast_stroke_has_no_gaps_and_respects_selection() {
        let mut doc = Document::new(30, 10).unwrap();
        doc.selection = Some(Arc::new(selection::rectangle(
            30,
            10,
            Point::new(0.0, 0.0),
            Point::new(15.0, 10.0),
            false,
        )));
        let brush = Brush {
            diameter: 4.0,
            hardness: 1.0,
            color: [255, 0, 0, 255],
            ..Brush::default()
        };
        stroke(
            &mut doc,
            Point::new(1.0, 5.0),
            Point::new(28.0, 5.0),
            &brush,
            PaintMode::Paint,
            false,
            None,
            Point::default(),
        )
        .unwrap();
        let pixels = doc.active().unwrap().pixels.as_ref().unwrap();
        for x in 1..15 {
            assert_eq!(pixels.get_pixel(x, 5)[3], 255);
        }
        for x in 15..30 {
            assert_eq!(pixels.get_pixel(x, 5)[3], 0);
        }
    }

    #[test]
    fn painting_preserves_shared_history_pixels() {
        let mut doc = Document::new(8, 8).unwrap();
        fill(&mut doc, [255, 0, 0, 255], false, false).unwrap();
        let before = doc.clone();
        fill(&mut doc, [0, 0, 255, 255], false, false).unwrap();
        assert_eq!(
            before
                .active()
                .unwrap()
                .pixels
                .as_ref()
                .unwrap()
                .get_pixel(0, 0)
                .0,
            [255, 0, 0, 255]
        );
        assert_eq!(
            doc.active()
                .unwrap()
                .pixels
                .as_ref()
                .unwrap()
                .get_pixel(0, 0)
                .0,
            [0, 0, 255, 255]
        );
    }
}
