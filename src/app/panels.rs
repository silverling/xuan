use super::widgets;
use egui::RichText;
use xuan::{
    paint::{PaintMode, ShapeKind},
    selection::SelectionMode,
};

use super::{EditorApp, Tool, icons, theme};

const DEFAULT_VALUE_WIDTH: f32 = 74.0;
const DEFAULT_VALUE_HEIGHT: f32 = 22.0;
const DEFAULT_PERCENT_VALUE_WIDTH: f32 = 50.0;

/// Draw a toolbar field with optional `width = ...` and `height = ...` overrides.
macro_rules! value {
    (@dimension $default:expr) => { $default };
    (@dimension $default:expr, $custom:expr) => { $custom };
    (
        $ui:expr, $label:expr, $number:expr, $range:expr, $unit:expr
        $(, width = $width:expr)?
        $(, height = $height:expr)?
        $(,)?
    ) => {{
        let ui = &mut *($ui);
        ui.label(RichText::new($label).color(theme::MUTED));
        ui.add(
            widgets::Number::new($number)
                .size(egui::vec2(
                    value!(@dimension DEFAULT_VALUE_WIDTH $(, $width)?),
                    value!(@dimension DEFAULT_VALUE_HEIGHT $(, $height)?),
                ))
                .speed(1.0)
                .range($range)
                .suffix($unit)
                .max_decimals(1),
        )
        .changed()
    }};
}

impl EditorApp {
    pub(super) fn tool_options(&mut self, ctx: &egui::Context) {
        let mut transform = self
            .session()
            .and_then(|s| xuan::operations::transform_box(&s.document, self.transforming_mask()));
        let mut changed = false;
        egui::TopBottomPanel::top("tool_options")
            .min_height(42.0)
            .frame(theme::frame())
            .show(ctx, |ui| {
                ui.add_enabled_ui(self.dialog.is_none() && self.job.is_none(), |ui| {
                    egui::ScrollArea::horizontal()
                        .id_salt("options_scroll")
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(if self.transforming_mask() {
                                        "Mask"
                                    } else {
                                        self.tool.label()
                                    })
                                    .strong()
                                    .size(13.0),
                                );
                                ui.add_space(8.0);
                                match self.tool {
                                    Tool::Move => {
                                        widgets::checkbox(ui, &mut self.auto_select, "Auto Select");
                                        widgets::checkbox(
                                            ui,
                                            &mut self.ignore_transparent_pixels,
                                            "Ignore Transparent Pixels",
                                        )
                                        .on_hover_text(
                                            "Select layers only at visible pixels. Uncheck to select anywhere inside a layer's bounds.",
                                        );
                                        widgets::checkbox(
                                            ui,
                                            &mut self.show_controls,
                                            "Show Controls",
                                        );
                                        ui.separator();
                                        if let Some(t) = &mut transform {
                                            changed |= value!(
                                                ui,
                                                "X",
                                                &mut t.x,
                                                -1_000_000.0..=1_000_000.0,
                                                " px",
                                            );
                                            changed |= value!(
                                                ui,
                                                "Y",
                                                &mut t.y,
                                                -1_000_000.0..=1_000_000.0,
                                                " px",
                                            );
                                            let old = *t;
                                            if value!(ui, "W", &mut t.width, 1.0..=300_000.0, " px") {
                                                if self.lock_ratio {
                                                    t.height *= t.width / old.width;
                                                }
                                                changed = true;
                                            }
                                            if value!(ui, "H", &mut t.height, 1.0..=300_000.0, " px") {
                                                if self.lock_ratio {
                                                    t.width *= t.height / old.height;
                                                }
                                                changed = true;
                                            }
                                            widgets::checkbox(ui, &mut self.lock_ratio, "Link");
                                            changed |= value!(
                                                ui,
                                                "Angle",
                                                &mut t.rotation,
                                                -360.0..=360.0,
                                                "°",
                                                width = DEFAULT_PERCENT_VALUE_WIDTH,
                                            );
                                        } else {
                                            ui.label(
                                                RichText::new("Select a layer to transform")
                                                    .color(theme::MUTED),
                                            );
                                        }
                                    }
                                    tool if tool.is_brush() => {
                                        if matches!(self.tool, Tool::Brush | Tool::Erase) {
                                            let mut brush_tool = self.tool;
                                            if widgets::segmented(
                                                ui,
                                                &mut brush_tool,
                                                &[(Tool::Brush, "Paint"), (Tool::Erase, "Erase")],
                                            )
                                            .changed()
                                            {
                                                self.set_tool(brush_tool);
                                            }
                                        }
                                        if self.tool == Tool::Blur {
                                            widgets::segmented(
                                                ui,
                                                &mut self.blur_mode,
                                                &[
                                                    (PaintMode::Blur, "Blur"),
                                                    (PaintMode::Smudge, "Smudge"),
                                                ],
                                            );
                                        }
                                        if self.tool == Tool::Clone {
                                            widgets::checkbox(
                                                ui,
                                                &mut self.clone_aligned,
                                                "Aligned",
                                            );
                                            widgets::segmented(
                                                ui,
                                                &mut self.clone_all,
                                                &[(false, "This Layer"), (true, "All Layers")],
                                            );
                                        }
                                        value!(ui, "Size", &mut self.brush.diameter, 1.0..=2000.0, " px", width = 62.0);
                                        ui.label("Hardness");
                                        ui.add(
                                            widgets::Slider::new(
                                                &mut self.brush.hardness,
                                                0.0..=1.0,
                                            )
                                            .value_width(DEFAULT_PERCENT_VALUE_WIDTH)
                                            .percentage(),
                                        );
                                        ui.label("Opacity");
                                        ui.add(
                                            widgets::Slider::new(
                                                &mut self.brush.opacity,
                                                0.01..=1.0,
                                            )
                                            .value_width(DEFAULT_PERCENT_VALUE_WIDTH)
                                            .percentage(),
                                        );
                                        widgets::color_well(ui, &mut self.brush.color);
                                        ui.menu_button("Pen dynamics", |ui| {
                                            widgets::checkbox(ui, &mut self.pressure_size, "Pressure: size");
                                            widgets::checkbox(ui, &mut self.pressure_opacity, "Pressure: opacity");
                                            widgets::checkbox(ui, &mut self.tilt_shape, "Tilt: shape");
                                        });
                                        ui.label("Smoothing");
                                        ui.add(
                                            widgets::Slider::new(&mut self.brush_smoothing, 0.0..=1.0)
                                                .value_width(DEFAULT_PERCENT_VALUE_WIDTH)
                                                .percentage(),
                                        )
                                        .on_hover_text(
                                            "Reduce hand jitter. Higher values make the brush follow farther behind the pointer. 0% turns smoothing off.",
                                        );
                                    }
                                    tool if tool.is_selection() => {
                                        widgets::segmented(
                                            ui,
                                            &mut self.selection_mode,
                                            &[
                                                (SelectionMode::Replace, "New"),
                                                (SelectionMode::Add, "Add"),
                                                (SelectionMode::Subtract, "Subtract"),
                                                (SelectionMode::Intersect, "Intersect"),
                                            ],
                                        );
                                        ui.separator();
                                        match self.tool {
                                            Tool::Marquee => {
                                                widgets::segmented(
                                                    ui,
                                                    &mut self.ellipse,
                                                    &[(false, "Rectangle"), (true, "Ellipse")],
                                                );
                                            }
                                            Tool::Lasso => {
                                                widgets::segmented(
                                                    ui,
                                                    &mut self.polygonal,
                                                    &[(false, "Freehand"), (true, "Polygonal")],
                                                );
                                            }
                                            Tool::Wand => {
                                                ui.label("Tolerance");
                                                ui.add(
                                                    widgets::Number::new(&mut self.tolerance)
                                                        .size(egui::vec2(DEFAULT_PERCENT_VALUE_WIDTH, DEFAULT_VALUE_HEIGHT))
                                                        .range(0..=255),
                                                );
                                                widgets::checkbox(
                                                    ui,
                                                    &mut self.contiguous,
                                                    "Contiguous",
                                                );
                                            }
                                            _ => {}
                                        }
                                    }
                                    Tool::Gradient => {
                                        widgets::segmented(
                                            ui,
                                            &mut self.radial,
                                            &[(false, "Linear"), (true, "Radial")],
                                        );
                                        ui.separator();
                                        widgets::color_well(ui, &mut self.brush.color);
                                        ui.label("→");
                                        widgets::color_well(ui, &mut self.background);
                                        ui.label("Opacity");
                                        ui.add(
                                            widgets::Slider::new(
                                                &mut self.brush.opacity,
                                                0.0..=1.0,
                                            )
                                            .value_width(50.0)
                                            .percentage(),
                                        );
                                    }
                                    Tool::Shape => {
                                        widgets::segmented(
                                            ui,
                                            &mut self.shape_kind,
                                            &[
                                                (ShapeKind::Rectangle, "Rectangle"),
                                                (ShapeKind::RoundedRectangle, "Rounded"),
                                                (ShapeKind::Ellipse, "Ellipse"),
                                            ],
                                        );
                                        ui.separator();
                                        ui.label("Fill");
                                        widgets::color_well(ui, &mut self.brush.color);
                                        if self.shape_kind == ShapeKind::RoundedRectangle {
                                            value!(
                                                ui,
                                                "Radius",
                                                &mut self.corner_radius,
                                                0.0..=1000.0,
                                                " px",
                                            );
                                        }
                                    }
                                    Tool::Text => self.text_options(ui),
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
                                        widgets::color_well(ui, &mut self.brush.color);
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
            let mask_target = self.transforming_mask();
            self.edit_continuous("Transform", |doc| {
                xuan::operations::apply_transform(doc, transform, mask_target)
            });
        }
    }

    pub(super) fn status_bar(&mut self, ctx: &egui::Context) {
        super::chrome::status_bar(ctx, "status_bar").show(ctx, |ui| {
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
                    .inner_margin(egui::Margin::symmetric(10, 16)),
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
                            widgets::palette(ui, &mut self.brush.color, &mut self.background);
                        });
                });
            });
        if let Some(tool) = tool {
            self.set_tool(tool);
        }
    }
}
