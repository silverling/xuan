use super::tests::{compare, readback};
use super::*;
use crate::{document::Mask, effects::Filter};
use image::Rgba;

#[test]
#[ignore = "requires a Vulkan or OpenGL compute adapter"]
fn filter_layers_match_cpu_and_reuse_sources() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let processor = Processor::new(device.clone(), queue.clone());
    let mut compositor = GpuCompositor::new(device, queue);
    let mut document = Document::new(160, 144).unwrap();
    let source = Layer::image(
        "Source",
        RgbaImage::from_fn(160, 144, |x, y| {
            Rgba([
                x as u8,
                y as u8,
                (x + y) as u8,
                if x < 80 { 255 } else { 64 },
            ])
        }),
    );
    let mut group = Layer::blank("Group", 160, 144);
    group.group = true;
    group.opacity = 0.7;
    let mut effect = Layer::blank("Filter", 160, 144);
    effect.parent = Some(group.id);
    effect.opacity = 0.8;
    effect.mask = Some(Mask {
        pixels: Arc::new(image::GrayImage::from_fn(160, 144, |x, _| {
            image::Luma([if x < 48 { 0 } else { 192 }])
        })),
        ..Mask::white()
    });
    let mut mask = Layer::mask("Group mask", 160, 144);
    mask.parent = Some(group.id);
    mask.opacity = 0.4;
    mask.mask.as_mut().unwrap().pixels =
        Arc::new(image::GrayImage::from_pixel(1, 1, image::Luma([192])));
    let mut adjustment = Layer::blank("Invert", 160, 144);
    adjustment.adjustment = Some(Adjustment::Invert);
    document.select(source.id, false);
    document.layers = vec![source, group, effect, mask, adjustment];

    for filter in [
        Filter::GaussianBlur { radius: 0.5 },
        Filter::GaussianBlur { radius: 4.0 },
        Filter::GaussianBlur { radius: 35.0 },
        Filter::MotionBlur {
            distance: 15.0,
            angle: 32.0,
        },
        Filter::MotionBlur {
            distance: 200.0,
            angle: -120.0,
        },
        Filter::Noise {
            amount: 24.0,
            monochrome: false,
        },
        Filter::Noise {
            amount: 24.0,
            monochrome: true,
        },
        Filter::LensCorrection {
            distortion: 22.0,
            vignette: 30.0,
        },
        Filter::LensCorrection {
            distortion: -22.0,
            vignette: -30.0,
        },
    ] {
        document.layers[2].filter = Some(filter.clone());
        document.validate().unwrap();
        for size in [[160, 144], [80, 72]] {
            // Repeated changes must reuse the source upload, even though filtering changes.
            scope(Some(processor.clone()), || {
                compositor.render(&document, size)
            });
            let textures: Vec<_> = compositor
                .sources
                .iter()
                .map(|(key, source)| (*key, source.texture.clone()))
                .collect();
            let actual = readback(&compositor);
            let expected = render::render_scaled(&document, size[0], size[1]);
            for (index, (a, b)) in actual
                .as_chunks::<4>()
                .0
                .iter()
                .zip(expected.pixels())
                .enumerate()
            {
                let expected = [
                    (b[0] as f32 * b[3] as f32 / 255.0).round() as u8,
                    (b[1] as f32 * b[3] as f32 / 255.0).round() as u8,
                    (b[2] as f32 * b[3] as f32 / 255.0).round() as u8,
                    b[3],
                ];
                assert!(
                    a.iter().zip(expected).all(|(a, b)| a.abs_diff(b) <= 3),
                    "{filter:?} {size:?} pixel {index}: GPU {a:?}, CPU {expected:?}"
                );
            }
            document.layers[2].opacity = 0.5;
            compositor.render(&document, size);
            for (key, texture) in textures {
                assert_eq!(compositor.sources[&key].texture, texture);
            }
            document.layers[2].opacity = 0.8;
        }
        let actual = processor.compose(&document, 160, 144).unwrap();
        let expected = render::render(&document);
        for (a, b) in actual.pixels().zip(expected.pixels()) {
            assert!(
                a.0.iter().zip(b.0).all(|(a, b)| a.abs_diff(b) <= 4),
                "{filter:?}: GPU {a:?}, CPU {b:?}"
            );
        }
    }

    let mut second = Layer::blank("Second filter", 160, 144);
    second.filter = Some(Filter::MotionBlur {
        distance: 9.0,
        angle: 45.0,
    });
    document.layers.push(second);
    for hidden in [None, Some(1), Some(2), Some(5)] {
        if let Some(index) = hidden {
            document.layers[index].visible = false;
        }
        for accelerated in [false, true] {
            scope(accelerated.then(|| processor.clone()), || {
                compositor.render(&document, [160, 144]);
            });
            compare(
                &document,
                &readback(&compositor),
                "Stacked filters and visibility",
            );
        }
        if let Some(index) = hidden {
            document.layers[index].visible = true;
        }
    }
}
