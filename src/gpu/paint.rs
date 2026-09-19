use super::{
    processor::attempt,
    raster::{padded, transform_config},
};
use crate::document::{Adjustment, Document, Layer, Point, Transform};
use image::{GrayImage, RgbaImage};

const SHADER: &str = concat!(
    include_str!("buffers.wgsl"),
    include_str!("raster.wgsl"),
    include_str!("coordinates.wgsl"),
    include_str!("paint.wgsl")
);

/// Shared settings for fill, erase, gradients and selection-to-mask projection.
pub(crate) struct Paint {
    pub mode: u32,
    pub opacity: f32,
    pub colors: [[u8; 4]; 2],
    pub points: [Point; 2],
}

pub(crate) fn paint(
    layer: &mut Layer,
    selection: Option<&GrayImage>,
    mask_target: bool,
    settings: &Paint,
) -> bool {
    let (size, bytes, transform) = if mask_target {
        let mask = layer.mask.as_ref().unwrap();
        (
            [mask.pixels.width(), mask.pixels.height()],
            mask.pixels.as_raw(),
            mask.placement.unwrap_or(layer.transform),
        )
    } else {
        let pixels = layer.pixels.as_ref().unwrap();
        (
            [pixels.width(), pixels.height()],
            pixels.as_raw(),
            layer.transform,
        )
    };
    let result = attempt(u64::from(size[0]) * u64::from(size[1]), 65_536, |gpu| {
        let mut config = edit_config(size, transform, selection, mask_target);
        config.push([settings.mode as f32, settings.opacity, 0.0, 0.0]);
        config.extend(settings.colors.map(|color| color.map(|v| v as f32 / 255.0)));
        config.push([
            settings.points[0].x,
            settings.points[0].y,
            settings.points[1].x,
            settings.points[1].y,
        ]);
        gpu.simple(
            "paint_pixels",
            SHADER,
            &padded(bytes),
            &padded(selection.map_or(&[], |s| s.as_raw())),
            &config,
            size,
        )
    });
    if let Some(bytes) = result {
        if mask_target {
            layer.mask.as_mut().unwrap().pixels = std::sync::Arc::new(gray(size, bytes));
        } else {
            layer.pixels = Some(std::sync::Arc::new(
                RgbaImage::from_raw(size[0], size[1], bytes).unwrap(),
            ));
        }
        true
    } else {
        false
    }
}

pub(crate) fn adjust_mask(
    image: &GrayImage,
    adjustment: &Adjustment,
    transform: Transform,
    selection: Option<&GrayImage>,
) -> Option<GrayImage> {
    let size = [image.width(), image.height()];
    attempt(u64::from(size[0]) * u64::from(size[1]), 65_536, |gpu| {
        let mut layer = Layer::blank("Adjustment", size[0], size[1]);
        layer.adjustment = Some(adjustment.clone());
        let params = super::parameters(&Document::new(size[0], size[1])?, &layer, size);
        let mut config = edit_config(size, transform, selection, true);
        config.extend_from_slice(bytemuck::cast_slice(bytemuck::bytes_of(&params)));
        let bytes = gpu.simple(
            "adjust_pixels",
            super::raster::adjust_shader(),
            &padded(image.as_raw()),
            &padded(selection.map_or(&[], |s| s.as_raw())),
            &config,
            size,
        )?;
        Ok(gray(size, bytes))
    })
}

pub(crate) fn shape(
    size: [u32; 2],
    kind: crate::paint::ShapeKind,
    color: [u8; 4],
    radius: f32,
) -> Option<RgbaImage> {
    use crate::paint::ShapeKind;
    // Solid rectangles are a memory fill; curved boundaries benefit from compute.
    if kind == ShapeKind::Rectangle {
        return None;
    }
    attempt(u64::from(size[0]) * u64::from(size[1]), 65_536, |gpu| {
        let config = [
            [size[0] as f32, size[1] as f32, 0.0, 0.0],
            [
                if kind == ShapeKind::Ellipse { 1.0 } else { 2.0 },
                radius,
                0.0,
                0.0,
            ],
            color.map(|v| v as f32 / 255.0),
        ];
        let bytes = gpu.simple("shape_pixels", SHADER, &[], &[], &config, size)?;
        Ok(RgbaImage::from_raw(size[0], size[1], bytes).unwrap())
    })
}

pub(crate) fn match_colors(image: &RgbaImage, color: [u8; 4], tolerance: u8) -> Option<GrayImage> {
    let size = [image.width(), image.height()];
    attempt(u64::from(size[0]) * u64::from(size[1]), 65_536, |gpu| {
        let config = [
            [size[0] as f32, size[1] as f32, tolerance as f32, 0.0],
            color.map(|v| v as f32),
        ];
        let bytes = gpu.simple("match_colors", SHADER, image.as_raw(), &[], &config, size)?;
        Ok(gray(size, bytes))
    })
}

pub(super) fn gray(size: [u32; 2], bytes: Vec<u8>) -> GrayImage {
    GrayImage::from_raw(
        size[0],
        size[1],
        bytes.as_chunks::<4>().0.iter().map(|p| p[0]).collect(),
    )
    .unwrap()
}
pub(super) fn edit_config(
    size: [u32; 2],
    transform: Transform,
    selection: Option<&GrayImage>,
    mask: bool,
) -> Vec<[f32; 4]> {
    let mut config = vec![[size[0] as f32, size[1] as f32, 0.0, 0.0]];
    config.extend(transform_config(transform));
    config.push([
        selection.map_or(0, GrayImage::width) as f32,
        selection.map_or(0, GrayImage::height) as f32,
        0.0,
        if mask { 1.0 } else { 0.0 },
    ]);
    config
}

pub(crate) struct Stroke<'a> {
    pub bounds: [u32; 4],
    pub endpoints: [Point; 2],
    pub brush: &'a crate::paint::Brush,
    pub options: crate::paint::StrokeOptions<'a>,
}

pub(crate) fn stroke(layer: &mut Layer, selection: Option<&GrayImage>, stroke: Stroke<'_>) -> bool {
    use crate::paint::PaintMode;
    let [left, top, right, bottom] = stroke.bounds;
    if right <= left || bottom <= top {
        return false;
    }
    let size = [right - left, bottom - top];
    let mask = stroke.options.mask_target;
    let mode = match stroke.options.mode {
        PaintMode::Paint => 0,
        PaintMode::Erase => 1,
        PaintMode::Clone => 2,
        PaintMode::Blur => 3,
        PaintMode::Heal => 4,
        PaintMode::Smudge => 5,
    };
    let minimum = if mode == 3 || mode == 4 {
        16_384
    } else {
        65_536
    };
    let result = attempt(u64::from(size[0]) * u64::from(size[1]), minimum, |gpu| {
        let (bytes, full, transform) = if mask {
            let mask = layer.mask.as_ref().unwrap();
            (
                image::imageops::crop_imm(&*mask.pixels, left, top, size[0], size[1])
                    .to_image()
                    .into_raw(),
                [mask.pixels.width(), mask.pixels.height()],
                mask.placement.unwrap_or(layer.transform),
            )
        } else {
            let pixels = layer.pixels.as_ref().unwrap();
            (
                image::imageops::crop_imm(&**pixels, left, top, size[0], size[1])
                    .to_image()
                    .into_raw(),
                [pixels.width(), pixels.height()],
                layer.transform,
            )
        };
        let mut config = edit_config(size, transform, selection, mask);
        config[0][2] = left as f32;
        config[0][3] = top as f32;
        config.push([
            (stroke.brush.diameter * 0.5).max(0.5),
            stroke.brush.hardness,
            stroke.brush.opacity,
            mode as f32,
        ]);
        config.push(stroke.brush.color.map(|v| v as f32 / 255.0));
        config.push([0.0; 4]);
        config.push([
            stroke.endpoints[0].x,
            stroke.endpoints[0].y,
            stroke.endpoints[1].x,
            stroke.endpoints[1].y,
        ]);
        config.push([
            full[0] as f32,
            full[1] as f32,
            stroke.options.clone_offset.x,
            stroke.options.clone_offset.y,
        ]);
        let mut auxiliary = padded(selection.map_or(&[], |s| s.as_raw()));
        let source = stroke.options.source;
        config.push([
            source.map_or(0, RgbaImage::width) as f32,
            source.map_or(0, RgbaImage::height) as f32,
            f32::from_bits((auxiliary.len() / 4) as u32),
            0.0,
        ]);
        if let Some(source) = source {
            auxiliary.extend_from_slice(source.as_raw());
        }
        gpu.simple(
            "stroke_pixels",
            SHADER,
            &padded(&bytes),
            &auxiliary,
            &config,
            size,
        )
    });
    let Some(bytes) = result else {
        return false;
    };
    if mask {
        let pixels = std::sync::Arc::make_mut(&mut layer.mask.as_mut().unwrap().pixels);
        image::imageops::replace(pixels, &gray(size, bytes), left.into(), top.into());
    } else {
        let pixels = std::sync::Arc::make_mut(layer.pixels.as_mut().unwrap());
        image::imageops::replace(
            pixels,
            &RgbaImage::from_raw(size[0], size[1], bytes).unwrap(),
            left.into(),
            top.into(),
        );
    }
    true
}

pub(crate) fn project_selection(
    size: [u32; 2],
    transform: Transform,
    selection: Option<&GrayImage>,
) -> Option<GrayImage> {
    attempt(u64::from(size[0]) * u64::from(size[1]), 65_536, |gpu| {
        let mut config = edit_config(size, transform, selection, true);
        config.extend([[4.0, 1.0, 0.0, 0.0], [0.0; 4], [0.0; 4], [0.0; 4]]);
        let bytes = gpu.simple(
            "paint_pixels",
            SHADER,
            &[],
            &padded(selection.map_or(&[], |s| s.as_raw())),
            &config,
            size,
        )?;
        Ok(gray(size, bytes))
    })
}

pub(crate) struct FilterSelection<'a> {
    pub image: &'a [u8],
    pub original: &'a [u8],
    pub size: [u32; 2],
    pub original_size: [u32; 2],
    pub transform: Transform,
    pub selection: &'a GrayImage,
    pub padding: u32,
    pub mask: bool,
}
pub(crate) fn filter_selection(edit: FilterSelection<'_>) -> Option<Vec<u8>> {
    attempt(
        u64::from(edit.size[0]) * u64::from(edit.size[1]),
        65_536,
        |gpu| {
            let mut config =
                edit_config(edit.size, edit.transform, Some(edit.selection), edit.mask);
            let mut auxiliary = padded(edit.selection.as_raw());
            config.push([
                edit.original_size[0] as f32,
                edit.original_size[1] as f32,
                edit.padding as f32,
                f32::from_bits((auxiliary.len() / 4) as u32),
            ]);
            auxiliary.extend(padded(edit.original));
            let result = gpu.simple(
                "filter_selection",
                SHADER,
                &padded(edit.image),
                &auxiliary,
                &config,
                edit.size,
            )?;
            Ok(if edit.mask {
                result.as_chunks::<4>().0.iter().map(|p| p[0]).collect()
            } else {
                result
            })
        },
    )
}
