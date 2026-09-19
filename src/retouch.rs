use std::{
    collections::{HashMap, VecDeque},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Result, bail, ensure};
use image::{GrayImage, Luma, RgbaImage};

use crate::{
    blend::BlendMode,
    document::{Document, Layer, Mask, Point, Transform},
    paint::Brush,
    render, selection,
};

fn random(state: &mut u32) -> u32 {
    *state ^= *state << 13;
    *state ^= *state >> 17;
    *state ^= *state << 5;
    *state
}

/// Fill selected pixels by matching nearby texture patches. Source candidates always
/// come from the original unselected image, so blemishes never propagate into the fill.
pub fn inpaint(image: &RgbaImage, mask: &GrayImage, cancel: &AtomicBool) -> Result<RgbaImage> {
    ensure!(
        image.dimensions() == mask.dimensions(),
        "Selection and image dimensions differ"
    );
    let (width, height) = image.dimensions();
    let index = |x: u32, y: u32| (y * width + x) as usize;
    let mut unknown: Vec<bool> = mask.pixels().map(|p| p[0] > 0).collect();
    // Reservoir sampling bounds the candidate pool independently of image size.
    let mut sources = Vec::with_capacity(65_536);
    let mut state = 0x7d41_397b;
    let mut seen = 0;
    for (x, y, pixel) in image.enumerate_pixels() {
        if !unknown[index(x, y)] && pixel[3] > 128 {
            seen += 1;
            if sources.len() < 65_536 {
                sources.push((x, y));
            } else {
                let slot = random(&mut state) as usize % seen;
                if slot < sources.len() {
                    sources[slot] = (x, y);
                }
            }
        }
    }
    ensure!(
        !sources.is_empty(),
        "Leave some opaque, unselected pixels to sample for Content-Aware Fill"
    );
    let mut queue = VecDeque::new();
    let mut queued = vec![false; unknown.len()];
    for y in 0..height {
        for x in 0..width {
            if !unknown[index(x, y)] {
                continue;
            }
            if neighbors(x, y, width, height).any(|(nx, ny)| !unknown[index(nx, ny)]) {
                queue.push_back((x, y));
                queued[index(x, y)] = true;
            }
        }
    }
    ensure!(
        !queue.is_empty(),
        "Select a region with unselected pixels around it"
    );
    let mut output = image.clone();
    let mut matches: HashMap<usize, (u32, u32)> = HashMap::new();
    while let Some((x, y)) = queue.pop_front() {
        if cancel.load(Ordering::Relaxed) {
            bail!("Cancelled");
        }
        let mut candidates = Vec::with_capacity(28);
        for (nx, ny) in neighbors(x, y, width, height) {
            if let Some(&(sx, sy)) = matches.get(&index(nx, ny)) {
                let sx = sx as i32 + x as i32 - nx as i32;
                let sy = sy as i32 + y as i32 - ny as i32;
                if sx >= 0 && sy >= 0 && sx < width as i32 && sy < height as i32 {
                    candidates.push((sx as u32, sy as u32));
                }
            } else if mask.get_pixel(nx, ny)[0] == 0 {
                candidates.push((nx, ny));
            }
        }
        for _ in 0..20 {
            candidates.push(sources[random(&mut state) as usize % sources.len()]);
        }
        let score = |sx: u32, sy: u32| -> f32 {
            if mask.get_pixel(sx, sy)[0] != 0 || image.get_pixel(sx, sy)[3] < 128 {
                return f32::MAX;
            }
            let mut error = 0.0;
            let mut count = 0;
            for oy in -2..=2 {
                for ox in -2..=2 {
                    let tx = x as i32 + ox;
                    let ty = y as i32 + oy;
                    let px = sx as i32 + ox;
                    let py = sy as i32 + oy;
                    if tx < 0
                        || ty < 0
                        || px < 0
                        || py < 0
                        || tx >= width as i32
                        || ty >= height as i32
                        || px >= width as i32
                        || py >= height as i32
                    {
                        continue;
                    }
                    if unknown[index(tx as u32, ty as u32)]
                        || mask.get_pixel(px as u32, py as u32)[0] != 0
                    {
                        continue;
                    }
                    let target = output.get_pixel(tx as u32, ty as u32);
                    let source = image.get_pixel(px as u32, py as u32);
                    if target[3] < 128 || source[3] < 128 {
                        continue;
                    }
                    for c in 0..3 {
                        error += (target[c] as f32 - source[c] as f32).powi(2);
                    }
                    count += 1;
                }
            }
            if count == 0 {
                1e8
            } else {
                error / count as f32
                    + ((sx as f32 - x as f32).abs() + (sy as f32 - y as f32).abs()) * 0.005
            }
        };
        let best = candidates
            .into_iter()
            .map(|candidate| (score(candidate.0, candidate.1), candidate))
            .min_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, candidate)| candidate)
            .unwrap_or(sources[0]);
        output.put_pixel(x, y, *image.get_pixel(best.0, best.1));
        unknown[index(x, y)] = false;
        matches.insert(index(x, y), best);
        for (nx, ny) in neighbors(x, y, width, height) {
            if unknown[index(nx, ny)] && !queued[index(nx, ny)] {
                queued[index(nx, ny)] = true;
                queue.push_back((nx, ny));
            }
        }
    }
    for (x, y, pixel) in output.enumerate_pixels_mut() {
        let amount = mask.get_pixel(x, y)[0] as f32 / 255.0;
        let original = image.get_pixel(x, y);
        for i in 0..4 {
            pixel[i] =
                (original[i] as f32 * (1.0 - amount) + pixel[i] as f32 * amount).round() as u8;
        }
    }
    Ok(output)
}

fn neighbors(x: u32, y: u32, width: u32, height: u32) -> impl Iterator<Item = (u32, u32)> {
    [
        (x.wrapping_sub(1), y),
        (x + 1, y),
        (x, y.wrapping_sub(1)),
        (x, y + 1),
    ]
    .into_iter()
    .filter(move |(x, y)| *x < width && *y < height)
}

fn raster_layer(document: &Document) -> Result<RgbaImage> {
    let layer = document
        .active()
        .ok_or_else(|| anyhow::anyhow!("Select a pixel layer"))?;
    ensure!(
        layer.raw.is_none(),
        "Rasterize the RAW layer before applying a pixel retouch operation"
    );
    ensure!(
        !layer.locked && !layer.group && layer.adjustment.is_none(),
        "Select an unlocked pixel layer"
    );
    let mut isolated = document.clone();
    isolated.layers = vec![Layer {
        parent: None,
        mask: None,
        clip_to: None,
        opacity: 1.0,
        blend: BlendMode::Normal,
        visible: true,
        ..layer.clone()
    }];
    Ok(render::render(&isolated))
}

fn replace_raster(document: &mut Document, pixels: RgbaImage) {
    let transform = Transform::new(document.width, document.height);
    if let Some(layer) = document.active_mut() {
        if let Some(mask) = &mut layer.mask {
            mask.placement = Some(mask.placement.unwrap_or(layer.transform));
            mask.linked = false;
        }
        layer.shape = None;
        layer.text = None;
        layer.pixels = Some(Arc::new(pixels));
        layer.transform = transform;
    }
}

pub fn content_aware_fill(document: &mut Document, cancel: &AtomicBool) -> Result<()> {
    let selection = document
        .selection
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Select the area to fill first"))?
        .clone();
    let source = raster_layer(document)?;
    let result = inpaint(&source, &selection, cancel)?;
    replace_raster(document, result);
    Ok(())
}

pub fn heal_path(
    document: &mut Document,
    points: &[Point],
    brush: &Brush,
    cancel: &AtomicBool,
) -> Result<()> {
    let mut mask = GrayImage::new(document.width, document.height);
    let radius = brush.diameter * 0.5;
    let mut path = points.to_vec();
    if path.len() == 1 {
        path.push(path[0]);
    }
    for segment in path.windows(2) {
        let [a, b] = [segment[0], segment[1]];
        let dx = b.x - a.x;
        let dy = b.y - a.y;
        let length = (dx * dx + dy * dy).max(0.001);
        let left = (a.x.min(b.x) - radius).floor().max(0.0) as u32;
        let right = (a.x.max(b.x) + radius).ceil().min(document.width as f32) as u32;
        let top = (a.y.min(b.y) - radius).floor().max(0.0) as u32;
        let bottom = (a.y.max(b.y) + radius).ceil().min(document.height as f32) as u32;
        for y in top..bottom {
            for x in left..right {
                let p = Point::new(x as f32 + 0.5, y as f32 + 0.5);
                let t = (((p.x - a.x) * dx + (p.y - a.y) * dy) / length).clamp(0.0, 1.0);
                let distance = p.distance(Point::new(a.x + t * dx, a.y + t * dy)) / radius.max(0.5);
                if distance <= 1.0 {
                    let amount = ((1.0 - distance) / (1.0 - brush.hardness).max(0.001)).min(1.0)
                        * brush.opacity
                        * selection::coverage(document.selection.as_deref(), p);
                    let old = mask.get_pixel(x, y)[0];
                    mask.put_pixel(x, y, Luma([old.max((amount * 255.0).round() as u8)]));
                }
            }
        }
    }
    let result = inpaint(&raster_layer(document)?, &mask, cancel)?;
    replace_raster(document, result);
    Ok(())
}

/// A portable edge-color matte. The seed color follows each edge region, retaining
/// foreground edges where the color distance crosses the threshold.
pub fn remove_background(
    document: &mut Document,
    tolerance: u8,
    cancel: &AtomicBool,
) -> Result<()> {
    let layer = document
        .active_mut()
        .ok_or_else(|| anyhow::anyhow!("Select an image layer"))?;
    ensure!(
        !layer.locked && !layer.group,
        "Select an unlocked image layer"
    );
    let image = layer
        .pixels
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("The selected layer is empty"))?;
    let (width, height) = image.dimensions();
    let mut matte = GrayImage::from_pixel(width, height, Luma([255]));
    let mut visited = vec![false; width as usize * height as usize];
    let mut queue = VecDeque::new();
    for x in 0..width {
        queue.push_back((x, 0, image.get_pixel(x, 0).0));
        queue.push_back((x, height - 1, image.get_pixel(x, height - 1).0));
    }
    for y in 0..height {
        queue.push_back((0, y, image.get_pixel(0, y).0));
        queue.push_back((width - 1, y, image.get_pixel(width - 1, y).0));
    }
    while let Some((x, y, seed)) = queue.pop_front() {
        if cancel.load(Ordering::Relaxed) {
            bail!("Cancelled");
        }
        let index = (y * width + x) as usize;
        if visited[index] {
            continue;
        }
        let pixel = image.get_pixel(x, y);
        if pixel[3] != 0
            && pixel.0[..3]
                .iter()
                .zip(seed)
                .any(|(a, b)| a.abs_diff(b) > tolerance)
        {
            continue;
        }
        visited[index] = true;
        matte.put_pixel(x, y, Luma([0]));
        for (nx, ny) in neighbors(x, y, width, height) {
            if !visited[(ny * width + nx) as usize] {
                queue.push_back((nx, ny, seed));
            }
        }
    }
    let mut matte = crate::gpu::blur_gray(&matte, 0.65);
    if let Some(result) = crate::gpu::bake_mask(layer, &matte) {
        matte = result;
    } else {
        for (x, y, pixel) in matte.enumerate_pixels_mut() {
            let point = layer.transform.point(Point::new(
                (x as f32 + 0.5) / width as f32,
                (y as f32 + 0.5) / height as f32,
            ));
            pixel[0] = (pixel[0] as f32 * render::own_mask(layer, point)).round() as u8;
        }
    }
    layer.mask = Some(Mask {
        pixels: Arc::new(matte),
        ..Mask::white()
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    #[test]
    fn fill_removes_a_spot_without_changing_unselected_pixels() {
        let image = RgbaImage::from_fn(16, 16, |x, y| {
            if (6..10).contains(&x) && (6..10).contains(&y) {
                Rgba([255, 0, 0, 255])
            } else {
                Rgba([80, 120, 160, 255])
            }
        });
        let mask =
            selection::rectangle(16, 16, Point::new(6.0, 6.0), Point::new(10.0, 10.0), false);
        let result = inpaint(&image, &mask, &AtomicBool::new(false)).unwrap();
        assert!(result.pixels().all(|p| p.0 == [80, 120, 160, 255]));
        assert!(
            inpaint(
                &image,
                &GrayImage::from_pixel(16, 16, Luma([255])),
                &AtomicBool::new(false)
            )
            .is_err()
        );
    }
    #[test]
    fn background_removal_creates_an_editable_mask() {
        let mut doc = Document::new(20, 20).unwrap();
        let image = RgbaImage::from_fn(20, 20, |x, y| {
            if (5..15).contains(&x) && (5..15).contains(&y) {
                Rgba([50, 80, 130, 255])
            } else {
                Rgba([255; 4])
            }
        });
        doc.layers[0].pixels = Some(Arc::new(image));
        remove_background(&mut doc, 20, &AtomicBool::new(false)).unwrap();
        let mask = &doc.layers[0].mask.as_ref().unwrap().pixels;
        assert_eq!(mask.get_pixel(0, 0)[0], 0);
        assert_eq!(mask.get_pixel(10, 10)[0], 255);
        assert_eq!(
            doc.layers[0].pixels.as_ref().unwrap().get_pixel(0, 0)[3],
            255
        );
    }
}
