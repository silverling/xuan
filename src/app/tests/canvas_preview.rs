use super::*;

#[test]
fn magnified_preview_preserves_source_pixels_and_reuses_the_native_texture() {
    let context = egui::Context::default();
    context.input_mut(|input| input.max_texture_side = 8192);
    // Exceed both preview caps without allocating a full photograph in this test.
    let pixels = RgbaImage::from_fn(5712, 24, |x, y| {
        image::Rgba([if x % 2 == 0 { 0 } else { 255 }, y as u8 * 10, 0, 255])
    });
    let mut document = Document::new(pixels.width(), pixels.height()).unwrap();
    document.layers = vec![Layer::image("Pixel detail", pixels.clone())];
    let mut session = Session::new(document, "Pixel detail".into(), None);
    session.zoom = 0.1;
    session.refresh(&context, None);
    assert_eq!(session.preview_size[0], 1600);
    let texture = session.texture.as_ref().unwrap().id();

    session.zoom = 1.0;
    session.refresh(&context, None);
    assert_eq!(session.preview_size, [5712, 24]);
    assert_eq!(session.composite.as_deref().unwrap(), &pixels);
    assert_eq!(session.texture.as_ref().unwrap().id(), texture);
    let native = session.composite.clone().unwrap();
    let delta = context.tex_manager().write().take_delta();
    let options = delta
        .set
        .iter()
        .find(|(id, _)| *id == texture)
        .unwrap()
        .1
        .options;
    assert_eq!(options.magnification, egui::TextureFilter::Nearest);
    assert_eq!(options.minification, egui::TextureFilter::Linear);

    for zoom in [8.0, 64.0, 0.1, 1.0, 64.0] {
        session.zoom = zoom;
        session.pan += Vec2::new(12.5, -7.25);
        session.refresh(&context, None);
        assert!(Arc::ptr_eq(session.composite.as_ref().unwrap(), &native));
        assert!(context.tex_manager().write().take_delta().set.is_empty());
    }

    session.document.layers[0].opacity = 0.5;
    session.invalidate();
    session.refresh(&context, None);
    assert_eq!(session.preview_size, [5712, 24]);
    assert_eq!(
        session.composite.as_ref().unwrap().get_pixel(100, 12)[3],
        128
    );

    // An edit while zoomed out can release the larger cached preview.
    session.zoom = 0.1;
    session.invalidate();
    session.refresh(&context, None);
    assert_eq!(session.preview_size[0], 1600);
    session.zoom = 64.0;
    session.refresh(&context, None);
    assert_eq!(session.preview_size, [5712, 24]);
}

#[test]
fn magnified_preview_respects_the_texture_limit() {
    let context = egui::Context::default();
    context.input_mut(|input| input.max_texture_side = 1024);
    let mut session = Session::new(Document::new(5712, 24).unwrap(), "Wide canvas".into(), None);
    for zoom in [0.1, 64.0] {
        session.zoom = zoom;
        session.refresh(&context, None);
        assert_eq!(session.preview_size, [1024, 4]);
    }
}

#[test]
fn pixel_grid_requires_a_native_resolution_preview() {
    let (context, mut app) = app();
    let mut session = Session::new(Document::new(5712, 24).unwrap(), "Wide canvas".into(), None);
    session.fit = false;
    session.zoom = 64.0;
    app.sessions.push(session);
    app.tool = Tool::Hand;

    for (limit, expected_grid) in [(1024, false), (8192, true)] {
        context.input_mut(|input| input.max_texture_side = limit);
        let output = frame(&context, &mut app);
        let has_grid = output.shapes.iter().any(|shape| {
            matches!(
                shape.shape,
                egui::Shape::LineSegment { stroke, .. }
                    if stroke == egui::Stroke::new(0.5_f32, egui::Color32::from_white_alpha(28))
            )
        });
        assert_eq!(has_grid, expected_grid);
    }
}

#[test]
#[ignore = "requires a GPU; optionally set XUAN_ZOOM_BENCH_IMAGE to an image path"]
fn magnified_gpu_preview_matches_source_pixels_at_fractional_display_scales() {
    let (context, mut app, state) = large_image_benchmark_app();
    context.input_mut(|input| {
        input.max_texture_side = state.device.limits().max_texture_dimension_2d as usize;
    });
    if std::env::var_os("XUAN_ZOOM_BENCH_IMAGE").is_none() {
        let pixels = RgbaImage::from_fn(5712, 64, |x, y| {
            image::Rgba([if x % 2 == 0 { 0 } else { 255 }, y as u8 * 4, 0, 255])
        });
        let mut document = Document::new(pixels.width(), pixels.height()).unwrap();
        document.layers = vec![Layer::image("Pixel detail", pixels)];
        app.sessions[0] = Session::new(document, "Pixel detail".into(), None);
    }
    app.tool = Tool::Hand;
    let session = app.session_mut().unwrap();
    session.fit = false;
    session.zoom = 0.1;
    for _ in 0..3 {
        render_frame(&context, &mut app, &state);
    }
    assert_eq!(
        app.session().unwrap().gpu.is_some(),
        state.adapter.get_info().device_type != wgpu::DeviceType::Cpu
    );
    assert!(app.session().unwrap().preview_size[0] <= 4096);

    for scale in [1.0, 1.7] {
        context.set_pixels_per_point(scale);
        let session = app.session_mut().unwrap();
        session.zoom = 64.0;
        session.pan = Vec2::new(13.25, -7.75);
        // egui adjusts the logical viewport for one frame when the scale changes.
        render_frame(&context, &mut app, &state);
        let captured = render_frame(&context, &mut app, &state);
        let session = app.session().unwrap();
        let pixels = session.document.layers[0].pixels.as_ref().unwrap();
        assert_eq!(session.preview_size, [pixels.width(), pixels.height()]);
        let origin = app.canvas_rect.unwrap().min;

        // Sample throughout neighboring grid cells, not just their centers: a
        // stretched overview texel may straddle a grid line without losing its center.
        for y in pixels.height() / 2 - 2..=pixels.height() / 2 + 2 {
            for x in pixels.width() / 2 - 2..=pixels.width() / 2 + 2 {
                let expected = pixels.get_pixel(x, y);
                for offset in [0.125, 0.5, 0.875] {
                    let screen = (origin
                        + Vec2::new(x as f32 + offset, y as f32 + offset) * session.zoom)
                        * scale;
                    let actual = captured.get_pixel(screen.x as u32, screen.y as u32);
                    assert!(
                        actual
                            .0
                            .iter()
                            .zip(expected.0)
                            .all(|(a, b)| a.abs_diff(b) <= 2),
                        "scale {scale}, pixel ({x}, {y}), offset {offset}: {actual:?} != {expected:?}"
                    );
                }
            }
        }
        if let Some(path) = std::env::var_os("XUAN_PIXEL_GRID_SCREENSHOT") {
            captured.save(path).unwrap();
        }
    }
}

fn render_frame(
    context: &egui::Context,
    app: &mut EditorApp,
    state: &eframe::egui_wgpu::RenderState,
) -> RgbaImage {
    let output = frame(context, app);
    let screen = eframe::egui_wgpu::ScreenDescriptor {
        size_in_pixels: [1280.0, 860.0].map(|side| (side * output.pixels_per_point).round() as u32),
        pixels_per_point: output.pixels_per_point,
    };
    let jobs = context.tessellate(output.shapes, output.pixels_per_point);
    let [width, height] = screen.size_in_pixels;
    let target = state.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("pixel grid verification"),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: state.target_format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let stride = (width * 4).div_ceil(256) * 256;
    let buffer = state.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("pixel grid readback"),
        size: u64::from(stride) * u64::from(height),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut renderer = state.renderer.write();
    for (id, delta) in &output.textures_delta.set {
        renderer.update_texture(&state.device, &state.queue, *id, delta);
    }
    let mut encoder = state.device.create_command_encoder(&Default::default());
    let commands =
        renderer.update_buffers(&state.device, &state.queue, &mut encoder, &jobs, &screen);
    let view = target.create_view(&Default::default());
    {
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        renderer.render(&mut pass.forget_lifetime(), &jobs, &screen);
    }
    encoder.copy_texture_to_buffer(
        target.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(height),
            },
        },
        target.size(),
    );
    state
        .queue
        .submit(commands.into_iter().chain([encoder.finish()]));
    for id in &output.textures_delta.free {
        renderer.free_texture(id);
    }
    let (send, receive) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            send.send(result).unwrap();
        });
    state
        .device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    receive.recv().unwrap().unwrap();
    let mapped = buffer.slice(..).get_mapped_range();
    let pixels = mapped
        .chunks_exact(stride as usize)
        .flat_map(|row| row[..width as usize * 4].iter().copied())
        .collect();
    RgbaImage::from_raw(width, height, pixels).unwrap()
}
