use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

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
        if let Some(result) = crate::gpu::adjust_mask(
            &layer.mask.as_ref().unwrap().pixels,
            adjustment,
            transform,
            selection.as_deref(),
        ) {
            layer.mask.as_mut().unwrap().pixels = Arc::new(result);
            return Ok(());
        }
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
    if let Some(result) = crate::gpu::adjustment(
        layer.pixels.as_ref().unwrap(),
        adjustment,
        transform,
        selection.as_deref(),
    ) {
        layer.pixels = Some(Arc::new(result));
        return Ok(());
    }
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

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Filter {
    GaussianBlur { radius: f32 },
    MotionBlur { distance: f32, angle: f32 },
    Noise { amount: f32, monochrome: bool },
    LensCorrection { distortion: f32, vignette: f32 },
}

impl Filter {
    pub fn validate(&self) -> Result<()> {
        let valid = match *self {
            Self::GaussianBlur { radius } => (0.0..=100.0).contains(&radius),
            Self::MotionBlur { distance, angle } => {
                (0.0..=200.0).contains(&distance) && (-180.0..=180.0).contains(&angle)
            }
            Self::Noise { amount, .. } => (0.0..=100.0).contains(&amount),
            Self::LensCorrection {
                distortion,
                vignette,
            } => (-50.0..=50.0).contains(&distortion) && (-100.0..=100.0).contains(&vignette),
        };
        ensure!(valid, "Invalid filter settings");
        Ok(())
    }

    pub fn scaled(&self, scale: f32) -> Self {
        match *self {
            Self::GaussianBlur { radius } => Self::GaussianBlur {
                radius: radius * scale,
            },
            Self::MotionBlur { distance, angle } => Self::MotionBlur {
                distance: distance * scale,
                angle,
            },
            _ => self.clone(),
        }
    }

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
    if let Some(result) = crate::gpu::filter(image, filter) {
        return result;
    }
    if crate::gpu::cancelled() {
        return image.clone();
    }
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
            motion_blur(image, *distance, *angle, &AtomicBool::new(false))
                .expect("Motion blur was not cancelled")
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

fn motion_blur(
    image: &RgbaImage,
    distance: f32,
    angle: f32,
    cancel: &AtomicBool,
) -> Result<RgbaImage> {
    ensure!(!cancel.load(Ordering::Relaxed), "Filter cancelled");
    let (width, height) = image.dimensions();
    let mut result = RgbaImage::new(width, height);
    if width == 0 || height == 0 {
        return Ok(result);
    }
    let steps = distance.ceil().clamp(1.0, 256.0) as u32;
    let (sin, cos) = angle.to_radians().sin_cos();
    // Translation gives every pixel the same bilinear weights. Compute them once,
    // and accumulate premultiplied colors without unpremultiplying every sample.
    let samples: Vec<_> = (0..steps)
        .map(|i| {
            let offset = ((i as f32 + 0.5) / steps as f32 - 0.5) * distance;
            let (x, y) = (offset * cos, offset * sin);
            let (fx, fy) = (x - x.floor(), y - y.floor());
            (
                x,
                y,
                x.floor() as i32,
                y.floor() as i32,
                [
                    (1.0 - fx) * (1.0 - fy),
                    fx * (1.0 - fy),
                    (1.0 - fx) * fy,
                    fx * fy,
                ],
            )
        })
        .collect();
    result
        .as_mut()
        .par_chunks_exact_mut(width as usize * 4)
        .enumerate()
        .try_for_each(|(y, row)| -> Result<()> {
            ensure!(!cancel.load(Ordering::Relaxed), "Filter cancelled");
            for (x, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let mut sum = [0.0; 4];
                for &(ox, oy, dx, dy, weights) in &samples {
                    let sx = x as f32 + 0.5 + ox;
                    let sy = y as f32 + 0.5 + oy;
                    if sx < 0.0 || sx >= width as f32 || sy < 0.0 || sy >= height as f32 {
                        continue;
                    }
                    for (i, weight) in weights.into_iter().enumerate() {
                        if weight == 0.0 {
                            continue;
                        }
                        let px = (x as i32 + dx + (i % 2) as i32).clamp(0, width as i32 - 1);
                        let py = (y as i32 + dy + (i / 2) as i32).clamp(0, height as i32 - 1);
                        let p = image.get_pixel(px as u32, py as u32);
                        let alpha = p[3] as f32 * weight;
                        for c in 0..3 {
                            sum[c] += p[c] as f32 * alpha;
                        }
                        sum[3] += alpha;
                    }
                }
                if sum[3] > 0.0 {
                    for c in 0..3 {
                        pixel[c] = (sum[c] / sum[3]).round() as u8;
                    }
                }
                pixel[3] = (sum[3] / steps as f32).round() as u8;
            }
            Ok(())
        })?;
    Ok(result)
}

pub fn apply_filter(document: &mut Document, filter: &Filter, mask_target: bool) -> Result<()> {
    apply_filter_cancellable(document, filter, mask_target, &AtomicBool::new(false))
}

pub fn apply_filter_cancellable(
    document: &mut Document,
    filter: &Filter,
    mask_target: bool,
    cancel: &AtomicBool,
) -> Result<()> {
    let processor = crate::gpu::current();
    let gpu = processor
        .as_ref()
        .filter(|_| {
            document
                .active()
                .and_then(|l| l.pixels.as_ref())
                .is_some_and(|p| u64::from(p.width()) * u64::from(p.height()) >= 16_384)
        })
        .map(|p| &p.motion_blur);
    apply_filter_impl(document, filter, mask_target, cancel, gpu)
}

/// Use the GPU for full-resolution Motion Blur, with CPU fallback for device
/// limits or GPU failures. Selection coverage and document edits are shared.
pub fn apply_filter_with_gpu(
    document: &mut Document,
    filter: &Filter,
    mask_target: bool,
    cancel: &AtomicBool,
    gpu: &crate::gpu::GpuMotionBlur,
) -> Result<()> {
    apply_filter_impl(document, filter, mask_target, cancel, Some(gpu))
}

fn apply_filter_impl(
    document: &mut Document,
    filter: &Filter,
    mask_target: bool,
    cancel: &AtomicBool,
    gpu: Option<&crate::gpu::GpuMotionBlur>,
) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "Filter cancelled");
    let selection = document.selection.clone();
    let layer = document
        .active_mut()
        .ok_or_else(|| anyhow::anyhow!("Select a layer first"))?;
    if mask_target {
        crate::paint::prepare_mask(layer)?;
        let mask = layer.mask.as_mut().unwrap();
        if let Filter::GaussianBlur { radius } = filter {
            let mut result = crate::gpu::blur_gray(&mask.pixels, radius.max(0.01));
            ensure!(!cancel.load(Ordering::Relaxed), "Filter cancelled");
            let transform = mask.placement.unwrap_or(layer.transform);
            let (width, height) = result.dimensions();
            if selection.is_none() {
                mask.pixels = Arc::new(result);
                return Ok(());
            }
            if let Some(bytes) = crate::gpu::filter_selection(crate::gpu::FilterSelection {
                image: result.as_raw(),
                original: mask.pixels.as_raw(),
                size: [width, height],
                original_size: [width, height],
                transform,
                selection: selection.as_deref().unwrap(),
                padding: 0,
                mask: true,
            }) {
                ensure!(!cancel.load(Ordering::Relaxed), "Filter cancelled");
                mask.pixels = Arc::new(image::GrayImage::from_raw(width, height, bytes).unwrap());
                return Ok(());
            }
            for (x, y, pixel) in result.enumerate_pixels_mut() {
                if x == 0 {
                    ensure!(!cancel.load(Ordering::Relaxed), "Filter cancelled");
                }
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
    let mut transform = original_transform;
    if padding > 0 {
        let width = original.width() as f32;
        let height = original.height() as f32;
        let pad = padding as f32;
        transform = original_transform.expanded(
            -pad / width,
            -pad / height,
            1.0 + pad / width,
            1.0 + pad / height,
        );
    }
    ensure!(!cancel.load(Ordering::Relaxed), "Filter cancelled");
    let accelerated = if let (Some(gpu), Filter::MotionBlur { distance, angle }) = (gpu, filter) {
        // WGPU can report allocation/validation failures through its panic handler.
        // An unsuccessful GPU attempt must not discard the user's pending edit.
        let attempt = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            gpu.render(original, *distance, *angle, padding, cancel)
        }))
        .unwrap_or_else(|_| Err(anyhow::anyhow!("GPU filter failed unexpectedly")));
        ensure!(!cancel.load(Ordering::Relaxed), "Filter cancelled");
        match attempt {
            Ok(result) => result,
            Err(error) => {
                eprintln!("GPU Motion Blur unavailable, using CPU: {error:#}");
                None
            }
        }
    } else {
        None
    };
    let mut result = if let Some(result) = accelerated {
        result
    } else {
        let mut expanded = RgbaImage::new(w, h);
        image::imageops::replace(&mut expanded, &**original, padding as i64, padding as i64);
        match filter {
            Filter::MotionBlur { distance, angle } => {
                motion_blur(&expanded, *distance, *angle, cancel)?
            }
            _ => filtered(&expanded, filter),
        }
    };
    ensure!(!cancel.load(Ordering::Relaxed), "Filter cancelled");
    let blended = selection.as_deref().and_then(|selection| {
        crate::gpu::filter_selection(crate::gpu::FilterSelection {
            image: result.as_raw(),
            original: original.as_raw(),
            size: [w, h],
            original_size: [original.width(), original.height()],
            transform,
            selection,
            padding,
            mask: false,
        })
    });
    ensure!(!cancel.load(Ordering::Relaxed), "Filter cancelled");
    if let Some(bytes) = blended {
        result = RgbaImage::from_raw(w, h, bytes).unwrap();
    } else if selection.is_some() {
        for (x, y, pixel) in result.enumerate_pixels_mut() {
            if x == 0 {
                ensure!(!cancel.load(Ordering::Relaxed), "Filter cancelled");
            }
            let point = transform.point(Point::new(
                (x as f32 + 0.5) / w as f32,
                (y as f32 + 0.5) / h as f32,
            ));
            let amount = selection::coverage(selection.as_deref(), point);
            let old = if (padding..padding + original.width()).contains(&x)
                && (padding..padding + original.height()).contains(&y)
            {
                original.get_pixel(x - padding, y - padding).0
            } else {
                [0; 4]
            };
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
    // This integer reduction is faster than a GPU upload/readback on CPU-owned
    // pixels. RAW preview combines RGB histograms and warnings in one GPU pass.
    let mut bins = [0; 256];
    for pixel in image.pixels().filter(|p| p[3] != 0) {
        // Integer weights make half-bin rounding identical on CPU and GPU.
        let value = (u32::from(pixel[0]) * 2126
            + u32::from(pixel[1]) * 7152
            + u32::from(pixel[2]) * 722
            + 5000) as usize
            / 10000;
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

    // The original implementation is an independent reference for sampling and alpha.
    fn reference_motion_blur(image: &RgbaImage, distance: f32, angle: f32) -> RgbaImage {
        let (w, h) = image.dimensions();
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
                for c in 0..3 {
                    sum[c] /= sum[3];
                }
            }
            sum[3] /= steps as f32;
            Rgba(sum.map(|v| (v * 255.0).round() as u8))
        })
    }

    #[test]
    fn motion_blur_matches_reference_at_edges_and_arbitrary_angles() {
        for (width, height) in [(31, 19), (1, 7), (7, 1), (0, 0)] {
            let image = RgbaImage::from_fn(width, height, |x, y| {
                Rgba([
                    (x * 67 + y * 41) as u8,
                    (x * 23 + y * 59) as u8,
                    (x * 13 + y * 17) as u8,
                    if (x + y) % 3 == 0 {
                        0
                    } else {
                        (x * 53 + y * 97) as u8
                    },
                ])
            });
            for distance in [1.0, 4.0, 20.0, 23.7, 200.0, 300.0] {
                for angle in [0.0, 90.0, -90.0, 180.0, 35.0, -35.0] {
                    let expected = reference_motion_blur(&image, distance, angle);
                    let actual = filtered(&image, &Filter::MotionBlur { distance, angle });
                    for (p, q) in actual.pixels().zip(expected.pixels()) {
                        for c in 0..4 {
                            // RGB is undefined when both results are fully transparent.
                            if c < 3 && p[3] == 0 && q[3] == 0 {
                                continue;
                            }
                            assert!(
                                p[c].abs_diff(q[c]) <= 1,
                                "{width}x{height}, distance {distance}, angle {angle}: {p:?} != {q:?}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn motion_blur_preserves_color_selection_and_mask_placement() {
        let mut doc = Document::new(20, 20).unwrap();
        let mut layer = crate::document::Layer::image(
            "red",
            RgbaImage::from_pixel(4, 4, Rgba([255, 0, 0, 255])),
        );
        layer.transform.x = 8.0;
        layer.transform.y = 8.0;
        layer.mask = Some(crate::document::Mask::white());
        let transform = layer.transform;
        doc.insert(layer);
        doc.selection = Some(Arc::new(image::GrayImage::from_fn(20, 20, |_, y| {
            image::Luma([if y >= 10 { 255 } else { 0 }])
        })));
        apply_filter(
            &mut doc,
            &Filter::MotionBlur {
                distance: 4.0,
                angle: 0.0,
            },
            false,
        )
        .unwrap();
        let layer = doc.active().unwrap();
        assert_eq!(layer.mask.as_ref().unwrap().placement, Some(transform));
        let pixels = layer.pixels.as_ref().unwrap();
        assert_eq!(pixels.dimensions(), (10, 10));
        assert_eq!(pixels.get_pixel(2, 4).0, [0; 4]);
        assert_eq!(pixels.get_pixel(3, 4).0, [255, 0, 0, 255]);
        assert!(pixels.get_pixel(2, 5)[3] > 0);
        assert_eq!(pixels.get_pixel(2, 5)[0], 255);
        doc.validate().unwrap();
    }

    #[test]
    fn cancelled_motion_blur_leaves_document_untouched() {
        let mut doc = Document::new(20, 20).unwrap();
        let original = doc.clone();
        assert!(
            apply_filter_cancellable(
                &mut doc,
                &Filter::MotionBlur {
                    distance: 200.0,
                    angle: 35.0
                },
                false,
                &AtomicBool::new(true),
            )
            .is_err()
        );
        assert_eq!(
            doc.active().unwrap().pixels,
            original.active().unwrap().pixels
        );
        assert_eq!(
            doc.active().unwrap().transform,
            original.active().unwrap().transform
        );
    }

    #[test]
    #[ignore = "manual performance comparison with the original motion blur"]
    fn motion_blur_benchmark() {
        let image = RgbaImage::from_fn(1024, 768, |x, y| {
            Rgba([x as u8, y as u8, (x + y) as u8, 255])
        });
        for (distance, angle) in [(20.0, 0.0), (200.0, 35.0)] {
            let start = std::time::Instant::now();
            let expected = reference_motion_blur(&image, distance, angle);
            let original = start.elapsed();
            let start = std::time::Instant::now();
            let actual = filtered(&image, &Filter::MotionBlur { distance, angle });
            let optimized = start.elapsed();
            assert!(
                actual
                    .as_raw()
                    .iter()
                    .zip(expected.as_raw())
                    .all(|(a, b)| a.abs_diff(*b) <= 1)
            );
            eprintln!(
                "1024x768, distance {distance}, angle {angle}: original {original:?}, optimized {optimized:?} ({:.1}x)",
                original.as_secs_f64() / optimized.as_secs_f64()
            );
        }
    }

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
