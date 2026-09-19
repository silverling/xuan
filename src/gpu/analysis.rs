use super::processor::attempt;
use image::RgbaImage;

pub struct Analysis {
    pub luminance: [u32; 256],
    pub channels: [[u32; 256]; 3],
    pub clipping: [f32; 2],
    pub warnings: Option<RgbaImage>,
}
pub fn analyze(image: &RgbaImage, warnings: bool) -> Option<Analysis> {
    let size = [image.width(), image.height()];
    let count = u64::from(size[0]) * u64::from(size[1]);
    attempt(count, 65_536, |gpu| {
        let source = gpu.buffer(image.as_raw())?;
        let bytes = 4112 + if warnings { count * 4 } else { 0 };
        let result = gpu.empty(bytes)?;
        let mut encoder = gpu.encoder();
        gpu.dispatch(
            &mut encoder,
            "analyze_pixels",
            include_str!("analysis.wgsl"),
            [&source, &source, &result],
            &[[
                size[0] as f32,
                size[1] as f32,
                if warnings { 1.0 } else { 0.0 },
                0.0,
            ]],
            size,
        )?;
        let result = gpu.read(encoder, &result, bytes)?;
        let bins: Vec<u32> = result[..4112]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|p| u32::from_le_bytes(*p))
            .collect();
        Ok(Analysis {
            luminance: std::array::from_fn(|i| bins[i]),
            channels: std::array::from_fn(|c| std::array::from_fn(|i| bins[256 + c * 256 + i])),
            clipping: std::array::from_fn(|i| {
                100.0 * bins[1025 + i] as f32 / bins[1024].max(1) as f32
            }),
            warnings: warnings
                .then(|| RgbaImage::from_raw(size[0], size[1], result[4112..].to_vec()).unwrap()),
        })
    })
}
