use std::{io::Cursor, sync::Arc};

use egui::{Color32, RichText, Stroke, Vec2, vec2};
use xuan::{
    document::{Adjustment, Layer, Point},
    effects::{self, Filter},
    io, operations, render,
};

use super::{Dialog, EditorApp, theme};

impl EditorApp {
    pub(super) fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(job) = &self.job {
            egui::Window::new(&job.name)
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Working…");
                    });
                    if ui.button("Cancel").clicked() {
                        job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                    }
                });
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if let Some(dialog) = self.dialog {
            match dialog {
                Dialog::New | Dialog::CanvasSize | Dialog::ImageSize => {
                    self.size_dialog(ctx, dialog)
                }
                Dialog::Effect => self.effect_dialog(ctx),
                Dialog::Export => self.export_dialog(ctx),
                Dialog::Shortcuts => {
                    let mut open = true;
                    egui::Window::new("Keyboard shortcuts")
                        .open(&mut open)
                        .resizable(false)
                        .collapsible(false)
                        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                        .show(ctx, |ui| {
                            egui::Grid::new("shortcut_grid")
                                .spacing(vec2(35.0, 10.0))
                                .show(ui, |ui| {
                                    for (key, label) in [
                                        ("Ctrl+N / O / S", "New / Open / Save"),
                                        ("Ctrl+Shift+O", "Import image as layer"),
                                        ("Ctrl+Alt+Shift+S", "Export image"),
                                        ("Ctrl+Z / Ctrl+Shift+Z", "Undo / Redo"),
                                        ("Ctrl+J / Ctrl+E / Ctrl+G", "Duplicate / Merge / Group"),
                                        ("Ctrl+A / Ctrl+D", "Select all / Deselect"),
                                        ("Ctrl+C / Ctrl+V", "Copy / Paste image"),
                                        ("Ctrl+0 / Ctrl+1", "Fit / Actual pixels"),
                                        (
                                            "V / M / L / W / C",
                                            "Move / Marquee / Lasso / Wand / Crop",
                                        ),
                                        (
                                            "B / E / J / S / R",
                                            "Brush / Eraser / Heal / Clone / Blur",
                                        ),
                                        (
                                            "G / U / I / H / Z",
                                            "Gradient / Shape / Eyedropper / Hand / Zoom",
                                        ),
                                        ("[ / ] · Shift+[ / ]", "Brush size / Hardness"),
                                        ("1–0", "Brush or layer opacity"),
                                        ("Alt-click", "Set clone source"),
                                        ("X / D", "Swap / Reset colors"),
                                        ("Space-drag", "Pan canvas"),
                                        ("Enter / Escape", "Apply crop / Cancel gesture"),
                                    ] {
                                        ui.label(RichText::new(key).strong());
                                        ui.label(label);
                                        ui.end_row();
                                    }
                                });
                        });
                    if !open {
                        self.dialog = None;
                    }
                }
                Dialog::About => {
                    let mut open = true;
                    egui::Window::new("About xuan")
                        .open(&mut open)
                        .resizable(false)
                        .collapsible(false)
                        .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                        .show(ctx, |ui| {
                            ui.heading("xuan");
                            ui.label("A space for your next composition.");
                            ui.add_space(12.0);
                            ui.label("Native Linux image editor · Rust + egui + wgpu");
                            ui.label(format!("Version {}", env!("CARGO_PKG_VERSION")));
                            ui.add_space(12.0);
                            ui.label("Ported from Compositor by Wonder Assembly LLC.");
                            ui.label("Free and open source, under the MIT license.");
                        });
                    if !open {
                        self.dialog = None;
                    }
                }
            }
        }
        if let Some((id, mut name)) = self.rename.clone() {
            let mut done = false;
            let mut cancel = false;
            egui::Window::new("Rename layer")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    let response = ui.text_edit_singleline(&mut name);
                    response.request_focus();
                    ui.horizontal(|ui| {
                        cancel = ui.button("Cancel").clicked();
                        done = ui
                            .add_enabled(!name.trim().is_empty(), egui::Button::new("Rename"))
                            .clicked()
                            || ui.input(|i| i.key_pressed(egui::Key::Enter));
                    });
                });
            self.rename = Some((id, name.clone()));
            if done && !name.trim().is_empty() {
                self.edit("Rename Layer", |doc| {
                    if let Some(layer) = doc.layers.iter_mut().find(|l| l.id == id) {
                        layer.name = name;
                    }
                    Ok(())
                });
                self.rename = None;
            }
            if cancel || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.rename = None;
            }
        }
        self.close_dialog(ctx);
        if let Some(error) = self.error.clone() {
            let mut dismiss = false;
            egui::Window::new("Couldn't complete the operation")
                .collapsible(false)
                .resizable(false)
                .default_width(420.0)
                .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label(error);
                    ui.add_space(12.0);
                    dismiss = ui.button("OK").clicked();
                });
            if dismiss {
                self.error = None;
            }
        }
    }

    fn size_dialog(&mut self, ctx: &egui::Context, dialog: Dialog) {
        let title = match dialog {
            Dialog::New => "New canvas",
            Dialog::CanvasSize => "Canvas size",
            _ => "Image size",
        };
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        egui::Window::new(title)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(410.0)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.add_space(7.0);
                ui.label(
                    RichText::new(if dialog == Dialog::New {
                        "A blank space for your next composition."
                    } else if dialog == Dialog::CanvasSize {
                        "Change the canvas bounds and anchor your composition."
                    } else {
                        "Scale the composition while preserving source pixels."
                    })
                    .color(theme::MUTED),
                );
                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label("Width");
                        ui.add(
                            egui::DragValue::new(&mut self.dimensions[0])
                                .range(1..=30_000)
                                .suffix(" px")
                                .speed(1.0),
                        );
                    });
                    ui.add_space(15.0);
                    ui.vertical(|ui| {
                        ui.label("Height");
                        ui.add(
                            egui::DragValue::new(&mut self.dimensions[1])
                                .range(1..=30_000)
                                .suffix(" px")
                                .speed(1.0),
                        );
                    });
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.label("Resolution");
                    ui.add(
                        egui::DragValue::new(&mut self.resolution)
                            .range(1.0..=9600.0)
                            .suffix(" ppi"),
                    );
                });
                if dialog == Dialog::CanvasSize {
                    ui.add_space(12.0);
                    ui.label("Anchor");
                    egui::Grid::new("anchor_grid")
                        .spacing(vec2(3.0, 3.0))
                        .show(ui, |ui| {
                            for y in 0..3 {
                                for x in 0..3 {
                                    let anchor = [x as f32 * 0.5, y as f32 * 0.5];
                                    if ui
                                        .selectable_label(
                                            self.anchor == anchor,
                                            if self.anchor == anchor { "●" } else { "·" },
                                        )
                                        .clicked()
                                    {
                                        self.anchor = anchor;
                                    }
                                }
                                ui.end_row();
                            }
                        });
                }
                let valid = xuan::document::validate_size(self.dimensions[0], self.dimensions[1]);
                ui.add_space(12.0);
                if let Err(error) = &valid {
                    ui.colored_label(Color32::LIGHT_RED, error.to_string());
                } else {
                    ui.label(RichText::new("Transparent canvas · sRGB").color(theme::MUTED));
                }
                ui.add_space(15.0);
                ui.horizontal(|ui| {
                    cancel = ui.button("Cancel").clicked();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        apply = ui
                            .add_enabled(
                                valid.is_ok(),
                                egui::Button::new(if dialog == Dialog::New {
                                    "Create canvas"
                                } else {
                                    "Apply"
                                }),
                            )
                            .clicked();
                    });
                });
            });
        if apply {
            if dialog == Dialog::New {
                self.new_document();
            } else {
                let [width, height] = self.dimensions;
                let anchor = self.anchor;
                let resolution = self.resolution;
                self.edit(title, |doc| {
                    if dialog == Dialog::CanvasSize {
                        operations::canvas_size(doc, width, height, anchor)?;
                    } else {
                        operations::image_size(doc, width, height)?;
                    }
                    doc.resolution = resolution;
                    Ok(())
                });
                if let Some(s) = self.session_mut() {
                    s.fit = true;
                }
                self.dialog = None;
            }
        } else if cancel || !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.dialog = None;
        }
    }

    fn effect_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut edit) = self.effect.take() else {
            self.dialog = None;
            return;
        };
        let title = edit
            .adjustment
            .as_ref()
            .map(|a| a.name())
            .or_else(|| edit.filter.as_ref().map(|f| f.name()))
            .unwrap_or("Adjustment");
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        let mut changed = false;
        egui::Window::new(title)
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(440.0)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.add_space(8.0);
                if let Some(adjustment) = &mut edit.adjustment {
                    match adjustment {
                        Adjustment::HueRanges { settings } => {
                            egui::ComboBox::from_id_salt("hue_range")
                                .selected_text(xuan::color::HueSettings::RANGES[settings.range])
                                .show_ui(ui, |ui| {
                                    for (index, name) in
                                        xuan::color::HueSettings::RANGES.iter().enumerate()
                                    {
                                        changed |= ui
                                            .selectable_value(&mut settings.range, index, *name)
                                            .changed();
                                    }
                                });
                            let values = &mut settings.adjustments[settings.range];
                            changed |= ui
                                .add(
                                    egui::Slider::new(&mut values[0], -180.0..=360.0)
                                        .text("Hue")
                                        .suffix("°"),
                                )
                                .changed();
                            changed |= ui
                                .add(
                                    egui::Slider::new(&mut values[1], -100.0..=100.0)
                                        .text("Saturation"),
                                )
                                .changed();
                            changed |= ui
                                .add(
                                    egui::Slider::new(&mut values[2], -100.0..=100.0)
                                        .text("Lightness"),
                                )
                                .changed();
                            changed |= ui.checkbox(&mut settings.colorize, "Colorize").changed();
                            if settings.range > 0 {
                                changed |= ui
                                    .checkbox(
                                        &mut settings.invert_range,
                                        "Invert selected color range",
                                    )
                                    .changed();
                                ui.collapsing("Color range falloff", |ui| {
                                    for (index, label) in
                                        ["Falloff start", "Range start", "Range end", "Falloff end"]
                                            .iter()
                                            .enumerate()
                                    {
                                        changed |= ui
                                            .add(
                                                egui::Slider::new(
                                                    &mut settings.bands[settings.range][index],
                                                    0.0..=360.0,
                                                )
                                                .text(*label),
                                            )
                                            .changed();
                                    }
                                });
                            }
                        }
                        Adjustment::LevelsChannels { ranges } => {
                            channel_picker(ui, &mut edit.channel);
                            let source = render::render_scaled(&edit.original, 256, 192);
                            histogram(ui, &source);
                            let range = &mut ranges[edit.channel];
                            let maximum = range[2] - 1.0;
                            changed |= ui
                                .add(
                                    egui::Slider::new(&mut range[0], 0.0..=maximum)
                                        .text("Input black"),
                                )
                                .changed();
                            changed |= ui
                                .add(
                                    egui::Slider::new(&mut range[1], 0.1..=9.99)
                                        .text("Gamma")
                                        .logarithmic(true),
                                )
                                .changed();
                            let minimum = range[0] + 1.0;
                            changed |= ui
                                .add(
                                    egui::Slider::new(&mut range[2], minimum..=255.0)
                                        .text("Input white"),
                                )
                                .changed();
                            ui.separator();
                            changed |= ui
                                .add(
                                    egui::Slider::new(&mut range[3], 0.0..=255.0)
                                        .text("Output black"),
                                )
                                .changed();
                            changed |= ui
                                .add(
                                    egui::Slider::new(&mut range[4], 0.0..=255.0)
                                        .text("Output white"),
                                )
                                .changed();
                            if ui.button("Auto").clicked() {
                                if let Adjustment::Levels {
                                    black,
                                    gamma,
                                    white,
                                    output_black,
                                    output_white,
                                } = effects::auto_levels(&source)
                                {
                                    *range = [black, gamma, white, output_black, output_white];
                                }
                                changed = true;
                            }
                        }
                        Adjustment::CurvesChannels { channels } => {
                            channel_picker(ui, &mut edit.channel);
                            changed |= curve_editor(ui, &mut channels[edit.channel]);
                        }
                        Adjustment::HueSaturation {
                            hue,
                            saturation,
                            lightness,
                            colorize,
                        } => {
                            changed |= ui
                                .add(
                                    egui::Slider::new(hue, -180.0..=180.0)
                                        .text("Hue")
                                        .suffix("°"),
                                )
                                .changed();
                            changed |= ui
                                .add(
                                    egui::Slider::new(saturation, -100.0..=100.0)
                                        .text("Saturation"),
                                )
                                .changed();
                            changed |= ui
                                .add(egui::Slider::new(lightness, -100.0..=100.0).text("Lightness"))
                                .changed();
                            changed |= ui.checkbox(colorize, "Colorize").changed();
                        }
                        Adjustment::Levels {
                            black,
                            gamma,
                            white,
                            output_black,
                            output_white,
                        } => {
                            let source = render::render_scaled(
                                &edit.original,
                                320,
                                ((edit.original.height as f32 / edit.original.width as f32) * 320.0)
                                    .round()
                                    .max(1.0) as u32,
                            );
                            histogram(ui, &source);
                            ui.label("Input levels");
                            changed |= ui
                                .add(
                                    egui::Slider::new(black, 0.0..=(*white - 1.0).max(0.0))
                                        .text("Black"),
                                )
                                .changed();
                            changed |= ui
                                .add(
                                    egui::Slider::new(gamma, 0.1..=9.99)
                                        .text("Gamma")
                                        .logarithmic(true),
                                )
                                .changed();
                            changed |= ui
                                .add(
                                    egui::Slider::new(white, (*black + 1.0).min(255.0)..=255.0)
                                        .text("White"),
                                )
                                .changed();
                            ui.separator();
                            ui.label("Output levels");
                            changed |= ui
                                .add(egui::Slider::new(output_black, 0.0..=255.0).text("Black"))
                                .changed();
                            changed |= ui
                                .add(egui::Slider::new(output_white, 0.0..=255.0).text("White"))
                                .changed();
                            if ui.button("Auto").clicked() {
                                *adjustment = effects::auto_levels(&source);
                                changed = true;
                            }
                        }
                        Adjustment::Curves { points } => {
                            changed |= curve_editor(ui, points);
                        }
                        Adjustment::Exposure {
                            exposure,
                            offset,
                            gamma,
                        } => {
                            changed |= ui
                                .add(
                                    egui::Slider::new(exposure, -5.0..=5.0)
                                        .text("Exposure")
                                        .suffix(" EV"),
                                )
                                .changed();
                            changed |= ui
                                .add(egui::Slider::new(offset, -0.5..=0.5).text("Offset"))
                                .changed();
                            changed |= ui
                                .add(egui::Slider::new(gamma, 0.1..=5.0).text("Gamma"))
                                .changed();
                        }
                        Adjustment::GradientMap {
                            shadows,
                            highlights,
                        } => {
                            ui.horizontal(|ui| {
                                ui.label("Shadows");
                                changed |=
                                    ui.color_edit_button_srgba_unmultiplied(shadows).changed();
                                ui.label("Highlights");
                                changed |= ui
                                    .color_edit_button_srgba_unmultiplied(highlights)
                                    .changed();
                            });
                        }
                        Adjustment::Grain {
                            amount, monochrome, ..
                        } => {
                            changed |= ui
                                .add(
                                    egui::Slider::new(amount, 0.0..=100.0)
                                        .text("Amount")
                                        .suffix("%"),
                                )
                                .changed();
                            changed |= ui.checkbox(monochrome, "Monochromatic").changed();
                        }
                        Adjustment::Invert => {}
                    }
                }
                if let Some(filter) = &mut edit.filter {
                    match filter {
                        Filter::GaussianBlur { radius } => {
                            changed |= ui
                                .add(
                                    egui::Slider::new(radius, 0.1..=100.0)
                                        .text("Radius")
                                        .suffix(" px"),
                                )
                                .changed();
                        }
                        Filter::MotionBlur { distance, angle } => {
                            changed |= ui
                                .add(
                                    egui::Slider::new(distance, 1.0..=200.0)
                                        .text("Distance")
                                        .suffix(" px"),
                                )
                                .changed();
                            changed |= ui
                                .add(
                                    egui::Slider::new(angle, -180.0..=180.0)
                                        .text("Angle")
                                        .suffix("°"),
                                )
                                .changed();
                        }
                        Filter::Noise { amount, monochrome } => {
                            changed |= ui
                                .add(
                                    egui::Slider::new(amount, 0.0..=100.0)
                                        .text("Amount")
                                        .suffix("%"),
                                )
                                .changed();
                            changed |= ui.checkbox(monochrome, "Monochromatic").changed();
                        }
                        Filter::LensCorrection {
                            distortion,
                            vignette,
                        } => {
                            changed |= ui
                                .add(egui::Slider::new(distortion, -50.0..=50.0).text("Distortion"))
                                .changed();
                            changed |= ui
                                .add(egui::Slider::new(vignette, -100.0..=100.0).text("Vignette"))
                                .changed();
                        }
                    }
                }
                ui.add_space(14.0);
                ui.separator();
                ui.horizontal(|ui| {
                    changed |= ui.checkbox(&mut edit.preview, "Preview").changed();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        apply = ui.button("Apply").clicked();
                        cancel = ui.button("Cancel").clicked();
                    });
                });
                if edit.as_layer {
                    ui.label(
                        RichText::new("Non-destructive adjustment layer")
                            .small()
                            .color(theme::MUTED),
                    );
                }
            });
        if cancel || !open || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if let Some(s) = self.session_mut() {
                s.history.cancel(&mut s.document);
                s.invalidate();
            }
            self.dialog = None;
            return;
        }
        if changed || edit.refresh || apply {
            let mask_target = self.mask_target;
            if let Some(session) = self.session_mut() {
                session.document = edit.original.clone();
                if edit.preview || apply {
                    let result = if let Some(adjustment) = &edit.adjustment {
                        if let Some(target) = edit.target {
                            if let Some(layer) =
                                session.document.layers.iter_mut().find(|l| l.id == target)
                            {
                                layer.adjustment = Some(adjustment.clone());
                            }
                            Ok(())
                        } else if edit.as_layer {
                            let mut layer = Layer::blank(
                                adjustment.name(),
                                session.document.width,
                                session.document.height,
                            );
                            layer.adjustment = Some(adjustment.clone());
                            if session.document.selection.is_some() {
                                let mask =
                                    xuan::paint::mask_from_selection(&session.document, &layer);
                                layer.mask = Some(xuan::document::Mask {
                                    pixels: Arc::new(mask),
                                    ..xuan::document::Mask::white()
                                });
                            }
                            session.document.insert(layer);
                            Ok(())
                        } else {
                            effects::apply_adjustment(
                                &mut session.document,
                                adjustment,
                                mask_target,
                            )
                        }
                    } else if let Some(filter) = &edit.filter {
                        effects::apply_filter(&mut session.document, filter, mask_target)
                    } else {
                        Ok(())
                    };
                    if let Err(error) = result {
                        session.document = edit.original.clone();
                        session.history.cancel(&mut session.document);
                        session.invalidate();
                        self.error = Some(error.to_string());
                        self.dialog = None;
                        return;
                    }
                }
                session.invalidate();
            }
            edit.refresh = false;
        }
        if apply {
            if let Some(s) = self.session_mut() {
                s.history.commit();
            }
            self.dialog = None;
        } else {
            self.effect = Some(edit);
        }
    }

    fn export_dialog(&mut self, ctx: &egui::Context) {
        if self.sessions.is_empty() {
            self.dialog = None;
            return;
        }
        if self.export_changed {
            let doc = &self.session().unwrap().document;
            let factor = (700.0 / doc.width.max(doc.height) as f32).min(1.0);
            let image = render::render_scaled(
                doc,
                (doc.width as f32 * factor).max(1.0) as u32,
                (doc.height as f32 * factor).max(1.0) as u32,
            );
            let mut preview = image.clone();
            let mut bytes = Vec::new();
            if self.export_format == "jpg" {
                let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
                    &mut bytes,
                    self.jpeg_quality,
                );
                if encoder.encode_image(&render::flatten_white(&image)).is_ok()
                    && let Ok(decoded) = image::load_from_memory(&bytes)
                {
                    preview = decoded.to_rgba8();
                }
            } else {
                let _ = image::DynamicImage::ImageRgba8(image)
                    .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png);
            }
            self.export_bytes = bytes.len();
            self.export_texture = Some(ctx.load_texture(
                "export_preview",
                egui::ColorImage::from_rgba_unmultiplied(
                    [preview.width() as usize, preview.height() as usize],
                    preview.as_raw(),
                ),
                egui::TextureOptions::LINEAR,
            ));
            self.export_changed = false;
        }
        let mut open = true;
        let mut export = false;
        let mut cancel = false;
        egui::Window::new("Export image")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(650.0)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                if let Some(texture) = &self.export_texture {
                    let size = texture.size_vec2();
                    let factor = (600.0 / size.x).min(350.0 / size.y).min(1.0);
                    ui.image((texture.id(), size * factor));
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.label("Format");
                    egui::ComboBox::from_id_salt("export_format")
                        .selected_text(self.export_format.to_uppercase())
                        .show_ui(ui, |ui| {
                            for format in ["png", "jpg", "tiff", "webp"] {
                                self.export_changed |= ui
                                    .selectable_value(
                                        &mut self.export_format,
                                        format.into(),
                                        format.to_uppercase(),
                                    )
                                    .changed();
                            }
                        });
                    if self.export_format == "jpg" {
                        self.export_changed |= ui
                            .add(egui::Slider::new(&mut self.jpeg_quality, 1..=100).text("Quality"))
                            .changed();
                    }
                });
                if self.export_format == "jpg" {
                    ui.label(
                        RichText::new("JPEG preview · transparency is flattened onto white")
                            .small()
                            .color(theme::MUTED),
                    );
                }
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    cancel = ui.button("Cancel").clicked();
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        export = ui.button("Export…").clicked();
                    });
                });
            });
        if export {
            let session = self.session().unwrap();
            if let Some(path) = rfd::FileDialog::new()
                .add_filter(
                    self.export_format.to_uppercase(),
                    &[self.export_format.as_str()],
                )
                .set_file_name(format!("{}.{}", session.title, self.export_format))
                .save_file()
            {
                match io::export(&session.document, &path, self.jpeg_quality) {
                    Ok(()) => {
                        self.status = format!("Exported {}", path.display());
                        self.dialog = None;
                    }
                    Err(error) => self.error = Some(error.to_string()),
                }
            }
        }
        if !open || cancel {
            self.dialog = None;
        }
    }

    fn close_dialog(&mut self, ctx: &egui::Context) {
        if self.job.is_some() {
            return;
        }
        if let Some(index) = self.close_tab {
            if index >= self.sessions.len() {
                self.close_tab = None;
                return;
            }
            if !self.sessions[index].history.dirty() {
                self.sessions.remove(index);
                self.current = self.current.min(self.sessions.len().saturating_sub(1));
                self.close_tab = None;
                return;
            }
        }
        if self.close_tab.is_none() && !self.close_app {
            return;
        }
        let mut choice = None;
        egui::Window::new("Save your changes?")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(if self.close_app {
                    "Some projects have unsaved changes.".to_owned()
                } else {
                    format!(
                        "“{}” has unsaved changes.",
                        self.sessions[self.close_tab.unwrap()].title
                    )
                });
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        choice = Some(0);
                    }
                    if ui.button("Discard changes").clicked() {
                        choice = Some(1);
                    }
                    if ui.button("Save").clicked() {
                        choice = Some(2);
                    }
                });
            });
        match choice {
            Some(0) => {
                self.close_tab = None;
                self.close_app = false;
            }
            Some(1) | Some(2) => {
                let save = choice == Some(2);
                if self.close_app {
                    if save {
                        for index in 0..self.sessions.len() {
                            if self.sessions[index].history.dirty() {
                                self.current = index;
                                if !self.save_current(false) {
                                    return;
                                }
                            }
                        }
                    }
                    self.allow_close = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                } else if let Some(index) = self.close_tab {
                    if save {
                        self.current = index;
                        if !self.save_current(false) {
                            return;
                        }
                    }
                    self.sessions.remove(index);
                    self.current = self.current.min(self.sessions.len().saturating_sub(1));
                    self.close_tab = None;
                }
            }
            _ => {}
        }
    }
}

fn histogram(ui: &mut egui::Ui, image: &image::RgbaImage) {
    let bins = effects::histogram(image);
    let max = *bins.iter().max().unwrap_or(&1) as f32;
    let (rect, _) = ui.allocate_exact_size(vec2(360.0, 110.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 2.0, Color32::from_gray(27));
    for (i, count) in bins.into_iter().enumerate() {
        let x = rect.left() + i as f32 / 256.0 * rect.width();
        let height = (count as f32 / max.max(1.0)).sqrt() * rect.height();
        ui.painter().line_segment(
            [
                egui::pos2(x, rect.bottom()),
                egui::pos2(x, rect.bottom() - height),
            ],
            Stroke::new(rect.width() / 256.0, Color32::from_gray(177)),
        );
    }
}

fn channel_picker(ui: &mut egui::Ui, channel: &mut usize) {
    egui::ComboBox::from_id_salt("adjustment_channel")
        .selected_text(["RGB", "Red", "Green", "Blue"][*channel])
        .show_ui(ui, |ui| {
            for (index, name) in ["RGB", "Red", "Green", "Blue"].iter().enumerate() {
                ui.selectable_value(channel, index, *name);
            }
        });
}

fn curve_editor(ui: &mut egui::Ui, points: &mut Vec<Point>) -> bool {
    let mut changed = false;

    ui.label(
        RichText::new("Click to add a point · Drag points to reshape the curve")
            .color(theme::MUTED),
    );
    let (rect, response) =
        ui.allocate_exact_size(vec2(360.0, 220.0), egui::Sense::click_and_drag());
    ui.painter().rect_filled(rect, 4.0, Color32::from_gray(28));
    for i in 1..4 {
        let t = i as f32 / 4.0;
        ui.painter().line_segment(
            [
                rect.left_top() + vec2(rect.width() * t, 0.0),
                rect.left_bottom() + vec2(rect.width() * t, 0.0),
            ],
            Stroke::new(1.0_f32, Color32::from_gray(55)),
        );
        ui.painter().line_segment(
            [
                rect.left_top() + vec2(0.0, rect.height() * t),
                rect.right_top() + vec2(0.0, rect.height() * t),
            ],
            Stroke::new(1.0_f32, Color32::from_gray(55)),
        );
    }
    let map = |p: Point| {
        egui::pos2(
            rect.left() + p.x * rect.width(),
            rect.bottom() - p.y * rect.height(),
        )
    };
    ui.painter().line_segment(
        [rect.left_bottom(), rect.right_top()],
        Stroke::new(1.0_f32, Color32::from_gray(75)),
    );
    ui.painter().add(egui::Shape::line(
        (0..=255)
            .map(|i| {
                let x = i as f32 / 255.0;
                map(Point::new(x, effects::curve_value(points, x)))
            })
            .collect(),
        Stroke::new(1.5_f32, theme::TEXT),
    ));
    for p in points.iter() {
        ui.painter().circle_filled(map(*p), 3.0, theme::TEXT);
    }
    if let Some(p) = response.interact_pointer_pos() {
        let point = Point::new(
            ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0),
            ((rect.bottom() - p.y) / rect.height()).clamp(0.0, 1.0),
        );
        if response.clicked()
            && points.len() < 32
            && !points.iter().any(|v| (v.x - point.x).abs() < 0.025)
        {
            points.push(point);
            points.sort_by(|a, b| a.x.total_cmp(&b.x));
            changed = true;
        }
        if response.dragged()
            && let Some(index) = points
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| (a.x - point.x).abs().total_cmp(&(b.x - point.x).abs()))
                .map(|(i, _)| i)
        {
            points[index].y = point.y;
            changed = true;
        }
    }
    changed
}
