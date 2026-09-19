use egui::{Button, RichText};
use xuan::{
    document::{Adjustment, Point},
    effects::Filter,
};

use super::{EditorApp, theme, widgets};

fn item(
    ui: &mut egui::Ui,
    label: &str,
    shortcut: &str,
    command: &'static str,
    action: &mut Option<&'static str>,
) {
    if ui.add(Button::new(label).shortcut_text(shortcut)).clicked() {
        *action = Some(command);
        ui.close();
    }
}

pub(super) fn adjustment_menu(ui: &mut egui::Ui) -> Option<Adjustment> {
    let mut result = None;
    for adjustment in [
        Adjustment::HueRanges {
            settings: Box::default(),
        },
        Adjustment::LevelsChannels {
            ranges: [xuan::color::DEFAULT_LEVELS; 4],
        },
        Adjustment::CurvesChannels {
            channels: std::array::from_fn(|_| vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)]),
        },
        Adjustment::Exposure {
            exposure: 0.0,
            offset: 0.0,
            gamma: 1.0,
        },
        Adjustment::GradientMap {
            shadows: [0, 0, 0, 255],
            highlights: [255; 4],
        },
        Adjustment::FilmGrain {
            amount: 10.0,
            size: 1.0,
            roughness: 50.0,
            seed: 1,
        },
    ] {
        if ui.button(adjustment.name()).clicked() {
            result = Some(adjustment);
            ui.close();
        }
    }
    result
}

impl EditorApp {
    pub(super) fn menus(&mut self, ctx: &egui::Context) {
        let mut action = None;
        let mut adjustment = None;
        let mut filter = None;
        let mut adjustment_layer = false;
        let has_doc = self.session().is_some();
        let blocked = self.job.is_some()
            || self.dialog.is_some()
            || self.close_app
            || self.close_tab.is_some();
        egui::TopBottomPanel::top("menubar")
            .exact_height(40.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::TITLEBAR)
                    .corner_radius(egui::CornerRadius {
                        nw: theme::window_corner_radius(ctx),
                        ne: theme::window_corner_radius(ctx),
                        sw: 0,
                        se: 0,
                    })
                    .inner_margin(egui::Margin::symmetric(14, 9)),
            )
            .show(ctx, |ui| {
                egui::MenuBar::new()
                    .config(egui::containers::menu::MenuConfig::new().style(theme::menu_style))
                    .ui(ui, |ui| {
                        self.window_controls(ui);
                        ui.add_enabled_ui(!blocked, |ui| {
                            ui.menu_button("File", |ui| {
                                item(ui, "New Canvas…", "Ctrl+N", "new", &mut action);
                                item(ui, "Open…", "Ctrl+O", "open", &mut action);
                                item(ui, "Open Compositor Package…", "", "open_comp", &mut action);
                                item(
                                    ui,
                                    "Import Image as Layer…",
                                    "Ctrl+Shift+O",
                                    "import",
                                    &mut action,
                                );
                                ui.separator();
                                ui.add_enabled_ui(has_doc, |ui| {
                                    item(ui, "Save", "Ctrl+S", "save", &mut action);
                                    item(ui, "Save As…", "Ctrl+Shift+S", "save_as", &mut action);
                                    item(
                                        ui,
                                        "Export Image…",
                                        "Ctrl+Alt+Shift+S",
                                        "export",
                                        &mut action,
                                    );
                                    ui.separator();
                                    item(ui, "Close Project", "Ctrl+W", "close", &mut action);
                                });
                            });
                            ui.menu_button("Edit", |ui| {
                                let undo = self.session().and_then(|s| s.history.undo_name());
                                let redo = self.session().and_then(|s| s.history.redo_name());
                                ui.add_enabled_ui(undo.is_some(), |ui| {
                                    item(
                                        ui,
                                        &format!("Undo {}", undo.unwrap_or("")),
                                        "Ctrl+Z",
                                        "undo",
                                        &mut action,
                                    )
                                });
                                ui.add_enabled_ui(redo.is_some(), |ui| {
                                    item(
                                        ui,
                                        &format!("Redo {}", redo.unwrap_or("")),
                                        "Ctrl+Shift+Z",
                                        "redo",
                                        &mut action,
                                    )
                                });
                                ui.separator();
                                ui.add_enabled_ui(has_doc, |ui| {
                                    item(ui, "Cut", "Ctrl+X", "cut", &mut action);
                                    item(ui, "Copy", "Ctrl+C", "copy", &mut action);
                                    item(
                                        ui,
                                        "Copy Merged",
                                        "Ctrl+Shift+C",
                                        "copy_merged",
                                        &mut action,
                                    );
                                });
                                item(ui, "Paste", "Ctrl+V", "paste", &mut action);
                                ui.add_enabled_ui(has_doc, |ui| {
                                    ui.separator();
                                    item(
                                        ui,
                                        "Fill Foreground",
                                        "Alt+Backspace",
                                        "fill_fg",
                                        &mut action,
                                    );
                                    item(
                                        ui,
                                        "Fill Background",
                                        "Ctrl+Backspace",
                                        "fill_bg",
                                        &mut action,
                                    );
                                    item(ui, "Clear Pixels", "Delete", "clear", &mut action);
                                    item(
                                        ui,
                                        "Content-Aware Fill",
                                        "Shift+F5",
                                        "content_fill",
                                        &mut action,
                                    );
                                });
                            });
                            ui.menu_button("Image", |ui| {
                                ui.add_enabled_ui(has_doc, |ui| {
                                    ui.menu_button("Adjustments", |ui| {
                                        adjustment = adjustment_menu(ui);
                                        ui.separator();
                                        item(ui, "Invert", "Ctrl+I", "invert", &mut action);
                                    });
                                    ui.separator();
                                    item(ui, "Image Size…", "", "image_size", &mut action);
                                    item(ui, "Canvas Size…", "", "canvas_size", &mut action);
                                    ui.separator();
                                    item(
                                        ui,
                                        "Flip Canvas Horizontal",
                                        "",
                                        "flip_canvas_h",
                                        &mut action,
                                    );
                                    item(
                                        ui,
                                        "Flip Canvas Vertical",
                                        "",
                                        "flip_canvas_v",
                                        &mut action,
                                    );
                                });
                            });
                            ui.menu_button("Layer", |ui| {
                                ui.add_enabled_ui(has_doc, |ui| {
                                    item(ui, "New Layer", "Ctrl+Shift+N", "new_layer", &mut action);
                                    item(
                                        ui,
                                        "Duplicate Layers",
                                        "Ctrl+J",
                                        "duplicate",
                                        &mut action,
                                    );
                                    item(ui, "Delete Layers", "", "delete_layer", &mut action);
                                    ui.separator();
                                    item(ui, "Group Layers", "Ctrl+G", "group", &mut action);
                                    item(
                                        ui,
                                        "Ungroup Layers",
                                        "Ctrl+Shift+G",
                                        "ungroup",
                                        &mut action,
                                    );
                                    item(
                                        ui,
                                        "Merge Down / Selected",
                                        "Ctrl+E",
                                        "merge",
                                        &mut action,
                                    );
                                    item(ui, "Flatten Image", "", "flatten", &mut action);
                                    ui.separator();
                                    ui.menu_button("New Adjustment Layer", |ui| {
                                        adjustment = adjustment_menu(ui);
                                        adjustment_layer = true;
                                    });
                                    ui.menu_button("Layer Mask", |ui| {
                                        item(
                                            ui,
                                            "Add Mask from Selection",
                                            "",
                                            "mask",
                                            &mut action,
                                        );
                                        item(
                                            ui,
                                            "Enable / Disable",
                                            "",
                                            "disable_mask",
                                            &mut action,
                                        );
                                        item(ui, "Link / Unlink", "", "link_mask", &mut action);
                                        item(ui, "Delete Mask", "", "delete_mask", &mut action);
                                    });
                                    item(
                                        ui,
                                        "Create / Release Clipping Mask",
                                        "Ctrl+Alt+G",
                                        "clip",
                                        &mut action,
                                    );
                                    ui.separator();
                                    item(ui, "Flip Horizontal", "", "flip_h", &mut action);
                                    item(ui, "Flip Vertical", "", "flip_v", &mut action);
                                });
                            });
                            ui.menu_button("Select", |ui| {
                                ui.add_enabled_ui(has_doc, |ui| {
                                    item(ui, "All", "Ctrl+A", "select_all", &mut action);
                                    item(ui, "Deselect", "Ctrl+D", "deselect", &mut action);
                                    item(
                                        ui,
                                        "Inverse",
                                        "Ctrl+Shift+I",
                                        "invert_selection",
                                        &mut action,
                                    );
                                    item(
                                        ui,
                                        "Load Layer / Mask",
                                        "",
                                        "load_selection",
                                        &mut action,
                                    );
                                    item(ui, "Feather 3 px", "", "feather", &mut action);
                                });
                            });
                            ui.menu_button("Filter", |ui| {
                                ui.add_enabled_ui(has_doc, |ui| {
                                    item(
                                        ui,
                                        "Remove Background (edge colors)",
                                        "",
                                        "remove_background",
                                        &mut action,
                                    );
                                    ui.separator();
                                    for f in [
                                        Filter::GaussianBlur { radius: 4.0 },
                                        Filter::MotionBlur {
                                            distance: 15.0,
                                            angle: 0.0,
                                        },
                                        Filter::Noise {
                                            amount: 10.0,
                                            monochrome: true,
                                        },
                                        Filter::LensCorrection {
                                            distortion: 0.0,
                                            vignette: 0.0,
                                        },
                                    ] {
                                        if ui.button(format!("{}…", f.name())).clicked() {
                                            filter = Some(f);
                                            ui.close();
                                        }
                                    }
                                });
                            });
                            ui.menu_button("View", |ui| {
                                item(ui, "Fit Canvas", "Ctrl+0", "fit", &mut action);
                                item(ui, "Actual Pixels", "Ctrl+1", "actual", &mut action);
                                item(ui, "Zoom In", "Ctrl++", "zoom_in", &mut action);
                                item(ui, "Zoom Out", "Ctrl+−", "zoom_out", &mut action);
                                ui.separator();
                                widgets::checkbox(
                                    ui,
                                    &mut self.show_controls,
                                    "Show Transform Controls",
                                );
                                widgets::checkbox(ui, &mut self.snap, "Snap to Canvas and Layers");
                            });
                            ui.menu_button("Help", |ui| {
                                item(ui, "Keyboard Shortcuts", "F1", "shortcuts", &mut action);
                                item(ui, "About Xuan", "", "about", &mut action);
                            });
                        });
                        self.titlebar_drag(ui);
                    });
            });
        if let Some(action) = action {
            self.command(action);
        }
        if let Some(adjustment) = adjustment {
            self.start_adjustment(adjustment, adjustment_layer);
        }
        if let Some(filter) = filter {
            self.start_filter(filter);
        }
    }

    pub(super) fn tabs(&mut self, ctx: &egui::Context) {
        let mut action = None;
        let mut switch = None;
        let mut copy = None;
        egui::TopBottomPanel::top("project_tabs")
            .exact_height(46.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::TITLEBAR)
                    .inner_margin(egui::Margin::symmetric(14, 9)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .add(widgets::Button::new("+").min_size(egui::vec2(28.0, 28.0)))
                        .on_hover_text("New canvas (Ctrl+N)")
                        .clicked()
                    {
                        action = Some("new");
                    }
                    ui.separator();
                    let available = (ui.available_width() - 228.0).max(150.0);
                    ui.allocate_ui(egui::vec2(available, 28.0), |ui| {
                        egui::ScrollArea::horizontal()
                            .id_salt("tabs_scroll")
                            .scroll_bar_visibility(
                                egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                            )
                            .show(ui, |ui| {
                                ui.horizontal_centered(|ui| {
                                    if self.sessions.is_empty() {
                                        ui.label(RichText::new("Untitled").color(theme::MUTED));
                                    }
                                    for (index, session) in self.sessions.iter().enumerate() {
                                        let (response, close) = widgets::project_tab(
                                            ui,
                                            &session.title,
                                            session.history.dirty(),
                                            self.current == index,
                                        );
                                        if let Some(layer) =
                                            response.dnd_release_payload::<super::LayerDrag>()
                                        {
                                            copy = Some((*layer, index));
                                        }
                                        if response
                                            .dnd_hover_payload::<super::LayerDrag>()
                                            .is_some()
                                        {
                                            ui.painter().rect_stroke(
                                                response.rect,
                                                theme::BUTTON_RADIUS,
                                                egui::Stroke::new(2.0_f32, theme::ACCENT),
                                                egui::StrokeKind::Inside,
                                            );
                                        }
                                        if response.clicked() {
                                            switch = Some(index);
                                        }
                                        if (close
                                            || response.clicked_by(egui::PointerButton::Middle))
                                            && self.job.is_none()
                                            && self.dialog.is_none()
                                        {
                                            self.close_tab = Some(index);
                                        }
                                    }
                                });
                            });
                    });
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if widgets::button(ui, "−").on_hover_text("Zoom out").clicked() {
                            action = Some("zoom_out");
                        }
                        if widgets::button(ui, "+").on_hover_text("Zoom in").clicked() {
                            action = Some("zoom_in");
                        }
                        if widgets::button(ui, "100%").clicked() {
                            action = Some("actual");
                        }
                        if widgets::button(ui, "Fit").clicked() {
                            action = Some("fit");
                        }
                    });
                });
            });
        if let Some((layer, destination)) = copy {
            self.copy_layer_to_project(layer, destination);
        }
        if let Some(index) = switch
            && self.dialog.is_none()
        {
            self.cancel_gesture();
            self.current = index;
            self.mask_target = false;
        }
        if let Some(action) = action
            && self.dialog.is_none()
        {
            self.command(action);
        }
    }
}
