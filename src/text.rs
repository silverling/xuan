use std::sync::Arc;

use anyhow::{Result, ensure};
use cosmic_text::{
    Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, Style, SwashCache, Weight, Wrap,
};
use image::{Pixel, Rgba, RgbaImage};
use serde::{Deserialize, Serialize};

use crate::document::{Layer, Point, validate_size};

pub const MAX_TEXT_BYTES: usize = 16_384;
const FALLBACK_FAMILY: &str = "Inter Variable";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TextStyle {
    pub content: String,
    pub family: String,
    pub size: f32,
    pub color: [u8; 4],
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strikethrough: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            content: "Text".into(),
            family: FALLBACK_FAMILY.into(),
            size: 48.0,
            color: [0, 0, 0, 255],
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
        }
    }
}

impl TextStyle {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.content.len() <= MAX_TEXT_BYTES,
            "Text is limited to 16 KiB"
        );
        ensure!(
            !self.family.trim().is_empty() && self.family.len() <= 1024,
            "Invalid font family"
        );
        ensure!(
            self.size.is_finite() && (1.0..=1024.0).contains(&self.size),
            "Font size must be between 1 and 1024 pixels"
        );
        Ok(())
    }

    pub fn layer_name(&self) -> String {
        let name: String = self
            .content
            .lines()
            .next()
            .unwrap_or("")
            .trim()
            .chars()
            .take(60)
            .collect();
        if name.is_empty() { "Text".into() } else { name }
    }
}

/// One font database per editor, loaded lazily when the text tool is first used.
pub struct TextRenderer {
    fonts: FontSystem,
    families: Vec<String>,
}

impl Default for TextRenderer {
    fn default() -> Self {
        let fonts = FontSystem::new_with_fonts([cosmic_text::fontdb::Source::Binary(Arc::new(
            include_bytes!("../assets/fonts/InterVariable.ttf").to_vec(),
        ))]);
        Self::with_fonts(fonts)
    }
}

impl TextRenderer {
    fn with_fonts(fonts: FontSystem) -> Self {
        let mut families: Vec<_> = fonts
            .db()
            .faces()
            .flat_map(|face| face.families.iter().map(|(name, _)| name.clone()))
            .collect();
        families.sort_by_key(|name| name.to_lowercase());
        families.dedup();
        Self { fonts, families }
    }

    pub fn families(&self) -> &[String] {
        &self.families
    }

    pub fn has_family(&self, family: &str) -> bool {
        self.families
            .iter()
            .any(|name| name.eq_ignore_ascii_case(family))
    }

    pub fn render(&mut self, style: &TextStyle) -> Result<RgbaImage> {
        style.validate()?;
        let line_height = style.size * 1.3;
        ensure!(
            (style.content.lines().count().max(1) as f32 * line_height) <= 30_000.0,
            "Text is too tall"
        );
        let family = if self.has_family(&style.family) {
            &style.family
        } else {
            FALLBACK_FAMILY
        };
        // Resolve the closest available face first. Requesting a missing weight or
        // style directly can substitute an unrelated family during shaping.
        let face = self
            .fonts
            .db()
            .query(&cosmic_text::fontdb::Query {
                families: &[Family::Name(family)],
                weight: if style.bold {
                    Weight::BOLD
                } else {
                    Weight::NORMAL
                },
                style: if style.italic {
                    Style::Italic
                } else {
                    Style::Normal
                },
                ..Default::default()
            })
            .and_then(|id| self.fonts.db().face(id))
            .ok_or_else(|| anyhow::anyhow!("The font could not be loaded"))?;
        let attrs = Attrs::new()
            .family(Family::Name(family))
            .weight(face.weight)
            .style(face.style)
            .stretch(face.stretch);
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(style.size, line_height));
        buffer.set_wrap(&mut self.fonts, Wrap::None);
        buffer.set_size(&mut self.fonts, None, None);
        buffer.set_text(&mut self.fonts, &style.content, &attrs, Shaping::Advanced);

        let mut right = 1.0_f32;
        let mut bottom = line_height;
        for run in buffer.layout_runs() {
            right = right.max(run.line_w);
            bottom = bottom.max(run.line_top + run.line_height);
        }
        validate_size(right.ceil() as u32, bottom.ceil() as u32)?;

        // Measure actual ink as well as advances so italic overhangs and combining
        // marks are preserved. Keep the glyph cache local to bound retained memory.
        let mut cache = SwashCache::new();
        let (mut left, mut top) = (0, 0);
        let (mut right, mut bottom) = (right.ceil() as i32, bottom.ceil() as i32);
        let mut glyphs = Vec::new();
        let mut rules = Vec::new();
        for run in buffer.layout_runs() {
            for glyph in run.glyphs {
                let mut physical = glyph.physical((0.0, 0.0), 1.0);
                if style.italic
                    && self
                        .fonts
                        .db()
                        .face(glyph.font_id)
                        .is_some_and(|face| face.style == Style::Normal)
                {
                    physical.cache_key.flags |= cosmic_text::CacheKeyFlags::FAKE_ITALIC;
                }
                let y = run.line_y as i32 + physical.y;
                // Families without a bold face still have a visible bold style.
                let embolden = if style.bold
                    && self
                        .fonts
                        .db()
                        .face(glyph.font_id)
                        .is_some_and(|face| face.weight < Weight::SEMIBOLD)
                {
                    (style.size * 0.025).ceil() as i32
                } else {
                    0
                };
                if let Some(image) = cache.get_image(&mut self.fonts, physical.cache_key) {
                    let placement = image.placement;
                    let x = physical.x + placement.left;
                    let y = y - placement.top;
                    left = left.min(x);
                    top = top.min(y);
                    right = right.max(x + placement.width as i32 + embolden);
                    bottom = bottom.max(y + placement.height as i32);
                }
                glyphs.push((physical, y, embolden));
            }
            let thickness = (style.size / 16.0).max(1.0);
            for (enabled, y) in [
                (style.underline, run.line_y + style.size * 0.1),
                (style.strikethrough, run.line_y - style.size * 0.3),
            ] {
                if enabled && run.line_w > 0.0 {
                    top = top.min(y.floor() as i32);
                    bottom = bottom.max((y + thickness).ceil() as i32);
                    rules.push((run.line_w, y, thickness));
                }
            }
        }
        let (width, height) = ((right - left) as u32, (bottom - top) as u32);
        validate_size(width, height)?;
        let mut pixels = RgbaImage::new(width, height);
        let color = Color::rgb(style.color[0], style.color[1], style.color[2]);
        for (glyph, baseline, embolden) in glyphs {
            cache.with_pixels(&mut self.fonts, glyph.cache_key, color, |x, y, color| {
                for offset in 0..=embolden {
                    if let Some(pixel) = pixels.get_pixel_mut_checked(
                        (glyph.x + x + offset - left) as u32,
                        (baseline + y - top) as u32,
                    ) {
                        pixel.blend(&Rgba(color.as_rgba()));
                    }
                }
            });
        }
        for (width, y, thickness) in rules {
            for py in y.floor() as i32..(y + thickness).ceil() as i32 {
                for px in 0..width.ceil() as i32 {
                    let coverage = (width - px as f32).min(1.0)
                        * ((y + thickness).min(py as f32 + 1.0) - y.max(py as f32));
                    let mut rgba = color.as_rgba();
                    rgba[3] = (coverage * 255.0).round() as u8;
                    pixels
                        .get_pixel_mut((px - left) as u32, (py - top) as u32)
                        .blend(&Rgba(rgba));
                }
            }
        }
        for pixel in pixels.pixels_mut() {
            pixel[3] = ((u16::from(pixel[3]) * u16::from(style.color[3]) + 127) / 255) as u8;
        }
        Ok(pixels)
    }
}

/// Replace the text while retaining the layer's scale, rotation, and top-left anchor.
pub fn update_layer(layer: &mut Layer, style: TextStyle, pixels: RgbaImage) -> Result<()> {
    style.validate()?;
    ensure!(
        !layer.locked && layer.text.is_some(),
        "Select an unlocked text layer"
    );
    let old = layer
        .pixels
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Text layer has no pixels"))?;
    let anchor = layer.transform.point(Point::new(0.0, 0.0));
    let mut transform = layer.transform;
    transform.width *= pixels.width() as f32 / old.width() as f32;
    transform.height *= pixels.height() as f32 / old.height() as f32;
    let moved = transform.point(Point::new(0.0, 0.0));
    transform.x += anchor.x - moved.x;
    transform.y += anchor.y - moved.y;
    ensure!(transform.valid(), "Text transform is too large");
    if layer
        .text
        .as_ref()
        .is_some_and(|old| old.layer_name() == layer.name)
    {
        layer.name = style.layer_name();
    }
    layer.set_transform(transform);
    layer.pixels = Some(Arc::new(pixels));
    layer.text = Some(style);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        document::{Document, Mask},
        io, paint, render,
    };

    fn renderer() -> TextRenderer {
        let mut db = cosmic_text::fontdb::Database::new();
        db.load_font_data(include_bytes!("../assets/fonts/InterVariable.ttf").to_vec());
        TextRenderer::with_fonts(FontSystem::new_with_locale_and_db("en-US".into(), db))
    }

    fn layer(renderer: &mut TextRenderer, style: TextStyle) -> Layer {
        let mut layer = Layer::image(style.layer_name(), renderer.render(&style).unwrap());
        layer.text = Some(style);
        layer
    }

    #[test]
    fn font_discovery_includes_installed_families_and_bundled_fallback() {
        let renderer = TextRenderer::default();
        assert!(renderer.has_family(FALLBACK_FAMILY));
        for face in renderer.fonts.db().faces() {
            for (family, _) in &face.families {
                assert!(renderer.has_family(family));
            }
        }
        let mut fallback = self::renderer();
        let missing = TextStyle {
            family: "Definitely unavailable Xuan test font".into(),
            ..Default::default()
        };
        assert_eq!(
            fallback.render(&missing).unwrap(),
            fallback.render(&TextStyle::default()).unwrap()
        );
    }

    #[test]
    fn styles_change_ink_and_preserve_color_alpha_and_multiline_layout() {
        let mut renderer = renderer();
        let style = TextStyle {
            content: "Office fj Å\nSecond line".into(),
            color: [30, 110, 190, 128],
            ..Default::default()
        };
        let plain = renderer.render(&style).unwrap();
        assert!(plain.height() >= (style.size * 2.6) as u32);
        assert!(plain.pixels().any(|pixel| pixel[3] == 128));
        assert!(plain.pixels().any(|pixel| (1..128).contains(&pixel[3])));
        for index in 0..4 {
            let mut decorated = style.clone();
            match index {
                0 => decorated.bold = true,
                1 => decorated.italic = true,
                2 => decorated.underline = true,
                _ => decorated.strikethrough = true,
            }
            let pixels = renderer.render(&decorated).unwrap();
            assert_ne!(
                pixels, plain,
                "Decoration {index} must affect rendered text"
            );
            for pixel in pixels.pixels().filter(|pixel| pixel[3] > 0) {
                assert!(pixel[3] <= 128);
                for channel in 0..3 {
                    assert!(
                        (i16::from(pixel[channel]) - i16::from(style.color[channel])).abs() <= 1
                    );
                }
            }
        }
    }

    #[test]
    fn empty_text_and_invalid_or_oversized_input_are_handled() {
        let mut renderer = renderer();
        let mut style = TextStyle {
            content: String::new(),
            ..Default::default()
        };
        assert!(
            renderer
                .render(&style)
                .unwrap()
                .pixels()
                .all(|pixel| pixel[3] == 0)
        );
        for size in [0.0, -1.0, f32::NAN, f32::INFINITY, 1025.0] {
            style.size = size;
            assert!(renderer.render(&style).is_err());
        }
        style.size = 1024.0;
        style.content = "W".repeat(100);
        assert!(renderer.render(&style).is_err());
        style.content = "line\n".repeat(100);
        assert!(renderer.render(&style).is_err());
        style.content = "a".repeat(MAX_TEXT_BYTES + 1);
        assert!(renderer.render(&style).is_err());
    }

    #[test]
    fn text_round_trips_with_pixels_and_remains_editable_after_transform() {
        let mut renderer = renderer();
        let mut layer = layer(&mut renderer, TextStyle::default());
        layer.transform.x = 20.0;
        layer.transform.y = 30.0;
        layer.transform.rotation = 25.0;
        layer.transform.width *= 1.5;
        layer.transform.height *= 0.75;
        layer.mask = Some(Mask::white());
        let anchor = layer.transform.point(Point::default());
        let mut style = layer.text.clone().unwrap();
        style.content = "Longer text\nwith decorations".into();
        style.underline = true;
        style.strikethrough = true;
        let pixels = renderer.render(&style).unwrap();
        let dimensions = pixels.dimensions();
        update_layer(&mut layer, style.clone(), pixels).unwrap();
        assert!(anchor.distance(layer.transform.point(Point::default())) < 0.001);
        assert!((layer.transform.width - dimensions.0 as f32 * 1.5).abs() < 0.001);
        assert!((layer.transform.height - dimensions.1 as f32 * 0.75).abs() < 0.001);
        let mut document = Document::new(600, 300).unwrap();
        document.insert(layer);
        let file = tempfile::NamedTempFile::new().unwrap();
        io::save(&document, file.path()).unwrap();
        let loaded = io::load(file.path()).unwrap();
        assert_eq!(loaded.active().unwrap().text, Some(style));
        assert_eq!(
            loaded.active().unwrap().pixels,
            document.active().unwrap().pixels
        );
        assert_eq!(render::render(&loaded), render::render(&document));

        let old = document.active_mut().unwrap();
        paint::ensure_pixels(old).unwrap();
        assert!(old.text.is_none());
        let mut json = serde_json::to_value(old).unwrap();
        json.as_object_mut().unwrap().remove("text");
        assert!(
            serde_json::from_value::<Layer>(json)
                .unwrap()
                .text
                .is_none()
        );
    }
}
