use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, ensure};
use image::{ImageBuffer, Primitive, Rgb32FImage, Rgba, RgbaImage};
use rayon::prelude::*;

use super::{DecodedRaw, DevelopSettings, Overlay, OverlayKind, WhiteBalance};
use crate::document::Point;

fn luminance(p: [f32; 3]) -> f32 {
    p[0] * 0.2126 + p[1] * 0.7152 + p[2] * 0.0722
}

fn matrix(m: [[f32; 3]; 3], p: [f32; 3]) -> [f32; 3] {
    m.map(|row| row[0] * p[0] + row[1] * p[1] + row[2] * p[2])
}

fn smooth(value: f32) -> f32 {
    let t = value.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Planckian-locus approximation in CIE xy, converted through the camera matrix.
fn temperature_wb(raw: &DecodedRaw, temperature: f32) -> [f32; 3] {
    let t = temperature.clamp(2000.0, 25_000.0);
    let x = if t <= 4000.0 {
        -0.2661239e9 / t.powi(3) - 0.2343589e6 / t.powi(2) + 0.8776956e3 / t + 0.179910
    } else {
        -3.0258469e9 / t.powi(3) + 2.107_038e6 / t.powi(2) + 0.2226347e3 / t + 0.240390
    };
    let y = if t <= 2222.0 {
        -1.1063814 * x.powi(3) - 1.3481102 * x.powi(2) + 2.1855583 * x - 0.20219683
    } else if t <= 4000.0 {
        -0.9549476 * x.powi(3) - 1.3741859 * x.powi(2) + 2.091_37 * x - 0.16748867
    } else {
        3.081758 * x.powi(3) - 5.8733864 * x.powi(2) + 3.7511299 * x - 0.37001483
    };
    let neutral = matrix(raw.xyz_to_camera, [x / y, 1.0, (1.0 - x - y) / y]);
    neutral.map(|v| (neutral[1] / v.max(0.001)).clamp(0.01, 100.0))
}

pub fn sample_white_balance(raw: &DecodedRaw, point: Point) -> [f32; 3] {
    let x = (point.x * raw.camera.width() as f32) as i32;
    let y = (point.y * raw.camera.height() as f32) as i32;
    let mut sum = [0.0; 3];
    for dy in -3..=3 {
        for dx in -3..=3 {
            let p = raw.camera.get_pixel(
                (x + dx).clamp(0, raw.camera.width() as i32 - 1) as u32,
                (y + dy).clamp(0, raw.camera.height() as i32 - 1) as u32,
            );
            for c in 0..3 {
                sum[c] += p[c];
            }
        }
    }
    sum.map(|v| (sum[1] / v.max(0.00001)).clamp(0.01, 100.0))
}

pub fn auto_exposure(raw: &DecodedRaw) -> f32 {
    let mut values: Vec<f32> = raw
        .camera
        .pixels()
        .step_by(16)
        .map(|p| {
            luminance(matrix(
                raw.camera_to_rgb,
                std::array::from_fn(|c| p[c] * raw.as_shot[c]),
            ))
            .max(0.00001)
        })
        .collect();
    if values.is_empty() {
        return 0.0;
    }
    values.sort_unstable_by(f32::total_cmp);
    let median = values[values.len() / 2];
    let high = values[(values.len() * 99 / 100).min(values.len() - 1)];
    (0.18 / median)
        .log2()
        .min((0.95 / high).log2() + 0.5)
        .clamp(-5.0, 5.0)
}

fn sample(image: &Rgb32FImage, x: f32, y: f32) -> [f32; 3] {
    let x = x.clamp(0.0, image.width().saturating_sub(1) as f32);
    let y = y.clamp(0.0, image.height().saturating_sub(1) as f32);
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (fx, fy) = (x.fract(), y.fract());
    let a = image.get_pixel(x0, y0);
    let b = image.get_pixel((x0 + 1).min(image.width() - 1), y0);
    let c = image.get_pixel(x0, (y0 + 1).min(image.height() - 1));
    let d = image.get_pixel(
        (x0 + 1).min(image.width() - 1),
        (y0 + 1).min(image.height() - 1),
    );
    std::array::from_fn(|i| {
        (a[i] * (1.0 - fx) + b[i] * fx) * (1.0 - fy) + (c[i] * (1.0 - fx) + d[i] * fx) * fy
    })
}

/// Inverse lens/geometry mapping, shared by rendering and the WB eyedropper.
pub fn source_point(point: Point, s: &DevelopSettings, aspect: f32) -> Point {
    let mut x = (point.x - 0.5) * 2.0;
    let mut y = (point.y - 0.5) * 2.0 / aspect;
    let (sin, cos) = s.rotation.to_radians().sin_cos();
    (x, y) = (cos * x + sin * y, -sin * x + cos * y);
    y *= aspect;
    let perspective = (1.0 + s.perspective[0] * x * 0.004 + s.perspective[1] * y * 0.004).max(0.2);
    x /= perspective;
    y /= perspective;
    let r2 = (x * x + y * y) * 0.5;
    let scale = 1.0 + s.distortion * 0.003 * r2;
    Point::new(0.5 + x * scale * 0.5, 0.5 + y * scale * 0.5)
}

fn overlay_weight(overlay: &Overlay, point: Point, aspect: f32) -> f32 {
    let dx = (overlay.end.x - overlay.start.x) * aspect;
    let dy = overlay.end.y - overlay.start.y;
    let px = (point.x - overlay.start.x) * aspect;
    let py = point.y - overlay.start.y;
    let value = match overlay.kind {
        OverlayKind::Linear => 1.0 - smooth((px * dx + py * dy) / (dx * dx + dy * dy).max(0.00001)),
        OverlayKind::Radial => {
            let distance =
                ((px / dx.abs().max(0.001)).powi(2) + (py / dy.abs().max(0.001)).powi(2)).sqrt();
            1.0 - smooth((distance - (1.0 - overlay.feather)) / overlay.feather)
        }
        OverlayKind::Brush => 0.0,
    };
    if overlay.invert { 1.0 - value } else { value }
}

fn brush_mask(overlay: &Overlay, width: u32, height: u32) -> Vec<f32> {
    let mut mask = vec![0.0_f32; width as usize * height as usize];
    let radius = overlay.radius * height as f32;
    for point in &overlay.points {
        let cx = point.x * width as f32;
        let cy = point.y * height as f32;
        for y in
            ((cy - radius).floor().max(0.0) as u32)..=((cy + radius).ceil() as u32).min(height - 1)
        {
            for x in ((cx - radius).floor().max(0.0) as u32)
                ..=((cx + radius).ceil() as u32).min(width - 1)
            {
                let distance = (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy) / radius;
                let value = 1.0 - smooth((distance - (1.0 - overlay.feather)) / overlay.feather);
                let pixel = &mut mask[(y * width + x) as usize];
                *pixel = pixel.max(value);
            }
        }
    }
    if overlay.invert {
        mask.iter_mut().for_each(|v| *v = 1.0 - *v);
    }
    mask
}

fn cancelled(cancel: &AtomicBool) -> Result<()> {
    ensure!(!cancel.load(Ordering::Relaxed), "RAW development cancelled");
    Ok(())
}

pub fn render(
    raw: &DecodedRaw,
    settings: &DevelopSettings,
    cancel: &AtomicBool,
) -> Result<RgbaImage> {
    if let Some(bytes) = accelerated(raw, settings, cancel, 8)? {
        let [l, t, r, b] =
            crate::gpu::raw_crop(settings, [raw.camera.width(), raw.camera.height()]);
        return Ok(RgbaImage::from_raw(r - l, b - t, bytes).unwrap());
    }
    render_at_depth(raw, settings, cancel, |v| (v * 255.0).round() as u8)
}

pub fn render_16(
    raw: &DecodedRaw,
    settings: &DevelopSettings,
    cancel: &AtomicBool,
) -> Result<ImageBuffer<Rgba<u16>, Vec<u16>>> {
    if let Some(bytes) = accelerated(raw, settings, cancel, 16)? {
        let [l, t, r, b] =
            crate::gpu::raw_crop(settings, [raw.camera.width(), raw.camera.height()]);
        let pixels = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| u16::from_le_bytes([p[0], p[1]]))
            .collect();
        return Ok(ImageBuffer::from_raw(r - l, b - t, pixels).unwrap());
    }
    render_at_depth(raw, settings, cancel, |v| (v * 65_535.0).round() as u16)
}

fn accelerated(
    raw: &DecodedRaw,
    settings: &DevelopSettings,
    cancel: &AtomicBool,
    depth: u32,
) -> Result<Option<Vec<u8>>> {
    settings.validate()?;
    cancelled(cancel)?;
    let mut wb = match settings.white_balance {
        WhiteBalance::AsShot => raw.as_shot,
        WhiteBalance::Temperature => temperature_wb(raw, settings.temperature),
        WhiteBalance::Custom => settings.custom_wb,
    };
    wb[1] *= 2.0_f32.powf(-settings.tint / 150.0);
    let result = crate::gpu::develop(raw, settings, wb, depth, cancel);
    cancelled(cancel)?;
    Ok(result)
}

fn render_at_depth<T: Primitive + Send + Sync>(
    raw: &DecodedRaw,
    settings: &DevelopSettings,
    cancel: &AtomicBool,
    encode: fn(f32) -> T,
) -> Result<ImageBuffer<Rgba<T>, Vec<T>>>
where
    Rgba<T>: image::Pixel<Subpixel = T>,
{
    settings.validate()?;
    cancelled(cancel)?;
    let s = settings;
    let (width, height) = raw.camera.dimensions();
    let aspect = width as f32 / height as f32;
    let mut wb = match s.white_balance {
        WhiteBalance::AsShot => raw.as_shot,
        WhiteBalance::Temperature => temperature_wb(raw, s.temperature),
        WhiteBalance::Custom => s.custom_wb,
    };
    wb[1] *= 2.0_f32.powf(-s.tint / 150.0);
    let exposure = 2.0_f32.powf(s.exposure);
    let mut pixels = vec![0.0_f32; width as usize * height as usize * 3];
    pixels
        .par_chunks_mut(width as usize * 3)
        .enumerate()
        .try_for_each(|(y, row)| -> Result<()> {
            cancelled(cancel)?;
            for (x, pixel) in row.as_chunks_mut::<3>().0.iter_mut().enumerate() {
                let point = Point::new(
                    (x as f32 + 0.5) / width as f32,
                    (y as f32 + 0.5) / height as f32,
                );
                let source = source_point(point, s, aspect);
                let mut camera = sample(
                    &raw.camera,
                    source.x * width as f32 - 0.5,
                    source.y * height as f32 - 0.5,
                );
                for (c, amount) in [(0, s.chromatic_red), (2, s.chromatic_blue)] {
                    if amount != 0.0 {
                        let scale = 1.0 + amount * 0.0002;
                        camera[c] = sample(
                            &raw.camera,
                            ((source.x - 0.5) * scale + 0.5) * width as f32 - 0.5,
                            ((source.y - 0.5) * scale + 0.5) * height as f32 - 0.5,
                        )[c];
                    }
                }
                let mut rgb = matrix(
                    raw.camera_to_rgb,
                    std::array::from_fn(|c| camera[c] * wb[c] * exposure),
                );
                let radial = ((point.x - 0.5).powi(2) + (point.y - 0.5).powi(2)) * 2.0;
                let gain = 2.0_f32.powf(s.vignette * 0.03 * radial.powi(2));
                rgb = rgb.map(|v| (v * gain).max(0.0));
                pixel.copy_from_slice(&rgb);
            }
            Ok(())
        })?;
    let mut image = Rgb32FImage::from_raw(width, height, pixels).unwrap();
    if s.luminance_noise > 0.0 || s.color_noise > 0.0 {
        image = denoise(&image, s, cancel)?;
    }
    for overlay in s.overlays.iter().filter(|o| o.enabled) {
        cancelled(cancel)?;
        let mask = (overlay.kind == OverlayKind::Brush).then(|| brush_mask(overlay, width, height));
        image
            .as_mut()
            .par_chunks_mut(width as usize * 3)
            .enumerate()
            .try_for_each(|(y, row)| -> Result<()> {
                cancelled(cancel)?;
                for (x, pixel) in row.as_chunks_mut::<3>().0.iter_mut().enumerate() {
                    let weight = mask.as_ref().map_or_else(
                        || {
                            overlay_weight(
                                overlay,
                                Point::new(
                                    (x as f32 + 0.5) / width as f32,
                                    (y as f32 + 0.5) / height as f32,
                                ),
                                aspect,
                            )
                        },
                        |mask| mask[y * width as usize + x],
                    );
                    let gain = 2.0_f32.powf(overlay.exposure * weight);
                    let warmth = 2.0_f32.powf(overlay.warmth * weight / 200.0);
                    let rgb = [
                        pixel[0] * gain * warmth,
                        pixel[1] * gain,
                        pixel[2] * gain / warmth,
                    ];
                    let luma = luminance(rgb);
                    for c in 0..3 {
                        pixel[c] = (luma
                            + (rgb[c] - luma) * (1.0 + overlay.saturation * weight / 100.0))
                            .max(0.0);
                    }
                }
                Ok(())
            })?;
    }
    // Tone mapping happens before the display transfer function and 8-bit conversion.
    image.as_mut().par_chunks_mut(3).for_each(|pixel| {
        let rgb = tone([pixel[0], pixel[1], pixel[2]], s);
        pixel.copy_from_slice(&rgb);
    });
    cancelled(cancel)?;
    let scale = width as f32 / raw.metadata.width as f32;
    for (amount, radius, threshold) in [
        (s.clarity / 100.0, 24.0 * scale, 0.0),
        (s.texture / 100.0, 3.0 * scale, 0.0),
        (
            s.sharpen / 100.0,
            s.sharpen_radius * scale,
            s.sharpen_threshold,
        ),
    ] {
        if amount != 0.0 {
            cancelled(cancel)?;
            let blurred = image::imageops::blur(&image, radius.max(0.3));
            image
                .as_mut()
                .par_chunks_mut(3)
                .zip(blurred.as_raw().par_chunks(3))
                .for_each(|(p, b)| {
                    let lum = luminance([p[0], p[1], p[2]]);
                    let delta = lum - luminance([b[0], b[1], b[2]]);
                    if delta.abs() >= threshold {
                        let gain = (lum + delta * amount).max(0.0) / lum.max(0.00001);
                        p.iter_mut().for_each(|v| *v *= gain);
                    }
                });
        }
    }
    cancelled(cancel)?;
    let left = (s.crop[0] * width as f32).floor() as u32;
    let top = (s.crop[1] * height as f32).floor() as u32;
    let right = (s.crop[2] * width as f32).ceil().min(width as f32) as u32;
    let bottom = (s.crop[3] * height as f32).ceil().min(height as f32) as u32;
    let mut output = ImageBuffer::<Rgba<T>, Vec<T>>::new(right - left, bottom - top);
    let out_width = output.width() as usize;
    output
        .as_mut()
        .par_chunks_mut(out_width * 4)
        .enumerate()
        .try_for_each(|(y, row)| -> Result<()> {
            cancelled(cancel)?;
            for (x, p) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
                let rgb = image.get_pixel(left + x as u32, top + y as u32);
                for c in 0..3 {
                    p[c] = encode(rgb[c].clamp(0.0, 1.0));
                }
                let point = source_point(
                    Point::new(
                        (left as f32 + x as f32 + 0.5) / width as f32,
                        (top as f32 + y as f32 + 0.5) / height as f32,
                    ),
                    s,
                    aspect,
                );
                p[3] = encode(
                    if (0.0..=1.0).contains(&point.x) && (0.0..=1.0).contains(&point.y) {
                        1.0
                    } else {
                        0.0
                    },
                );
            }
            Ok(())
        })?;
    Ok(output)
}

fn denoise(image: &Rgb32FImage, s: &DevelopSettings, cancel: &AtomicBool) -> Result<Rgb32FImage> {
    let mut output = image.clone();
    let width = image.width();
    output
        .as_mut()
        .par_chunks_mut(width as usize * 3)
        .enumerate()
        .try_for_each(|(y, row)| -> Result<()> {
            cancelled(cancel)?;
            for (x, pixel) in row.as_chunks_mut::<3>().0.iter_mut().enumerate() {
                let center = image.get_pixel(x as u32, y as u32).0;
                let lum = luminance(center);
                let mut sum = [0.0; 3];
                let mut weights = 0.0;
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let p = image
                            .get_pixel(
                                (x as i32 + dx).clamp(0, width as i32 - 1) as u32,
                                (y as i32 + dy).clamp(0, image.height() as i32 - 1) as u32,
                            )
                            .0;
                        let delta = (luminance(p) - lum) / (0.015 + lum * 0.12);
                        let weight = (-delta * delta).exp();
                        for c in 0..3 {
                            sum[c] += p[c] * weight;
                        }
                        weights += weight;
                    }
                }
                let mean = sum.map(|v| v / weights.max(0.00001));
                let mean_lum = luminance(mean);
                let target_lum = lum + (mean_lum - lum) * s.luminance_noise / 100.0;
                for c in 0..3 {
                    let chroma = (center[c] - lum) * (1.0 - s.color_noise / 100.0)
                        + (mean[c] - mean_lum) * s.color_noise / 100.0;
                    pixel[c] = (target_lum + chroma).max(0.0);
                }
            }
            Ok(())
        })?;
    Ok(output)
}

fn curve(value: f32, knots: &[f32; 5]) -> f32 {
    let x = value.clamp(0.0, 1.0) * 4.0;
    let i = (x as usize).min(3);
    knots[i] + (knots[i + 1] - knots[i]) * (x - i as f32)
}

fn to_hsl(rgb: [f32; 3]) -> [f32; 3] {
    let max = rgb.into_iter().fold(f32::NEG_INFINITY, f32::max);
    let min = rgb.into_iter().fold(f32::INFINITY, f32::min);
    let delta = max - min;
    let light = (max + min) * 0.5;
    if delta < 0.00001 {
        return [0.0, 0.0, light];
    }
    let hue = if max == rgb[0] {
        (rgb[1] - rgb[2]) / delta
    } else if max == rgb[1] {
        (rgb[2] - rgb[0]) / delta + 2.0
    } else {
        (rgb[0] - rgb[1]) / delta + 4.0
    };
    [
        (hue * 60.0).rem_euclid(360.0),
        delta / (1.0 - (2.0 * light - 1.0).abs()).max(0.00001),
        light,
    ]
}

fn from_hsl([h, s, l]: [f32; 3]) -> [f32; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h = h.rem_euclid(360.0) / 60.0;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let rgb = match h as usize {
        0 => [c, x, 0.0],
        1 => [x, c, 0.0],
        2 => [0.0, c, x],
        3 => [0.0, x, c],
        4 => [x, 0.0, c],
        _ => [c, 0.0, x],
    };
    rgb.map(|v| v + l - c * 0.5)
}

fn tone(mut rgb: [f32; 3], s: &DevelopSettings) -> [f32; 3] {
    let lum = luminance(rgb).max(0.00001);
    let shadow_weight = (1.0 - (lum / 0.5).clamp(0.0, 1.0)).powi(2);
    let highlight_weight = smooth((lum - 0.18) / 0.82);
    let gain = 2.0_f32.powf((s.shadows * shadow_weight + s.highlights * highlight_weight) / 50.0);
    rgb = rgb.map(|v| {
        let v = (v * gain * 2.0_f32.powf(s.whites / 100.0) + s.blacks / 1000.0).max(0.0);
        let v = ((v - s.dehaze / 1000.0) / (1.0 - s.dehaze / 500.0)).max(0.0);
        let v = v.powf(2.0_f32.powf(-s.brightness / 100.0));
        let v = if v <= 0.0031308 {
            v * 12.92
        } else {
            1.055 * v.powf(1.0 / 2.4) - 0.055
        };
        ((v - 0.5) * 2.0_f32.powf(s.contrast / 100.0) + 0.5).clamp(0.0, 1.0)
    });
    if s.defringe > 0.0 {
        let excess = ((rgb[0] + rgb[2]) * 0.5 - rgb[1]).max(0.0);
        let amount = excess * s.defringe / 100.0;
        rgb[0] -= amount;
        rgb[2] -= amount;
    }
    for (c, value) in rgb.iter_mut().enumerate() {
        *value = curve(curve(*value, &s.curves[0]), &s.curves[c + 1]);
    }
    let mut hsl = to_hsl(rgb);
    let mut change = [0.0; 3];
    for (i, band) in [0.0_f32, 30.0, 60.0, 120.0, 180.0, 240.0, 270.0, 300.0]
        .iter()
        .enumerate()
    {
        let distance = ((hsl[0] - band + 180.0).rem_euclid(360.0) - 180.0).abs();
        let weight = (1.0 - distance / 45.0).max(0.0);
        for (c, value) in change.iter_mut().enumerate() {
            *value += s.hsl[i][c] * weight;
        }
    }
    hsl[0] += change[0] * 0.3;
    hsl[1] = (hsl[1]
        * (1.0 + s.saturation / 100.0 + change[1] / 100.0)
        * (1.0 + s.vibrance / 100.0 * (1.0 - hsl[1])))
        .clamp(0.0, 1.0);
    hsl[2] = (hsl[2] + change[2] / 200.0).clamp(0.0, 1.0);
    rgb = from_hsl(hsl);
    if s.monochrome {
        rgb = [rgb
            .iter()
            .zip(s.bw_mix)
            .map(|(v, weight)| v * weight)
            .sum::<f32>()
            .clamp(0.0, 1.0); 3];
    }
    let high = smooth(luminance(rgb) + s.tone_balance / 200.0);
    for (tone, weight) in [(s.shadow_tone, 1.0 - high), (s.highlight_tone, high)] {
        let color = from_hsl([tone[0], 1.0, 0.5]);
        let amount = tone[1] / 100.0 * weight * 0.35;
        for c in 0..3 {
            rgb[c] = rgb[c] * (1.0 - amount) + color[c] * amount;
        }
    }
    rgb
}
