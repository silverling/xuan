use std::{borrow::Cow, sync::Arc};

use image::{Rgba, RgbaImage};
use rayon::prelude::*;

use crate::{
    document::{Document, Layer, Point},
    effects,
};

use super::{own_mask, sample};

/// Resolve each image's children into a temporary raster. Source pixels and
/// effect settings in the project remain untouched. Document order is bottom-up.
pub(crate) fn prepare(document: &Document) -> Cow<'_, Document> {
    let owners: Vec<_> = document
        .layers
        .iter()
        .filter(|layer| {
            layer.can_attach_effects()
                && document
                    .layers
                    .iter()
                    .any(|child| child.parent == Some(layer.id))
        })
        .collect();
    if owners.is_empty() {
        return Cow::Borrowed(document);
    }
    let mut prepared = document.clone();
    for owner in owners {
        let mut raster = owner.clone();
        for effect in document
            .layers
            .iter()
            .filter(|child| child.parent == Some(owner.id) && child.visible && child.opacity > 0.0)
        {
            apply(document, &mut raster, effect);
        }
        if let Some(layer) = prepared.layers.iter_mut().find(|l| l.id == owner.id) {
            *layer = raster;
        }
        prepared.layers.retain(|l| l.parent != Some(owner.id));
    }
    Cow::Owned(prepared)
}

fn apply(document: &Document, raster: &mut Layer, effect: &Layer) {
    let Some(original) = raster.pixels.clone() else {
        return;
    };
    let transform = raster.transform;
    if effect.standalone_mask {
        if effect.mask.as_ref().is_none_or(|mask| !mask.enabled) {
            return;
        }
        let pixels = Arc::make_mut(raster.pixels.as_mut().unwrap());
        let (width, height) = pixels.dimensions();
        pixels
            .as_mut()
            .par_chunks_exact_mut(4)
            .enumerate()
            .for_each(|(index, pixel)| {
                let point = transform.point(Point::new(
                    ((index as u32 % width) as f32 + 0.5) / width as f32,
                    ((index as u32 / width) as f32 + 0.5) / height as f32,
                ));
                let amount = 1.0 - effect.opacity * (1.0 - own_mask(effect, point));
                pixel[3] = (pixel[3] as f32 * amount).round() as u8;
            });
        return;
    }

    let mut isolated = Document {
        layers: vec![raster.clone()],
        active: Some(raster.id),
        selection: None,
        ..document.clone()
    };
    let layer = &mut isolated.layers[0];
    layer.locked = false;
    layer.raw = None;
    layer.mask = None;
    let result = if let Some(filter) = &effect.filter {
        effects::apply_filter(&mut isolated, filter, false)
    } else if let Some(adjustment) = &effect.adjustment {
        effects::apply_adjustment(&mut isolated, adjustment, false)
    } else {
        return;
    };
    if result.is_err() {
        // A source at the document size limit may have no room for blur padding.
        // Still evaluate the filter within its original bounds in that case.
        if let Some(filter) = &effect.filter {
            isolated.layers[0].pixels = Some(Arc::new(effects::filtered(&original, filter)));
            isolated.layers[0].transform = transform;
        } else {
            return;
        }
    }
    let processed = &isolated.layers[0];
    let pixels = processed.pixels.as_ref().unwrap();
    if raster.transform != processed.transform
        && let Some(mask) = &mut raster.mask
    {
        mask.placement = Some(mask.placement.unwrap_or(raster.transform));
    }
    raster.transform = processed.transform;
    if effect.opacity == 1.0 && effect.mask.as_ref().is_none_or(|mask| !mask.enabled) {
        raster.pixels = Some(pixels.clone());
        return;
    }
    let (width, height) = pixels.dimensions();
    let mut mixed = RgbaImage::new(width, height);
    mixed
        .as_mut()
        .par_chunks_exact_mut(4)
        .enumerate()
        .for_each(|(index, target)| {
            let point = processed.transform.point(Point::new(
                ((index as u32 % width) as f32 + 0.5) / width as f32,
                ((index as u32 / width) as f32 + 0.5) / height as f32,
            ));
            let before = sample(&original, transform.inverse(point));
            let after = pixels
                .get_pixel(index as u32 % width, index as u32 / width)
                .0
                .map(|v| v as f32 / 255.0);
            let amount = effect.opacity * own_mask(effect, point);
            let pixel = mix(before, after, amount);
            target
                .copy_from_slice(&Rgba(pixel.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8)).0);
        });
    raster.pixels = Some(Arc::new(mixed));
}

pub(super) fn mix(before: [f32; 4], after: [f32; 4], amount: f32) -> [f32; 4] {
    let alpha = before[3] * (1.0 - amount) + after[3] * amount;
    let mut result = [0.0; 4];
    for i in 0..3 {
        result[i] = (before[i] * before[3] * (1.0 - amount) + after[i] * after[3] * amount)
            / alpha.max(0.000001);
    }
    result[3] = alpha;
    result
}
