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

pub fn combine(document: &mut Document, incoming: GrayImage, mode: SelectionMode) {
    let mask = if let Some(old) = &document.selection {
        GrayImage::from_fn(document.width, document.height, |x, y| {
            let a = old.get_pixel(x, y)[0];
            let b = incoming.get_pixel(x, y)[0];
            Luma([match mode {
                SelectionMode::Replace => b,
                SelectionMode::Add => a.max(b),
                SelectionMode::Subtract => a.saturating_sub(b),
                SelectionMode::Intersect => a.min(b),
            }])
        })
    } else {
        incoming
    };
    document.selection = Some(Arc::new(mask));
}

pub fn rectangle(width: u32, height: u32, start: Point, end: Point, ellipse: bool) -> GrayImage {
    let left = start.x.min(end.x);
    let top = start.y.min(end.y);
    let right = start.x.max(end.x);
    let bottom = start.y.max(end.y);
    let rx = ((right - left) * 0.5).max(0.5);
    let ry = ((bottom - top) * 0.5).max(0.5);
    GrayImage::from_fn(width, height, |x, y| {
        let px = x as f32 + 0.5;
        let py = y as f32 + 0.5;
        let inside = px >= left
            && px < right
            && py >= top
            && py < bottom
            && (!ellipse
                || ((px - left - rx) / rx).powi(2) + ((py - top - ry) / ry).powi(2) <= 1.0);
        Luma([if inside { 255 } else { 0 }])
    })
}

pub fn polygon(width: u32, height: u32, points: &[Point]) -> GrayImage {
    GrayImage::from_fn(width, height, |x, y| {
        let px = x as f32 + 0.5;
        let py = y as f32 + 0.5;
        let mut inside = false;
        if points.len() >= 3 {
            for i in 0..points.len() {
                let a = points[i];
                let b = points[(i + 1) % points.len()];
                if (a.y > py) != (b.y > py) && px < (b.x - a.x) * (py - a.y) / (b.y - a.y) + a.x {
                    inside = !inside;
                }
            }
        }
        Luma([if inside { 255 } else { 0 }])
    })
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
    GrayImage::from_fn(mask.width(), mask.height(), |x, y| {
        let sx = x as i32 - dx;
        let sy = y as i32 - dy;
        if sx >= 0 && sy >= 0 && sx < mask.width() as i32 && sy < mask.height() as i32 {
            *mask.get_pixel(sx as u32, sy as u32)
        } else {
            Luma([0])
        }
    })
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
