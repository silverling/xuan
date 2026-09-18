use std::{collections::HashSet, sync::Arc};

use anyhow::{Result, ensure};
use image::{GrayImage, RgbaImage};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::blend::BlendMode;

pub const MAX_SIDE: u32 = 30_000;
pub const MAX_PIXELS: u64 = 100_000_000;
pub const MAX_LAYERS: usize = 10_000;

pub fn validate_size(width: u32, height: u32) -> Result<()> {
    ensure!(
        (1..=MAX_SIDE).contains(&width) && (1..=MAX_SIDE).contains(&height),
        "Dimensions must be between 1 and {MAX_SIDE} pixels"
    );
    ensure!(
        u64::from(width) * u64::from(height) <= MAX_PIXELS,
        "Images are limited to 100 megapixels"
    );
    Ok(())
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    pub fn distance(self, other: Self) -> f32 {
        (self.x - other.x).hypot(self.y - other.y)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub rotation: f32,
    pub flip_x: bool,
    pub flip_y: bool,
}

impl Transform {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: width as f32,
            height: height as f32,
            rotation: 0.0,
            flip_x: false,
            flip_y: false,
        }
    }

    pub fn center(self) -> Point {
        Point::new(self.x + self.width * 0.5, self.y + self.height * 0.5)
    }

    /// Map normalized source coordinates to document coordinates.
    pub fn point(self, unit: Point) -> Point {
        let x = (if self.flip_x { 1.0 - unit.x } else { unit.x } - 0.5) * self.width;
        let y = (if self.flip_y { 1.0 - unit.y } else { unit.y } - 0.5) * self.height;
        let (sin, cos) = self.rotation.to_radians().sin_cos();
        let center = self.center();
        Point::new(center.x + cos * x - sin * y, center.y + sin * x + cos * y)
    }

    pub fn inverse(self, point: Point) -> Point {
        let center = self.center();
        let x = point.x - center.x;
        let y = point.y - center.y;
        let (sin, cos) = self.rotation.to_radians().sin_cos();
        let mut u = (cos * x + sin * y) / self.width + 0.5;
        let mut v = (-sin * x + cos * y) / self.height + 0.5;
        if self.flip_x {
            u = 1.0 - u;
        }
        if self.flip_y {
            v = 1.0 - v;
        }
        Point::new(u, v)
    }

    pub fn corners(self) -> [Point; 4] {
        [
            Point::new(0.0, 0.0),
            Point::new(1.0, 0.0),
            Point::new(1.0, 1.0),
            Point::new(0.0, 1.0),
        ]
        .map(|point| self.point(point))
    }

    pub fn valid(self) -> bool {
        [self.x, self.y, self.width, self.height, self.rotation]
            .iter()
            .all(|x| x.is_finite())
            && (1.0..=300_000.0).contains(&self.width)
            && (1.0..=300_000.0).contains(&self.height)
            && self.x.abs() <= 1_000_000.0
            && self.y.abs() <= 1_000_000.0
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Mask {
    #[serde(skip)]
    pub pixels: Arc<GrayImage>,
    pub enabled: bool,
    pub linked: bool,
    pub placement: Option<Transform>,
}

impl Mask {
    pub fn white() -> Self {
        Self {
            pixels: Arc::new(GrayImage::from_pixel(1, 1, image::Luma([255]))),
            enabled: true,
            linked: true,
            placement: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Adjustment {
    HueSaturation {
        hue: f32,
        saturation: f32,
        lightness: f32,
        colorize: bool,
    },
    Levels {
        black: f32,
        gamma: f32,
        white: f32,
        output_black: f32,
        output_white: f32,
    },
    Curves {
        points: Vec<Point>,
    },
    Exposure {
        exposure: f32,
        offset: f32,
        gamma: f32,
    },
    GradientMap {
        shadows: [u8; 4],
        highlights: [u8; 4],
    },
    Grain {
        amount: f32,
        monochrome: bool,
        seed: u32,
    },
    Invert,
}

impl Adjustment {
    pub fn name(&self) -> &'static str {
        match self {
            Self::HueSaturation { .. } => "Hue/Saturation",
            Self::Levels { .. } => "Levels",
            Self::Curves { .. } => "Curves",
            Self::Exposure { .. } => "Exposure",
            Self::GradientMap { .. } => "Gradient Map",
            Self::Grain { .. } => "Grain",
            Self::Invert => "Invert",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Layer {
    pub id: Uuid,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    pub opacity: f32,
    pub blend: BlendMode,
    pub transform: Transform,
    pub parent: Option<Uuid>,
    pub group: bool,
    pub clip_to: Option<Uuid>,
    pub mask: Option<Mask>,
    pub adjustment: Option<Adjustment>,
    #[serde(skip)]
    pub pixels: Option<Arc<RgbaImage>>,
}

impl Layer {
    pub fn blank(name: impl Into<String>, width: u32, height: u32) -> Self {
        Self {
            id: Uuid::new_v4(),
            name: name.into(),
            visible: true,
            locked: false,
            opacity: 1.0,
            blend: BlendMode::Normal,
            transform: Transform::new(width, height),
            parent: None,
            group: false,
            clip_to: None,
            mask: None,
            adjustment: None,
            pixels: None,
        }
    }

    pub fn image(name: impl Into<String>, pixels: RgbaImage) -> Self {
        let mut layer = Self::blank(name, pixels.width(), pixels.height());
        layer.pixels = Some(Arc::new(pixels));
        layer
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Document {
    pub id: Uuid,
    pub width: u32,
    pub height: u32,
    pub resolution: f32,
    pub layers: Vec<Layer>,
    pub active: Option<Uuid>,
    #[serde(skip)]
    pub selected: HashSet<Uuid>,
    #[serde(skip)]
    pub selection: Option<Arc<GrayImage>>,
}

impl Document {
    pub fn new(width: u32, height: u32) -> Result<Self> {
        validate_size(width, height)?;
        let layer = Layer::blank("Layer 1", width, height);
        Ok(Self {
            id: Uuid::new_v4(),
            width,
            height,
            resolution: 72.0,
            active: Some(layer.id),
            selected: HashSet::from([layer.id]),
            layers: vec![layer],
            selection: None,
        })
    }

    pub fn active(&self) -> Option<&Layer> {
        self.layers
            .iter()
            .find(|layer| Some(layer.id) == self.active)
    }

    pub fn active_mut(&mut self) -> Option<&mut Layer> {
        self.layers
            .iter_mut()
            .find(|layer| Some(layer.id) == self.active)
    }

    pub fn select(&mut self, id: Uuid, extend: bool) {
        if !extend {
            self.selected.clear();
        }
        if extend && self.selected.contains(&id) {
            self.selected.remove(&id);
        } else {
            self.selected.insert(id);
        }
        self.active = Some(id);
    }

    pub fn insert(&mut self, mut layer: Layer) {
        let index = self
            .layers
            .iter()
            .position(|layer| Some(layer.id) == self.active);
        if let Some(active) = self.active() {
            layer.parent = if active.group {
                Some(active.id)
            } else {
                active.parent
            };
        }
        self.select(layer.id, false);
        self.layers
            .insert(index.map_or(self.layers.len(), |i| i + 1), layer);
    }

    pub fn descendants(&self, id: Uuid) -> HashSet<Uuid> {
        let mut result = HashSet::from([id]);
        loop {
            let old = result.len();
            for layer in &self.layers {
                if layer.parent.is_some_and(|parent| result.contains(&parent)) {
                    result.insert(layer.id);
                }
            }
            if result.len() == old {
                break;
            }
        }
        result
    }

    pub fn transform_targets(&self) -> HashSet<Uuid> {
        let mut result = self.selected.clone();
        for id in &self.selected {
            result.extend(self.descendants(*id));
        }
        result
    }

    pub fn delete_selected(&mut self) {
        let deleted = self.transform_targets();
        self.layers.retain(|layer| !deleted.contains(&layer.id));
        for layer in &mut self.layers {
            if layer.clip_to.is_some_and(|id| deleted.contains(&id)) {
                layer.clip_to = None;
            }
        }
        self.active = self.layers.last().map(|layer| layer.id);
        self.selected = self.active.into_iter().collect();
    }

    pub fn validate(&self) -> Result<()> {
        validate_size(self.width, self.height)?;
        ensure!(
            self.resolution.is_finite() && (1.0..=9600.0).contains(&self.resolution),
            "Invalid resolution"
        );
        ensure!(self.layers.len() <= MAX_LAYERS, "Too many layers");
        let ids: HashSet<_> = self.layers.iter().map(|layer| layer.id).collect();
        ensure!(
            ids.len() == self.layers.len(),
            "Duplicate layer identifiers"
        );
        ensure!(
            self.active.is_none_or(|id| ids.contains(&id)),
            "Missing active layer"
        );
        let mut pixels = 0_u64;
        let mut mask_pixels = 0_u64;
        for layer in &self.layers {
            if let Some(adjustment) = &layer.adjustment {
                crate::effects::validate_adjustment(adjustment)?;
            }
            ensure!(
                !layer.name.trim().is_empty() && layer.name.len() <= 16_384,
                "Invalid layer name"
            );
            ensure!(layer.transform.valid(), "Invalid layer transform");
            ensure!(
                layer.opacity.is_finite() && (0.0..=1.0).contains(&layer.opacity),
                "Invalid opacity"
            );
            ensure!(
                !(layer.group || layer.adjustment.is_some()) || layer.pixels.is_none(),
                "Group/adjustment cannot contain pixels"
            );
            if let Some(image) = &layer.pixels {
                validate_size(image.width(), image.height())?;
                pixels += u64::from(image.width()) * u64::from(image.height());
            }
            if let Some(mask) = &layer.mask {
                validate_size(mask.pixels.width(), mask.pixels.height())?;
                ensure!(
                    mask.placement.is_none_or(Transform::valid),
                    "Invalid mask transform"
                );
                mask_pixels += u64::from(mask.pixels.width()) * u64::from(mask.pixels.height());
            }
            let mut parent = layer.parent;
            let mut visited = HashSet::from([layer.id]);
            while let Some(id) = parent {
                ensure!(
                    visited.insert(id) && visited.len() <= 65,
                    "Cyclic or excessively nested groups"
                );
                let group = self.layers.iter().find(|l| l.id == id);
                ensure!(group.is_some_and(|l| l.group), "Missing parent group");
                parent = group.and_then(|l| l.parent);
            }
            let mut source = layer.clip_to;
            let mut visited = HashSet::from([layer.id]);
            while let Some(id) = source {
                ensure!(
                    !layer.group && visited.insert(id) && visited.len() <= 257,
                    "Invalid clipping mask graph"
                );
                let target = self.layers.iter().find(|l| l.id == id);
                ensure!(target.is_some_and(|l| !l.group), "Missing clipping source");
                source = target.and_then(|l| l.clip_to);
            }
        }
        ensure!(
            pixels <= MAX_PIXELS && mask_pixels <= MAX_PIXELS,
            "Project exceeds the 100 megapixel asset limit"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transform_round_trip_with_rotation_and_flips() {
        for flip_x in [false, true] {
            let t = Transform {
                x: -13.0,
                y: 25.0,
                width: 90.0,
                height: 45.0,
                rotation: 37.0,
                flip_x,
                flip_y: true,
            };
            let p = Point::new(0.17, 0.89);
            assert!(t.inverse(t.point(p)).distance(p) < 0.00001);
        }
    }

    #[test]
    fn rejects_invalid_hierarchy_and_clipping_cycles() {
        let mut doc = Document::new(8, 8).unwrap();
        doc.layers[0].clip_to = Some(doc.layers[0].id);
        assert!(doc.validate().is_err());
        doc.layers[0].clip_to = None;
        doc.layers[0].parent = Some(Uuid::new_v4());
        assert!(doc.validate().is_err());
    }

    #[test]
    fn deleting_group_removes_descendants_and_stale_links() {
        let mut doc = Document::new(8, 8).unwrap();
        doc.layers[0].group = true;
        let group = doc.layers[0].id;
        doc.insert(Layer::blank("Child", 8, 8));
        let child = doc.active.unwrap();
        let mut outside = Layer::blank("Outside", 8, 8);
        outside.clip_to = Some(child);
        doc.layers.push(outside);
        doc.select(group, false);
        doc.delete_selected();
        assert_eq!(doc.layers.len(), 1);
        assert_eq!(doc.layers[0].clip_to, None);
        doc.validate().unwrap();
    }
}
