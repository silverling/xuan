use std::sync::Arc;

use anyhow::{Result, ensure};
use image::{Rgba, RgbaImage};
use rayon::prelude::*;

use crate::{
    document::{Adjustment, Document, Point},
    paint::ensure_pixels,
    render, selection,
};

pub fn rgb_to_hsl(c: [f32; 3]) -> [f32; 3] {
    let high = c.into_iter().fold(f32::MIN, f32::max);
    let low = c.into_iter().fold(f32::MAX, f32::min);
    let light = (high + low) * 0.5;
    let delta = high - low;
    if delta < 1e-6 {
        return [0.0, 0.0, light];
    }
    let sat = delta / (1.0 - (2.0 * light - 1.0).abs()).max(1e-6);
    let hue = if high == c[0] {
        (c[1] - c[2]) / delta
    } else if high == c[1] {
        (c[2] - c[0]) / delta + 2.0
    } else {
        (c[0] - c[1]) / delta + 4.0
    };
    [(hue * 60.0).rem_euclid(360.0), sat, light]
}

pub fn hsl_to_rgb(hsl: [f32; 3]) -> [f32; 3] {
    let [h, s, l] = hsl;
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let rgb = match h as u32 {
        0 => [c, x, 0.0],
        1 => [x, c, 0.0],
        2 => [0.0, c, x],
        3 => [0.0, x, c],
        4 => [x, 0.0, c],
        _ => [c, 0.0, x],
    };
    rgb.map(|v| v + l - c * 0.5)
}

pub fn curve_value(points: &[Point], value: f32) -> f32 {
    if points.len() < 2 {
        return value;
    }
    let i = points
        .partition_point(|p| p.x <= value)
        .saturating_sub(1)
        .min(points.len() - 2);
    let slope =
        |j: usize| (points[j + 1].y - points[j].y) / (points[j + 1].x - points[j].x).max(1e-6);
    let tangent = |j: usize| {
        if j == 0 {
            return slope(0);
        }
        if j == points.len() - 1 {
            return slope(j - 1);
        }
        let a = slope(j - 1);
        let b = slope(j);
        if a * b <= 0.0 {
            0.0
        } else {
            2.0 / (1.0 / a + 1.0 / b)
        }
    };
    let h = (points[i + 1].x - points[i].x).max(1e-6);
    let t = ((value - points[i].x) / h).clamp(0.0, 1.0);
    let t2 = t * t;
    let t3 = t2 * t;
    ((2.0 * t3 - 3.0 * t2 + 1.0) * points[i].y
        + (t3 - 2.0 * t2 + t) * h * tangent(i)
        + (-2.0 * t3 + 3.0 * t2) * points[i + 1].y
        + (t3 - t2) * h * tangent(i + 1))
    .clamp(0.0, 1.0)
}

fn noise(x: u32, y: u32, seed: u32) -> f32 {
    let mut value = x
        .wrapping_mul(374761393)
        .wrapping_add(y.wrapping_mul(668265263))
        .wrapping_add(seed);
    value = (value ^ (value >> 13)).wrapping_mul(1274126177);
    value ^= value >> 16;
    value as f32 / u32::MAX as f32 * 2.0 - 1.0
}

fn hue_saturation(rgb: [f32; 3], adjustment: [f32; 3], colorize: bool) -> [f32; 3] {
    let [hue, saturation, lightness] = adjustment;
    let mut hsl = rgb_to_hsl(rgb);
    hsl[0] = if colorize { hue } else { hsl[0] + hue };
    hsl[1] = if colorize {
        saturation / 100.0
    } else {
        hsl[1] * (1.0 + saturation / 100.0)
    }
    .clamp(0.0, 1.0);
    let amount = (lightness / 100.0).clamp(-1.0, 1.0);
    hsl[2] = if amount < 0.0 {
        hsl[2] * (1.0 + amount)
    } else {
        hsl[2] + (1.0 - hsl[2]) * amount
    };
    hsl_to_rgb(hsl)
}

fn mix32(mut value: u32) -> u32 {
    value = (value ^ (value >> 16)).wrapping_mul(0x7feb352d);
    value = (value ^ (value >> 15)).wrapping_mul(0x846ca68b);
    value ^ (value >> 16)
}

fn lattice(x: i32, y: i32, seed: u32) -> f32 {
    let hash = mix32(
        (x as u32).wrapping_mul(0x9e3779b1) ^ mix32((y as u32).wrapping_mul(0x85ebca77) ^ seed),
    );
    (hash & 65535) as f32 / 65535.0 + (hash >> 16) as f32 / 65535.0 - 1.0
}

fn film_grain(point: Point, size: f32, roughness: f32, seed: u32) -> f32 {
    let x = point.x / size;
    let y = point.y / size;
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let smoothstep = |v: f32| v * v * (3.0 - 2.0 * v);
    let tx = smoothstep(x - x.floor());
    let ty = smoothstep(y - y.floor());
    let top = lattice(ix, iy, seed) * (1.0 - tx) + lattice(ix + 1, iy, seed) * tx;
    let bottom = lattice(ix, iy + 1, seed) * (1.0 - tx) + lattice(ix + 1, iy + 1, seed) * tx;
    let smooth = (top * (1.0 - ty) + bottom * ty) * 1.6;
    let fine = lattice(
        point.x.floor() as i32,
        point.y.floor() as i32,
        mix32(seed ^ 0xa511e9b3),
    );
    smooth + (fine - smooth) * roughness / 100.0
}

pub fn adjust(pixel: [f32; 4], adjustment: &Adjustment, point: Point) -> [f32; 4] {
    let rgb = [pixel[0], pixel[1], pixel[2]];
    let rgb = match adjustment {
        Adjustment::HueRanges { settings } => {
            let response = settings.response(rgb_to_hsl(rgb)[0]);
            hue_saturation(rgb, response, settings.colorize)
        }
        Adjustment::LevelsChannels { ranges } => std::array::from_fn(|i| {
            crate::color::level(crate::color::level(rgb[i], ranges[i + 1]), ranges[0])
        }),
        Adjustment::CurvesChannels { channels } => std::array::from_fn(|i| {
            curve_value(&channels[0], curve_value(&channels[i + 1], rgb[i]))
        }),
        Adjustment::HueSaturation {
            hue,
            saturation,
            lightness,
            colorize,
        } => hue_saturation(rgb, [*hue, *saturation, *lightness], *colorize),
        Adjustment::Levels {
            black,
            gamma,
            white,
            output_black,
            output_white,
        } => rgb.map(|v| {
            let v = ((v * 255.0 - black) / (white - black).max(1.0))
                .clamp(0.0, 1.0)
                .powf(1.0 / gamma.max(0.01));
            (output_black + v * (output_white - output_black)) / 255.0
        }),
        Adjustment::Curves { points } => rgb.map(|v| curve_value(points, v)),
        Adjustment::Exposure {
            exposure,
            offset,
            gamma,
        } => rgb.map(|v| {
            crate::color::encode_srgb(
                (crate::color::decode_srgb(v) * 2.0_f32.powf(*exposure) + offset)
                    .max(0.0)
                    .powf(1.0 / gamma.max(0.01)),
            )
        }),
        Adjustment::GradientMap {
            shadows,
            highlights,
        } => {
            let luma = rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
            std::array::from_fn(|i| {
                (shadows[i] as f32 * (1.0 - luma) + highlights[i] as f32 * luma) / 255.0
            })
        }
        Adjustment::FilmGrain {
            amount,
            size,
            roughness,
            seed,
        } => {
            let level = rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722;
            let delta = film_grain(point, *size, *roughness, *seed) * amount / 100.0
                * 0.35
                * (0.4 + 2.4 * level * (1.0 - level));
            rgb.map(|v| v + delta)
        }
        Adjustment::Grain {
            amount,
            monochrome,
            seed,
        } => std::array::from_fn(|i| {
            rgb[i]
                + noise(
                    point.x as u32,
                    point.y as u32,
                    seed.wrapping_add(if *monochrome { 0 } else { i as u32 * 12345 }),
                ) * amount
                    / 100.0
        }),
        Adjustment::Invert => rgb.map(|v| 1.0 - v),
    };
    [
        rgb[0].clamp(0.0, 1.0),
        rgb[1].clamp(0.0, 1.0),
        rgb[2].clamp(0.0, 1.0),
        pixel[3],
    ]
}

pub fn apply_adjustment(
    document: &mut Document,
    adjustment: &Adjustment,
    mask_target: bool,
) -> Result<()> {
    let selection = document.selection.clone();
    let layer = document
        .active_mut()
        .ok_or_else(|| anyhow::anyhow!("Select a layer first"))?;
    if mask_target {
        crate::paint::prepare_mask(layer)?;
        let transform = layer
            .mask
            .as_ref()
            .and_then(|m| m.placement)
            .unwrap_or(layer.transform);
        let pixels = Arc::make_mut(&mut layer.mask.as_mut().unwrap().pixels);
        let (w, h) = pixels.dimensions();
        for (x, y, pixel) in pixels.enumerate_pixels_mut() {
            let point = transform.point(Point::new(
                (x as f32 + 0.5) / w as f32,
                (y as f32 + 0.5) / h as f32,
            ));
            let amount = selection::coverage(selection.as_deref(), point);
            let old = pixel[0] as f32 / 255.0;
            let new = adjust([old, old, old, 1.0], adjustment, point)[0];
            pixel[0] = ((old * (1.0 - amount) + new * amount) * 255.0).round() as u8;
        }
        return Ok(());
    }
    ensure_pixels(layer)?;
    let transform = layer.transform;
    let pixels = Arc::make_mut(layer.pixels.as_mut().unwrap());
    let (w, h) = pixels.dimensions();
    pixels
        .as_mut()
        .par_chunks_exact_mut(4)
        .enumerate()
        .for_each(|(index, pixel)| {
            let point = transform.point(Point::new(
                ((index as u32 % w) as f32 + 0.5) / w as f32,
                ((index as u32 / w) as f32 + 0.5) / h as f32,
            ));
            let amount = selection::coverage(selection.as_deref(), point);
            let old = [pixel[0], pixel[1], pixel[2], pixel[3]].map(|v| v as f32 / 255.0);
            let new = adjust(old, adjustment, point);
            for i in 0..3 {
                pixel[i] = ((old[i] * (1.0 - amount) + new[i] * amount) * 255.0).round() as u8;
            }
        });
    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
pub enum Filter {
    GaussianBlur { radius: f32 },
    MotionBlur { distance: f32, angle: f32 },
    Noise { amount: f32, monochrome: bool },
    LensCorrection { distortion: f32, vignette: f32 },
}

impl Filter {
    pub fn name(&self) -> &'static str {
        match self {
            Self::GaussianBlur { .. } => "Gaussian Blur",
            Self::MotionBlur { .. } => "Motion Blur",
            Self::Noise { .. } => "Add Noise",
            Self::LensCorrection { .. } => "Lens Correction",
        }
    }
}

pub fn filtered(image: &RgbaImage, filter: &Filter) -> RgbaImage {
    let (w, h) = image.dimensions();
    match filter {
        Filter::GaussianBlur { radius } => {
            // Blur premultiplied pixels to prevent dark fringes at transparent edges.
            let premul = RgbaImage::from_fn(w, h, |x, y| {
                let p = image.get_pixel(x, y).0;
                Rgba([
                    ((p[0] as u16 * p[3] as u16) / 255) as u8,
                    ((p[1] as u16 * p[3] as u16) / 255) as u8,
                    ((p[2] as u16 * p[3] as u16) / 255) as u8,
                    p[3],
                ])
            });
            let mut result = image::imageops::blur(&premul, radius.max(0.01));
            for pixel in result.pixels_mut() {
                if pixel[3] > 0 {
                    for i in 0..3 {
                        pixel[i] = ((pixel[i] as u32 * 255) / pixel[3] as u32).min(255) as u8;
                    }
                }
            }
            result
        }
        Filter::MotionBlur { distance, angle } => {
            let steps = distance.ceil().clamp(1.0, 256.0) as u32;
            let (sin, cos) = angle.to_radians().sin_cos();
            RgbaImage::from_fn(w, h, |x, y| {
                let mut sum = [0.0; 4];
                for i in 0..steps {
                    let offset = ((i as f32 + 0.5) / steps as f32 - 0.5) * distance;
                    let p = render::sample(
                        image,
                        Point::new(
                            (x as f32 + 0.5 + offset * cos) / w as f32,
                            (y as f32 + 0.5 + offset * sin) / h as f32,
                        ),
                    );
                    for c in 0..3 {
                        sum[c] += p[c] * p[3];
                    }
                    sum[3] += p[3];
                }
                if sum[3] > 0.0 {
                    for i in 0..3 {
                        sum[i] /= sum[3];
                    }
                }
                sum[3] /= steps as f32;
                Rgba(sum.map(|v| (v * 255.0).round() as u8))
            })
        }
        Filter::Noise { amount, monochrome } => RgbaImage::from_fn(w, h, |x, y| {
            Rgba(
                adjust(
                    image.get_pixel(x, y).0.map(|v| v as f32 / 255.0),
                    &Adjustment::Grain {
                        amount: *amount,
                        monochrome: *monochrome,
                        seed: 3187,
                    },
                    Point::new(x as f32, y as f32),
                )
                .map(|v| (v * 255.0).round() as u8),
            )
        }),
        Filter::LensCorrection {
            distortion,
            vignette,
        } => RgbaImage::from_fn(w, h, |x, y| {
            let u = (x as f32 + 0.5) / w as f32 * 2.0 - 1.0;
            let v = (y as f32 + 0.5) / h as f32 * 2.0 - 1.0;
            let radius = u * u + v * v;
            let k = 1.0 + distortion * radius / 100.0;
            let mut p = render::sample(image, Point::new((u * k + 1.0) * 0.5, (v * k + 1.0) * 0.5));
            for value in &mut p[..3] {
                *value *= 1.0 - vignette * radius * 0.005;
            }
            Rgba(p.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8))
        }),
    }
}

pub fn apply_filter(document: &mut Document, filter: &Filter, mask_target: bool) -> Result<()> {
    let selection = document.selection.clone();
    let layer = document
        .active_mut()
        .ok_or_else(|| anyhow::anyhow!("Select a layer first"))?;
    if mask_target {
        crate::paint::prepare_mask(layer)?;
        let mask = layer.mask.as_mut().unwrap();
        if let Filter::GaussianBlur { radius } = filter {
            let mut result = image::imageops::blur(&*mask.pixels, radius.max(0.01));
            let transform = mask.placement.unwrap_or(layer.transform);
            let (width, height) = result.dimensions();
            for (x, y, pixel) in result.enumerate_pixels_mut() {
                let point = transform.point(Point::new(
                    (x as f32 + 0.5) / width as f32,
                    (y as f32 + 0.5) / height as f32,
                ));
                let amount = selection::coverage(selection.as_deref(), point);
                pixel[0] = (mask.pixels.get_pixel(x, y)[0] as f32 * (1.0 - amount)
                    + pixel[0] as f32 * amount)
                    .round() as u8;
            }
            mask.pixels = Arc::new(result);
            return Ok(());
        }
        anyhow::bail!("Use Gaussian Blur on a mask");
    }
    ensure_pixels(layer)?;
    let original_transform = layer.transform;
    let original = layer.pixels.as_ref().unwrap();
    let padding = match filter {
        Filter::GaussianBlur { radius } => (radius * 3.0).ceil() as u32,
        Filter::MotionBlur { distance, .. } => (distance * 0.5).ceil() as u32 + 1,
        _ => 0,
    };
    let (w, h) = (
        original.width() + padding * 2,
        original.height() + padding * 2,
    );
    crate::document::validate_size(w, h)?;
    let mut expanded = RgbaImage::new(w, h);
    image::imageops::replace(&mut expanded, &**original, padding as i64, padding as i64);
    let mut transform = original_transform;
    if padding > 0 {
        let width = original.width() as f32;
        let height = original.height() as f32;
        let pad = padding as f32;
        let corners = [
            Point::new(-pad / width, -pad / height),
            Point::new(1.0 + pad / width, -pad / height),
            Point::new(1.0 + pad / width, 1.0 + pad / height),
            Point::new(-pad / width, 1.0 + pad / height),
        ]
        .map(|p| original_transform.point(p));
        let left = corners.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
        let top = corners.iter().map(|p| p.y).fold(f32::INFINITY, f32::min);
        let right = corners
            .iter()
            .map(|p| p.x)
            .fold(f32::NEG_INFINITY, f32::max);
        let bottom = corners
            .iter()
            .map(|p| p.y)
            .fold(f32::NEG_INFINITY, f32::max);
        transform = crate::document::Transform::new(1, 1);
        transform.x = left;
        transform.y = top;
        transform.width = right - left;
        transform.height = bottom - top;
        transform.warp = Some(corners.map(|p| {
            Point::new(
                (p.x - left) / transform.width,
                (p.y - top) / transform.height,
            )
        }));
    }
    let mut result = filtered(&expanded, filter);
    if selection.is_some() {
        for (x, y, pixel) in result.enumerate_pixels_mut() {
            let point = transform.point(Point::new(
                (x as f32 + 0.5) / w as f32,
                (y as f32 + 0.5) / h as f32,
            ));
            let amount = selection::coverage(selection.as_deref(), point);
            let old = expanded.get_pixel(x, y);
            for i in 0..4 {
                pixel[i] =
                    (old[i] as f32 * (1.0 - amount) + pixel[i] as f32 * amount).round() as u8;
            }
        }
    }
    if padding > 0
        && let Some(mask) = &mut layer.mask
    {
        mask.placement = Some(mask.placement.unwrap_or(original_transform));
    }
    layer.transform = transform;
    layer.pixels = Some(Arc::new(result));
    Ok(())
}

pub fn histogram(image: &RgbaImage) -> [u32; 256] {
    let mut bins = [0; 256];
    for pixel in image.pixels().filter(|p| p[3] != 0) {
        let value = (pixel[0] as f32 * 0.2126 + pixel[1] as f32 * 0.7152 + pixel[2] as f32 * 0.0722)
            .round() as usize;
        bins[value.min(255)] += 1;
    }
    bins
}

pub fn auto_levels(image: &RgbaImage) -> Adjustment {
    let bins = histogram(image);
    let total: u32 = bins.iter().sum();
    let cutoff = total / 200;
    let mut sum = 0;
    let black = bins
        .iter()
        .position(|n| {
            sum += n;
            sum > cutoff
        })
        .unwrap_or(0) as f32;
    sum = 0;
    let white = 255
        - bins
            .iter()
            .rev()
            .position(|n| {
                sum += n;
                sum > cutoff
            })
            .unwrap_or(0);
    Adjustment::Levels {
        black: black.min(254.0),
        white: (white as f32).max(black + 1.0),
        gamma: 1.0,
        output_black: 0.0,
        output_white: 255.0,
    }
}

pub fn validate_adjustment(adjustment: &Adjustment) -> Result<()> {
    let valid = match adjustment {
        Adjustment::HueRanges { settings } => settings.valid(),
        Adjustment::LevelsChannels { ranges } => ranges.iter().all(|r| {
            validate_adjustment(&Adjustment::Levels {
                black: r[0],
                gamma: r[1],
                white: r[2],
                output_black: r[3],
                output_white: r[4],
            })
            .is_ok()
        }),
        Adjustment::CurvesChannels { channels } => channels.iter().all(|points| {
            validate_adjustment(&Adjustment::Curves {
                points: points.clone(),
            })
            .is_ok()
        }),
        Adjustment::HueSaturation {
            hue,
            saturation,
            lightness,
            ..
        } => {
            hue.is_finite()
                && hue.abs() <= 360.0
                && saturation.is_finite()
                && saturation.abs() <= 100.0
                && lightness.is_finite()
                && lightness.abs() <= 100.0
        }
        Adjustment::Levels {
            black,
            gamma,
            white,
            output_black,
            output_white,
        } => {
            [black, gamma, white, output_black, output_white]
                .iter()
                .all(|v| v.is_finite())
                && *white > *black
                && *black >= 0.0
                && *white <= 255.0
                && (0.01..=10.0).contains(gamma)
                && (0.0..=255.0).contains(output_black)
                && (0.0..=255.0).contains(output_white)
        }
        Adjustment::Curves { points } => {
            (2..=32).contains(&points.len())
                && points.first().is_some_and(|p| p.x == 0.0)
                && points.last().is_some_and(|p| p.x == 1.0)
                && points
                    .iter()
                    .all(|p| p.x.is_finite() && p.y.is_finite() && (0.0..=1.0).contains(&p.y))
                && points.windows(2).all(|p| p[0].x < p[1].x)
        }
        Adjustment::Exposure {
            exposure,
            offset,
            gamma,
        } => {
            exposure.is_finite()
                && exposure.abs() <= 20.0
                && offset.is_finite()
                && offset.abs() <= 1.0
                && gamma.is_finite()
                && (0.01..=10.0).contains(gamma)
        }
        Adjustment::FilmGrain {
            amount,
            size,
            roughness,
            ..
        } => {
            amount.is_finite()
                && (0.0..=100.0).contains(amount)
                && size.is_finite()
                && (0.1..=100.0).contains(size)
                && roughness.is_finite()
                && (0.0..=100.0).contains(roughness)
        }
        Adjustment::Grain { amount, .. } => amount.is_finite() && (0.0..=100.0).contains(amount),
        Adjustment::GradientMap { .. } | Adjustment::Invert => true,
    };
    ensure!(valid, "Invalid adjustment settings");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blur_spreads_beyond_bounds_and_preserves_color() {
        let mut doc = Document::new(20, 20).unwrap();
        let mut layer = crate::document::Layer::image(
            "red",
            RgbaImage::from_pixel(4, 4, Rgba([255, 0, 0, 255])),
        );
        layer.transform.x = 8.0;
        layer.transform.y = 8.0;
        doc.insert(layer);
        apply_filter(&mut doc, &Filter::GaussianBlur { radius: 1.0 }, false).unwrap();
        let image = render::render(&doc);
        assert!(image.get_pixel(7, 9)[3] > 0);
        assert_eq!(image.get_pixel(7, 9)[0], 255);
        assert!(image.get_pixel(9, 9)[3] < 255);
        doc.validate().unwrap();
    }

    #[test]
    fn saturation_keeps_grays_neutral_and_exposure_uses_linear_light() {
        let saturated = adjust(
            [0.5, 0.5, 0.5, 1.0],
            &Adjustment::HueSaturation {
                hue: 60.0,
                saturation: 100.0,
                lightness: 0.0,
                colorize: false,
            },
            Point::default(),
        );
        assert_eq!(saturated, [0.5, 0.5, 0.5, 1.0]);
        let exposed = adjust(
            [0.5, 0.5, 0.5, 1.0],
            &Adjustment::Exposure {
                exposure: 1.0,
                offset: 0.0,
                gamma: 1.0,
            },
            Point::default(),
        );
        assert!((exposed[0] - 0.685_836).abs() < 0.00001);
    }

    #[test]
    fn identity_adjustments_and_hue_rotation() {
        let p = [0.2, 0.5, 0.8, 0.7];
        let a = Adjustment::HueSaturation {
            hue: 0.0,
            saturation: 0.0,
            lightness: 0.0,
            colorize: false,
        };
        let q = adjust(p, &a, Point::default());
        for i in 0..4 {
            assert!((p[i] - q[i]).abs() < 0.00001);
        }
        let green = adjust(
            [1.0, 0.0, 0.0, 1.0],
            &Adjustment::HueSaturation {
                hue: 120.0,
                saturation: 0.0,
                lightness: 0.0,
                colorize: false,
            },
            Point::default(),
        );
        assert!(green[0] < 0.001 && green[1] > 0.999 && green[2] < 0.001);
    }

    #[test]
    fn monotone_curves_do_not_overshoot() {
        let points = [
            Point::new(0.0, 0.0),
            Point::new(0.25, 0.7),
            Point::new(0.75, 0.7),
            Point::new(1.0, 1.0),
        ];
        for i in 25..75 {
            assert!((curve_value(&points, i as f32 / 100.0) - 0.7).abs() < 0.00001);
        }
    }
}
