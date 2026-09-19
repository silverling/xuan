use std::{collections::VecDeque, sync::Arc};

use image::{GrayImage, Luma, RgbaImage};

use crate::document::{Document, Point};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionMode {
    #[default]
    Replace,
    Add,
    Subtract,
    Intersect,
}

pub fn combine(document: &mut Document, mut incoming: GrayImage, mode: SelectionMode) {
    if mode != SelectionMode::Replace
        && let Some(old) = &document.selection
    {
        for (b, &a) in incoming.as_mut().iter_mut().zip(old.as_raw()) {
            *b = match mode {
                SelectionMode::Replace => *b,
                SelectionMode::Add => a.max(*b),
                SelectionMode::Subtract => a.saturating_sub(*b),
                SelectionMode::Intersect => a.min(*b),
            };
        }
    }
    document.selection = Some(Arc::new(incoming));
}

fn pixel_bound(coordinate: f32, limit: u32) -> u32 {
    (coordinate - 0.5).ceil().clamp(0.0, limit as f32) as u32
}

pub fn rectangle(width: u32, height: u32, start: Point, end: Point, ellipse: bool) -> GrayImage {
    let left = start.x.min(end.x);
    let top = start.y.min(end.y);
    let right = start.x.max(end.x);
    let bottom = start.y.max(end.y);
    let rx = ((right - left) * 0.5).max(0.5);
    let ry = ((bottom - top) * 0.5).max(0.5);
    let mut mask = GrayImage::new(width, height);
    let start = pixel_bound(left, width) as usize;
    let end = pixel_bound(right, width) as usize;
    for y in pixel_bound(top, height)..pixel_bound(bottom, height) {
        let row =
            &mut mask.as_mut()[y as usize * width as usize..(y + 1) as usize * width as usize];
        if ellipse {
            let py = y as f32 + 0.5;
            for (x, pixel) in row.iter_mut().enumerate().take(end).skip(start) {
                let px = x as f32 + 0.5;
                if ((px - left - rx) / rx).powi(2) + ((py - top - ry) / ry).powi(2) <= 1.0 {
                    *pixel = 255;
                }
            }
        } else {
            row[start..end].fill(255);
        }
    }
    mask
}

pub fn polygon(width: u32, height: u32, points: &[Point]) -> GrayImage {
    let mut mask = GrayImage::new(width, height);
    if points.len() < 3 {
        return mask;
    }
    let top = points.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
    let bottom = points.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max);
    let mut crossings = Vec::with_capacity(points.len());
    // Find edge intersections once per row, then fill spans using the even-odd
    // rule. Checking every edge for every image pixel makes long lassos stall.
    for y in pixel_bound(top, height)..pixel_bound(bottom, height) {
        let py = y as f32 + 0.5;
        crossings.clear();
        for i in 0..points.len() {
            let a = points[i];
            let b = points[(i + 1) % points.len()];
            if (a.y > py) != (b.y > py) {
                crossings.push((b.x - a.x) * (py - a.y) / (b.y - a.y) + a.x);
            }
        }
        crossings.sort_unstable_by(f32::total_cmp);
        let offset = y as usize * width as usize;
        for &[left, right] in crossings.as_chunks::<2>().0 {
            let start = offset + pixel_bound(left, width) as usize;
            let end = offset + pixel_bound(right, width) as usize;
            mask.as_mut()[start..end].fill(255);
        }
    }
    mask
}

pub fn wand(image: &RgbaImage, point: Point, tolerance: u8, contiguous: bool) -> GrayImage {
    let (width, height) = image.dimensions();
    let mut output = GrayImage::new(width, height);
    if point.x < 0.0 || point.y < 0.0 || point.x >= width as f32 || point.y >= height as f32 {
        return output;
    }
    let seed = (point.x as u32, point.y as u32);
    let color = image.get_pixel(seed.0, seed.1).0;
    let matches = |x, y| {
        let pixel = image.get_pixel(x, y).0;
        (color[3] == 0 && pixel[3] == 0)
            || pixel
                .iter()
                .zip(color)
                .all(|(a, b)| a.abs_diff(b) <= tolerance)
    };
    if !contiguous {
        if let Some(result) = crate::gpu::match_colors(image, color, tolerance) {
            return result;
        }
        for (x, y, pixel) in output.enumerate_pixels_mut() {
            if matches(x, y) {
                pixel[0] = 255;
            }
        }
        return output;
    }
    let mut visited = vec![false; width as usize * height as usize];
    let mut queue = VecDeque::from([seed]);
    visited[(seed.1 * width + seed.0) as usize] = true;
    while let Some((x, y)) = queue.pop_front() {
        if !matches(x, y) {
            continue;
        }
        output.put_pixel(x, y, Luma([255]));
        for (nx, ny) in [
            (x.wrapping_sub(1), y),
            (x + 1, y),
            (x, y.wrapping_sub(1)),
            (x, y + 1),
        ] {
            if nx >= width || ny >= height {
                continue;
            }
            let index = (ny * width + nx) as usize;
            if !visited[index] {
                visited[index] = true;
                queue.push_back((nx, ny));
            }
        }
    }
    output
}

pub fn coverage(selection: Option<&GrayImage>, point: Point) -> f32 {
    selection.map_or(1.0, |mask| {
        if point.x < 0.0
            || point.y < 0.0
            || point.x >= mask.width() as f32
            || point.y >= mask.height() as f32
        {
            0.0
        } else {
            mask.get_pixel(point.x as u32, point.y as u32)[0] as f32 / 255.0
        }
    })
}

pub fn translate(mask: &GrayImage, dx: i32, dy: i32) -> GrayImage {
    let (width, height) = mask.dimensions();
    let mut translated = GrayImage::new(width, height);
    let dx = i64::from(dx);
    let dy = i64::from(dy);
    let left = dx.max(0).min(i64::from(width));
    let right = (i64::from(width) + dx).clamp(left, i64::from(width));
    let top = dy.max(0).min(i64::from(height));
    let bottom = (i64::from(height) + dy).clamp(top, i64::from(height));
    if left == right {
        return translated;
    }
    let count = (right - left) as usize;
    for y in top..bottom {
        let dst = (y * i64::from(width) + left) as usize;
        let src = ((y - dy) * i64::from(width) + left - dx) as usize;
        translated.as_mut()[dst..dst + count].copy_from_slice(&mask.as_raw()[src..src + count]);
    }
    translated
}

pub fn bounds(mask: &GrayImage) -> Option<(u32, u32, u32, u32)> {
    let mut min_x = mask.width();
    let mut min_y = mask.height();
    let mut max_x = 0;
    let mut max_y = 0;
    for (x, y, p) in mask.enumerate_pixels() {
        if p[0] > 0 {
            min_x = min_x.min(x);
            min_y = min_y.min(y);
            max_x = max_x.max(x);
            max_y = max_y.max(y);
        }
    }
    (min_x <= max_x && min_y <= max_y).then_some((min_x, min_y, max_x + 1, max_y + 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn scanline_lasso_matches_pixel_reference_for_concave_and_crossing_paths() {
        for seed in 0..48 {
            let mut state = seed + 1_u32;
            let mut points: Vec<_> = (0..12)
                .map(|_| {
                    state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                    let x = (state % 96) as f32 * 0.5 - 8.0;
                    state = state.wrapping_mul(1664525).wrapping_add(1013904223);
                    let y = (state % 80) as f32 * 0.5 - 8.0;
                    Point::new(x, y)
                })
                .collect();
            // Repeated vertices and horizontal/vertical edges are valid lasso input.
            points.extend([points[0], points[0], Point::new(points[0].x, points[1].y)]);
            let expected = GrayImage::from_fn(32, 24, |x, y| {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let mut inside = false;
                for i in 0..points.len() {
                    let a = points[i];
                    let b = points[(i + 1) % points.len()];
                    if (a.y > py) != (b.y > py) && px < (b.x - a.x) * (py - a.y) / (b.y - a.y) + a.x
                    {
                        inside = !inside;
                    }
                }
                Luma([if inside { 255 } else { 0 }])
            });
            assert_eq!(polygon(32, 24, &points), expected, "path {seed}");
        }
        assert!(
            polygon(8, 8, &[Point::default(); 2])
                .as_raw()
                .iter()
                .all(|v| *v == 0)
        );
    }

    #[test]
    fn marquee_and_translation_preserve_pixel_center_and_clipping_rules() {
        for (start, end) in [
            (Point::new(-3.5, -2.0), Point::new(10.5, 8.0)),
            (Point::new(13.2, 11.5), Point::new(1.5, 2.5)),
            (Point::new(4.5, 5.5), Point::new(4.5, 5.5)),
            (Point::new(1.1, 2.2), Point::new(1.9, 2.8)),
        ] {
            for ellipse in [false, true] {
                let left = start.x.min(end.x);
                let top = start.y.min(end.y);
                let right = start.x.max(end.x);
                let bottom = start.y.max(end.y);
                let rx = ((right - left) * 0.5).max(0.5);
                let ry = ((bottom - top) * 0.5).max(0.5);
                let expected = GrayImage::from_fn(12, 10, |x, y| {
                    let px = x as f32 + 0.5;
                    let py = y as f32 + 0.5;
                    let inside = px >= left
                        && px < right
                        && py >= top
                        && py < bottom
                        && (!ellipse
                            || ((px - left - rx) / rx).powi(2) + ((py - top - ry) / ry).powi(2)
                                <= 1.0);
                    Luma([if inside { 255 } else { 0 }])
                });
                assert_eq!(rectangle(12, 10, start, end, ellipse), expected);
            }
        }
        let mask = GrayImage::from_fn(12, 10, |x, y| Luma([(x + y * 12) as u8]));
        for (dx, dy) in [
            (0, 0),
            (3, -4),
            (-5, 2),
            (12, 10),
            (-20, -30),
            (i32::MIN, i32::MAX),
        ] {
            let expected = GrayImage::from_fn(12, 10, |x, y| {
                let sx = i64::from(x) - i64::from(dx);
                let sy = i64::from(y) - i64::from(dy);
                if (0..12).contains(&sx) && (0..10).contains(&sy) {
                    *mask.get_pixel(sx as u32, sy as u32)
                } else {
                    Luma([0])
                }
            });
            assert_eq!(translate(&mask, dx, dy), expected);
        }
    }

    #[test]
    fn wand_respects_connectivity_and_transparent_rgb() {
        let image = RgbaImage::from_fn(3, 1, |x, _| {
            if x == 1 {
                Rgba([255, 0, 0, 255])
            } else {
                Rgba([x as u8, 0, 0, 0])
            }
        });
        assert_eq!(
            wand(&image, Point::new(0.0, 0.0), 0, true).as_raw(),
            &[255, 0, 0]
        );
        assert_eq!(
            wand(&image, Point::new(0.0, 0.0), 0, false).as_raw(),
            &[255, 0, 255]
        );
    }

    #[test]
    fn selection_subtract_and_polygon_bounds() {
        let mut document = Document::new(4, 4).unwrap();
        document.selection = Some(Arc::new(GrayImage::from_pixel(4, 4, Luma([255]))));
        combine(
            &mut document,
            rectangle(4, 4, Point::new(0.0, 0.0), Point::new(2.0, 4.0), false),
            SelectionMode::Subtract,
        );
        assert_eq!(
            bounds(document.selection.as_ref().unwrap()),
            Some((2, 0, 4, 4))
        );
        let polygon = polygon(
            4,
            4,
            &[
                Point::new(0.0, 0.0),
                Point::new(4.0, 0.0),
                Point::new(0.0, 4.0),
            ],
        );
        assert_eq!(polygon.get_pixel(0, 0)[0], 255);
        assert_eq!(polygon.get_pixel(3, 3)[0], 0);
    }
}
