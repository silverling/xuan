use super::{Brush, Document, Layer, PaintMode, Point, Result, StrokeOptions};

/// Coverage and original pixels for one pointer-down/up gesture. Overlapping
/// segments use the strongest coverage, so event frequency cannot darken joins.
#[derive(Default)]
pub struct Stroke {
    bounds: [u32; 4],
    pixels: Vec<StrokePixel>,
}

#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C)]
pub(crate) struct StrokePixel {
    pub original: [u8; 4],
    pub amount: f32,
}

impl Stroke {
    /// Append a segment, keeping paint/erase opacity consistent within this stroke.
    /// Start a fresh `Stroke` for each gesture; retouch tools retain their buildup.
    pub fn segment(
        &mut self,
        document: &mut Document,
        from: Point,
        to: Point,
        from_brush: &Brush,
        brush: &Brush,
        options: StrokeOptions<'_>,
    ) -> Result<()> {
        let accumulate =
            options.mask_target || matches!(options.mode, PaintMode::Paint | PaintMode::Erase);
        super::stroke_segment(
            document,
            from,
            to,
            from_brush,
            brush,
            options,
            accumulate.then_some(self),
        )
    }

    pub(super) fn shift(&mut self, [x, y]: [u32; 2]) {
        if !self.pixels.is_empty() {
            self.bounds[0] += x;
            self.bounds[2] += x;
            self.bounds[1] += y;
            self.bounds[3] += y;
        }
    }

    pub(super) fn prepare(&mut self, layer: &Layer, mask: bool, bounds: [u32; 4]) {
        let [left, top, right, bottom] = self.bounds;
        if !self.pixels.is_empty()
            && bounds[0] >= left
            && bounds[1] >= top
            && bounds[2] <= right
            && bounds[3] <= bottom
        {
            return;
        }
        let (width, height) = if mask {
            layer.mask.as_ref().unwrap().pixels.dimensions()
        } else {
            layer.pixels.as_ref().unwrap().dimensions()
        };
        // Reserve in blocks to avoid reallocating for every small pointer move.
        // Only the stroke's bounding region needs snapshots, not the whole layer.
        let mut next = [
            bounds[0] / 64 * 64,
            bounds[1] / 64 * 64,
            (bounds[2].div_ceil(64) * 64).min(width),
            (bounds[3].div_ceil(64) * 64).min(height),
        ];
        if !self.pixels.is_empty() {
            next = [
                next[0].min(left),
                next[1].min(top),
                next[2].max(right),
                next[3].max(bottom),
            ];
        }
        let stride = (next[2] - next[0]) as usize;
        let mut pixels = Vec::with_capacity(stride * (next[3] - next[1]) as usize);
        for y in next[1]..next[3] {
            for x in next[0]..next[2] {
                let original = if mask {
                    let value = layer.mask.as_ref().unwrap().pixels.get_pixel(x, y)[0];
                    [value, value, value, 255]
                } else {
                    layer.pixels.as_ref().unwrap().get_pixel(x, y).0
                };
                pixels.push(StrokePixel {
                    original,
                    amount: 0.0,
                });
            }
        }
        for y in top..bottom {
            let start = (y - next[1]) as usize * stride + (left - next[0]) as usize;
            pixels[start..start + (right - left) as usize]
                .copy_from_slice(self.row(left, right, y));
        }
        self.bounds = next;
        self.pixels = pixels;
    }

    pub(crate) fn row(&self, left: u32, right: u32, y: u32) -> &[StrokePixel] {
        let start = self.index(left, y);
        &self.pixels[start..start + (right - left) as usize]
    }

    pub(crate) fn pixel_mut(&mut self, x: u32, y: u32) -> &mut StrokePixel {
        let index = self.index(x, y);
        &mut self.pixels[index]
    }

    fn index(&self, x: u32, y: u32) -> usize {
        (y - self.bounds[1]) as usize * (self.bounds[2] - self.bounds[0]) as usize
            + (x - self.bounds[0]) as usize
    }
}
