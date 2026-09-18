use egui::RichText;
use xuan::{
    paint::{PaintMode, ShapeKind},
    selection::SelectionMode,
};

use super::{EditorApp, Tool, icons, theme};

fn value(
    ui: &mut egui::Ui,
    label: &str,
    number: &mut f32,
    range: std::ops::RangeInclusive<f32>,
) -> bool {
    ui.label(RichText::new(label).color(theme::MUTED));
    ui.add(
        egui::DragValue::new(number)
            .speed(1.0)
            .range(range)
            .max_decimals(1),
    )
    .changed()
}

impl EditorApp {
    pub(super) fn tool_options(&mut self, ctx: &egui::Context) {
        let mut transform = self
            .session()
            .and_then(|s| xuan::operations::transform_box(&s.document, self.mask_target));
        let mut changed = false;
        egui::TopBottomPanel::top("tool_options")
            .exact_height(42.0)
            .frame(theme::frame())
            .show(ctx, |ui| {
                ui.add_enabled_ui(self.dialog.is_none() && self.job.is_none(), |ui| {
                    egui::ScrollArea::horizontal()
                        .id_salt("options_scroll")
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(if self.mask_target {
                                        "Mask"
                                    } else {
                                        self.tool.label()
                                    })
                                    .strong(),
                                );
                                ui.add_space(8.0);
                                match self.tool {
                                    Tool::Move => {
                                        ui.checkbox(&mut self.auto_select, "Auto Select");
                                        ui.checkbox(&mut self.show_controls, "Show Controls");
                                        ui.separator();
                                        if let Some(t) = &mut transform {
                                            changed |= value(
                                                ui,
                                                "X",
                                                &mut t.x,
                                                -1_000_000.0..=1_000_000.0,
                                            );
                                            changed |= value(
                                                ui,
                                                "Y",
                                                &mut t.y,
                                                -1_000_000.0..=1_000_000.0,
                                            );
                                            let old = *t;
                                            if value(ui, "W", &mut t.width, 1.0..=300_000.0) {
                                                if self.lock_ratio {
                                                    t.height *= t.width / old.width;
                                                }
                                                changed = true;
                                            }
                                            if value(ui, "H", &mut t.height, 1.0..=300_000.0) {
                                                if self.lock_ratio {
                                                    t.width *= t.height / old.height;
                                                }
                                                changed = true;
                                            }
                                            ui.checkbox(&mut self.lock_ratio, "Link");
                                            changed |=
                                                value(ui, "Angle", &mut t.rotation, -360.0..=360.0);
                                            ui.label("°");
                                        } else {
                                            ui.label(
                                                RichText::new("Select a layer to transform")
                                                    .color(theme::MUTED),
                                            );
                                        }
                                    }
                                    tool if tool.is_brush() => {
                                        if self.tool == Tool::Blur {
                                            ui.selectable_value(
                                                &mut self.blur_mode,
                                                PaintMode::Blur,
                                                "Blur",
                                            );
                                            ui.selectable_value(
                                                &mut self.blur_mode,
                                                PaintMode::Smudge,
                                                "Smudge",
                                            );
                                        }
                                        if self.tool == Tool::Clone {
                                            ui.checkbox(&mut self.clone_aligned, "Aligned");
                                            ui.checkbox(&mut self.clone_all, "All Layers");
                                        }
                                        value(ui, "Size", &mut self.brush.diameter, 1.0..=2000.0);
                                        ui.label("px");
                                        ui.label("Hardness");
                                        ui.add(
                                            egui::Slider::new(&mut self.brush.hardness, 0.0..=1.0)
                                                .custom_formatter(|v, _| {
                                                    format!("{:.0}%", v * 100.0)
                                                }),
                                        );
                                        ui.label("Opacity");
                                        ui.add(
                                            egui::Slider::new(&mut self.brush.opacity, 0.01..=1.0)
                                                .custom_formatter(|v, _| {
                                                    format!("{:.0}%", v * 100.0)
                                                }),
                                        );
                                        ui.color_edit_button_srgba_unmultiplied(
                                            &mut self.brush.color,
                                        );
                                    }
                                    tool if tool.is_selection() => {
                                        for (mode, label) in [
                                            (SelectionMode::Replace, "New"),
                                            (SelectionMode::Add, "Add"),
                                            (SelectionMode::Subtract, "Subtract"),
                                            (SelectionMode::Intersect, "Intersect"),
                                        ] {
                                            ui.selectable_value(
                                                &mut self.selection_mode,
                                                mode,
                                                label,
                                            );
                                        }
                                        ui.separator();
                                        match self.tool {
                                            Tool::Marquee => {
                                                ui.selectable_value(
                                                    &mut self.ellipse,
                                                    false,
                                                    "Rectangle",
                                                );
                                                ui.selectable_value(
                                                    &mut self.ellipse,
                                                    true,
                                                    "Ellipse",
                                                );
                                            }
                                            Tool::Lasso => {
                                                ui.selectable_value(
                                                    &mut self.polygonal,
                                                    false,
                                                    "Freehand",
                                                );
                                                ui.selectable_value(
                                                    &mut self.polygonal,
                                                    true,
                                                    "Polygonal",
                                                );
                                            }
                                            Tool::Wand => {
                                                ui.label("Tolerance");
                                                ui.add(
                                                    egui::DragValue::new(&mut self.tolerance)
                                                        .range(0..=255),
                                                );
                                                ui.checkbox(&mut self.contiguous, "Contiguous");
                                            }
                                            _ => {}
                                        }
                                    }
                                    Tool::Gradient => {
                                        ui.selectable_value(&mut self.radial, false, "Linear");
                                        ui.selectable_value(&mut self.radial, true, "Radial");
                                        ui.separator();
                                        ui.color_edit_button_srgba_unmultiplied(
                                            &mut self.brush.color,
                                        );
                                        ui.label("→");
                                        ui.color_edit_button_srgba_unmultiplied(
                                            &mut self.background,
                                        );
                                        ui.label("Opacity");
                                        ui.add(
                                            egui::Slider::new(&mut self.brush.opacity, 0.0..=1.0)
                                                .custom_formatter(|v, _| {
                                                    format!("{:.0}%", v * 100.0)
                                                }),
                                        );
                                    }
                                    Tool::Shape => {
                                        ui.selectable_value(
                                            &mut self.shape_kind,
                                            ShapeKind::Rectangle,
                                            "Rectangle",
                                        );
                                        ui.selectable_value(
                                            &mut self.shape_kind,
                                            ShapeKind::RoundedRectangle,
                                            "Rounded",
                                        );
                                        ui.selectable_value(
                                            &mut self.shape_kind,
                                            ShapeKind::Ellipse,
                                            "Ellipse",
                                        );
                                        ui.separator();
                                        ui.label("Fill");
                                        ui.color_edit_button_srgba_unmultiplied(
                                            &mut self.brush.color,
                                        );
                                        if self.shape_kind == ShapeKind::RoundedRectangle {
                                            value(
                                                ui,
                                                "Radius",
                                                &mut self.corner_radius,
                                                0.0..=1000.0,
                                            );
                                            ui.label("px");
                                        }
                                    }
                                    Tool::Crop => {
                                        ui.label(
                                            RichText::new(
                                                "Drag a crop area, then press Enter to apply",
                                            )
                                            .color(theme::MUTED),
                                        );
                                    }
                                    Tool::Dropper => {
                                        ui.label("Sample: All visible layers");
                                        ui.color_edit_button_srgba_unmultiplied(
                                            &mut self.brush.color,
                                        );
                                    }
                                    Tool::Hand | Tool::Zoom => {
                                        ui.label(
                                        RichText::new(
                                            "Scroll to zoom · Space-drag to pan · Ctrl+0 to fit",
                                        )
                                        .color(theme::MUTED),
                                    );
                                    }
                                    _ => {}
                                }
                            });
                        });
                });
            });
        if changed && let Some(transform) = transform {
            let mask_target = self.mask_target;
            self.edit_continuous("Transform", |doc| {
                xuan::operations::apply_transform(doc, transform, mask_target)
            });
        }
    }

    pub(super) fn status_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::bottom("status_bar")
            .exact_height(30.0)
            .frame(theme::frame())
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(session) = self.session() {
                        ui.label(
                            RichText::new(format!("{:.1}%", session.zoom * 100.0))
                                .size(11.0)
                                .color(theme::MUTED),
                        );
                        ui.separator();
                        ui.label(
                            RichText::new(format!(
                                "{} × {} px",
                                session.document.width, session.document.height
                            ))
                            .size(11.0)
                            .color(theme::MUTED),
                        );
                        ui.separator();
                        ui.label(
                            RichText::new("sRGB · Transparent")
                                .size(11.0)
                                .color(theme::MUTED),
                        );
                    } else {
                        ui.label(
                            RichText::new("Ready when you are")
                                .size(11.0)
                                .color(theme::MUTED),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add(
                            egui::Label::new(
                                RichText::new(self.tool.hint())
                                    .size(11.0)
                                    .color(theme::MUTED),
                            )
                            .truncate(),
                        );
                    });
                });
            });
    }

    pub(super) fn tool_rail(&mut self, ctx: &egui::Context) {
        let mut tool = None;
        egui::SidePanel::left("tools")
            .exact_width(56.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(10, 12)),
            )
            .show(ctx, |ui| {
                ui.add_enabled_ui(self.dialog.is_none() && self.job.is_none(), |ui| {
                    egui::ScrollArea::vertical()
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 5.0;
                            for t in Tool::ALL {
                                if icons::tool_button(ui, t, self.tool == t).clicked() {
                                    tool = Some(t);
                                }
                            }
                            ui.add_space(8.0);
                            ui.separator();
                            ui.add_space(5.0);
                            ui.color_edit_button_srgba_unmultiplied(&mut self.brush.color)
                                .on_hover_text("Foreground color");
                            ui.color_edit_button_srgba_unmultiplied(&mut self.background)
                                .on_hover_text("Background color");
                            if ui
                                .small_button("⇄")
                                .on_hover_text("Swap colors (X)")
                                .clicked()
                            {
                                std::mem::swap(&mut self.brush.color, &mut self.background);
                            }
                        });
                });
            });
        if let Some(tool) = tool {
            self.set_tool(tool);
        }
    }
}
