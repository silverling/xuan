use super::widgets;
use std::sync::{Arc, Weak};

use egui::{Color32, RichText, Sense, Stroke, StrokeKind, TextureOptions, vec2};
use uuid::Uuid;
use xuan::{
    blend::BlendMode,
    document::{Adjustment, Document, Layer},
};

use super::{EditorApp, LayerDrag, icons, menus, theme};

pub(super) struct LayerRename {
    project: Uuid,
    layer: Uuid,
    pub(super) name: String,
    focus: bool,
}

impl LayerRename {
    fn input_id(&self) -> egui::Id {
        egui::Id::new(("layer_name", self.project, self.layer))
    }
}

pub(super) struct LayerThumbnail {
    texture: egui::TextureHandle,
    canvas: [u32; 2],
    transform: xuan::document::Transform,
    // Weak references identify assets without copying a large image on every brush dab.
    pixels: Option<Weak<image::RgbaImage>>,
    mask: Option<Weak<image::GrayImage>>,
}

impl LayerThumbnail {
    pub(super) fn id(&self) -> egui::TextureId {
        self.texture.id()
    }

    fn matches(&self, document: &Document, layer: &Layer, mask: bool) -> bool {
        if mask {
            self.mask.as_ref().map(Weak::as_ptr)
                == layer.mask.as_ref().map(|mask| Arc::as_ptr(&mask.pixels))
        } else {
            self.canvas == [document.width, document.height]
                && self.transform == layer.transform
                && self.pixels.as_ref().map(Weak::as_ptr) == layer.pixels.as_ref().map(Arc::as_ptr)
        }
    }
}

#[derive(Default)]
struct Actions {
    command: Option<&'static str>,
    adjustment: Option<Adjustment>,
    filter: Option<xuan::effects::Filter>,
    visibility: Option<Uuid>,
    select: Option<(Uuid, bool)>,
    deselect: bool,
    collapse: Option<Uuid>,
    reorder: Option<(LayerDrag, Uuid, DropPosition, bool)>,
    drop_indicator: Option<egui::Shape>,
    appearance: Option<(BlendMode, f32, bool)>,
    rename: Option<Uuid>,
    finish_rename: Option<bool>,
    edit_adjustment: Option<Uuid>,
    edit_filter: Option<Uuid>,
    edit_text: Option<Uuid>,
    edit_raw: Option<Uuid>,
}

#[derive(Clone, Copy)]
enum DropPosition {
    Above,
    Below,
    Inside,
}

impl DropPosition {
    fn at(rect: egui::Rect, y: f32, group: bool) -> Self {
        if group
            && (rect.top() + rect.height() * 0.25..=rect.bottom() - rect.height() * 0.25)
                .contains(&y)
        {
            Self::Inside
        } else if y < rect.center().y {
            Self::Above
        } else {
            Self::Below
        }
    }
}

fn rows(document: &Document, collapsed: &std::collections::HashSet<Uuid>) -> Vec<(Layer, usize)> {
    fn visit(
        document: &Document,
        collapsed: &std::collections::HashSet<Uuid>,
        parent: Option<Uuid>,
        depth: usize,
        output: &mut Vec<(Layer, usize)>,
    ) {
        if depth > 64 {
            return;
        }
        for layer in document.layers.iter().rev().filter(|l| l.parent == parent) {
            output.push((layer.clone(), depth));
            if !collapsed.contains(&layer.id) {
                visit(document, collapsed, Some(layer.id), depth + 1, output);
            }
        }
    }
    let mut output = Vec::new();
    visit(document, collapsed, None, 0, &mut output);
    output
}

impl EditorApp {
    pub(super) fn layers_panel(&mut self, ctx: &egui::Context) {
        if let Some(rename) = &self.rename
            && self.session().is_none_or(|session| {
                session.document.id != rename.project
                    || !session.document.layers.iter().any(|l| l.id == rename.layer)
            })
        {
            self.finish_layer_rename(ctx, false);
        }
        let mut actions = Actions::default();
        egui::SidePanel::right("layers_panel")
            .default_width(252.0)
            .width_range(202.0..=352.0)
            .resizable(true)
            .frame(egui::Frame::new().fill(theme::PANEL))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.add_enabled_ui(self.dialog.is_none() && self.job.is_none(), |ui| {
                    self.layer_controls(ui, &mut actions);
                    ui.separator();
                    let height = (ui.available_height() - 40.0).max(40.0);
                    egui::ScrollArea::vertical()
                        .id_salt("layers_scroll")
                        .max_height(height)
                        .min_scrolled_height(height)
                        .auto_shrink([false, false])
                        .show_viewport(ui, |ui, viewport| {
                            // Register the background before rows so their controls take priority.
                            let background = ui.interact(
                                viewport.translate(ui.max_rect().min.to_vec2()),
                                ui.id().with("background"),
                                Sense::click(),
                            );
                            actions.deselect = background.clicked();
                            if let Some(session) = self.session() {
                                // Adjacent rows share a single insertion boundary.
                                ui.spacing_mut().item_spacing.y = 0.0;
                                for (layer, depth) in rows(&session.document, &session.collapsed) {
                                    self.layer_row(ui, &layer, depth, &mut actions);
                                }
                            } else {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.add_space((height * 0.5 - 48.0).max(10.0));
                                ui.vertical_centered(|ui| {
                                    ui.label(RichText::new("No layers yet").color(theme::MUTED));
                                    ui.label(
                                        RichText::new("Create a canvas or import an image.")
                                            .size(11.0)
                                            .color(theme::MUTED),
                                    );
                                });
                            }
                            // Paint last so the next row cannot cover half of the line.
                            if let Some(indicator) = actions.drop_indicator.take() {
                                ui.painter().add(indicator);
                            }
                        });
                    ui.separator();
                    self.layer_footer(ui, &mut actions);
                });
            });
        self.apply_layer_actions(ctx, actions);
    }

    fn layer_controls(&self, ui: &mut egui::Ui, actions: &mut Actions) {
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Layers").strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let count = self.session().map_or(0, |s| s.document.layers.len());
                        ui.label(RichText::new(count.to_string()).color(theme::MUTED).small());
                    });
                });
            });
        ui.separator();
        let active = self.session().and_then(|s| s.document.active());
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(12, 12))
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                ui.add_enabled_ui(active.is_some_and(|layer| !layer.group), |ui| {
                    let mut blend = active.map_or(BlendMode::Normal, |l| l.blend);
                    let mut opacity = active.map_or(1.0, |l| l.opacity);
                    let mut locked = active.is_some_and(|l| l.locked);
                    let mut changed = false;
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Blend").size(11.0));
                        ui.add_enabled_ui(active.is_none_or(|l| !l.standalone_mask), |ui| {
                            widgets::PopUp::from_id_salt("blend_mode")
                                .width((ui.available_width() - 24.0).max(80.0))
                                .selected_text(blend.name())
                                .show_ui(ui, |ui| {
                                    for mode in BlendMode::ALL {
                                        changed |=
                                            widgets::menu_choice(ui, &mut blend, mode, mode.name())
                                                .changed();
                                    }
                                });
                        });
                        if icons::lock(ui, locked).clicked() {
                            locked = !locked;
                            changed = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Opacity").size(11.0));
                        ui.spacing_mut().slider_width =
                            (ui.available_width() - 50.0 - widgets::SLIDER_SPACING).max(40.0);
                        changed |= ui
                            .add(
                                widgets::Slider::new(&mut opacity, 0.0..=1.0)
                                    .value_width(50.0)
                                    .percentage(),
                            )
                            .changed();
                    });
                    if changed {
                        actions.appearance = Some((blend, opacity, locked));
                    }
                });
            });
    }

    fn layer_row(&mut self, ui: &mut egui::Ui, layer: &Layer, depth: usize, actions: &mut Actions) {
        let session = &self.sessions[self.current];
        let selected = session.document.selected.contains(&layer.id);
        let expandable = layer.group
            || session
                .document
                .layers
                .iter()
                .any(|l| l.parent == Some(layer.id));
        let attached = layer.parent.is_some_and(|id| {
            session
                .document
                .layers
                .iter()
                .any(|l| l.id == id && l.can_attach_effects())
        });
        let project = session.document.id;
        let width = ui.available_width();
        let renaming = self
            .rename
            .as_ref()
            .is_some_and(|edit| edit.layer == layer.id);
        let mut name_rect = egui::Rect::NOTHING;
        // Register the row behind its controls so it cannot steal their clicks.
        let row = ui.scope_builder(
            egui::UiBuilder::new()
                .id_salt(layer.id)
                .sense(Sense::click_and_drag()),
            |ui| {
                ui.style_mut().interaction.selectable_labels = false;
                egui::Frame::new()
                    .fill(if selected {
                        Color32::from_gray(57)
                    } else {
                        theme::PANEL
                    })
                    .inner_margin(egui::Margin::symmetric(8, 8))
                    .show(ui, |ui| {
                        ui.set_min_width((width - 16.0).max(0.0));
                        ui.allocate_ui_with_layout(
                            vec2(ui.available_width(), 36.0),
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.set_min_height(36.0);
                                ui.spacing_mut().item_spacing.x = 6.0;
                                if icons::eye(ui, layer.visible).clicked() {
                                    actions.visibility = Some(layer.id);
                                }
                                ui.with_layout(
                                    egui::Layout::left_to_right(egui::Align::Center),
                                    |ui| {
                                        ui.add_space((depth as f32 * 24.0).min(72.0));
                                        if expandable {
                                            let collapsed = self.sessions[self.current]
                                                .collapsed
                                                .contains(&layer.id);
                                            if icons::disclosure(ui, collapsed).clicked() {
                                                actions.collapse = Some(layer.id);
                                            }
                                        }
                                        if layer.group {
                                            if icons::action_button(ui, "group").clicked() {
                                                actions.select = Some((layer.id, false));
                                            }
                                        } else if !layer.standalone_mask {
                                            if layer.clip_to.is_some() {
                                                ui.label(
                                                    RichText::new("↳").small().color(theme::MUTED),
                                                );
                                            }
                                            self.layer_thumbnail(
                                                ui, layer, false, selected, actions,
                                            );
                                        }
                                        if layer.mask.is_some() {
                                            self.layer_thumbnail(
                                                ui, layer, true, selected, actions,
                                            );
                                        }
                                        let color = if layer.visible {
                                            theme::TEXT
                                        } else {
                                            theme::MUTED
                                        };
                                        ui.vertical(|ui| {
                                            ui.spacing_mut().item_spacing.y = 3.0;
                                            name_rect =
                                                self.layer_name(ui, layer, color, actions).rect;
                                            let detail = if layer.group {
                                                "Folder".to_owned()
                                            } else if layer.standalone_mask {
                                                if attached {
                                                    "Mask · Image only"
                                                } else {
                                                    "Mask · Layers below"
                                                }
                                                .to_owned()
                                            } else if let Some(adjustment) = &layer.adjustment {
                                                adjustment.name().to_owned()
                                            } else if let Some(filter) = &layer.filter {
                                                filter.name().to_owned()
                                            } else if layer.raw.is_some() {
                                                "RAW · Embedded · Double-click to develop"
                                                    .to_owned()
                                            } else if let Some(text) = &layer.text {
                                                format!(
                                                    "Text · {} · {:.0} px",
                                                    text.family, text.size
                                                )
                                            } else {
                                                format!(
                                                    "{:.0} × {:.0} px{}",
                                                    layer.transform.width,
                                                    layer.transform.height,
                                                    if layer.locked { " · Locked" } else { "" }
                                                )
                                            };
                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(detail)
                                                        .size(10.0)
                                                        .color(theme::MUTED),
                                                )
                                                .truncate(),
                                            );
                                        });
                                    },
                                );
                            },
                        );
                    });
            },
        );
        let response = row.response;
        response.dnd_set_drag_payload(LayerDrag {
            project,
            layer: layer.id,
        });
        if response.clicked() {
            actions.select = Some((layer.id, false));
        }
        if response.double_clicked() && !renaming {
            if response
                .interact_pointer_pos()
                .is_some_and(|p| name_rect.contains(p))
            {
                actions.rename = Some(layer.id);
            } else if layer.raw.is_some() {
                actions.edit_raw = Some(layer.id);
            } else if layer.adjustment.is_some() {
                actions.edit_adjustment = Some(layer.id);
            } else if layer.filter.is_some() {
                actions.edit_filter = Some(layer.id);
            } else if layer.text.is_some() {
                actions.edit_text = Some(layer.id);
            } else {
                actions.rename = Some(layer.id);
            }
        }
        ui.painter().line_segment(
            [response.rect.left_bottom(), response.rect.right_bottom()],
            Stroke::new(0.5_f32, Color32::from_white_alpha(14)),
        );
        response.context_menu(|ui| {
            if layer.raw.is_some() {
                if ui
                    .add_enabled(!layer.locked, egui::Button::new("Develop RAW…"))
                    .clicked()
                {
                    actions.edit_raw = Some(layer.id);
                    ui.close();
                }
                if ui
                    .add_enabled(!layer.locked, egui::Button::new("Rasterize RAW Layer"))
                    .clicked()
                {
                    actions.select = Some((layer.id, false));
                    actions.command = Some("rasterize_raw");
                    ui.close();
                }
                ui.separator();
            }
            if layer.text.is_some() && ui.button("Edit text…").clicked() {
                actions.edit_text = Some(layer.id);
                ui.close();
            }
            if layer.adjustment.is_some() && ui.button("Edit adjustment…").clicked() {
                actions.edit_adjustment = Some(layer.id);
                ui.close();
            }
            if layer.filter.is_some() && ui.button("Edit filter…").clicked() {
                actions.edit_filter = Some(layer.id);
                ui.close();
            }
            if ui.button("Rename…").clicked() {
                actions.rename = Some(layer.id);
                ui.close();
            }
            for (label, command) in [
                ("Duplicate", "duplicate"),
                ("New Group", "group"),
                ("Move Out of Parent", "move_out"),
                ("Merge Down / Selected", "merge"),
                ("Add Mask", "mask"),
                ("Clipping Mask", "clip"),
                ("Delete", "delete_layer"),
            ] {
                if layer.standalone_mask && matches!(command, "mask" | "clip") {
                    continue;
                }
                if ui.button(label).clicked() {
                    actions.select = Some((layer.id, false));
                    actions.command = Some(command);
                    ui.close();
                }
            }
            if layer.mask.is_some() {
                ui.separator();
                for (label, command) in [
                    ("Enable / Disable Mask", "disable_mask"),
                    ("Link / Unlink Mask", "link_mask"),
                    ("Delete Mask", "delete_mask"),
                ] {
                    if layer.standalone_mask && !attached && command == "link_mask" {
                        continue;
                    }
                    if ui.button(label).clicked() {
                        actions.select = Some((layer.id, true));
                        actions.command = Some(command);
                        ui.close();
                    }
                }
            }
        });
        if let Some(source) = response.dnd_hover_payload::<LayerDrag>()
            && (source.project != project
                || !self.sessions[self.current]
                    .document
                    .descendants(source.layer)
                    .contains(&layer.id))
            && let Some(pointer) = ui.input(|i| i.pointer.hover_pos())
        {
            let source_layer = self
                .sessions
                .iter()
                .find(|s| s.document.id == source.project)
                .and_then(|s| s.document.layers.iter().find(|l| l.id == source.layer));
            let can_attach =
                layer.can_attach_effects() && source_layer.is_some_and(Layer::is_effect);
            let position = DropPosition::at(response.rect, pointer.y, layer.group || can_attach);
            if !matches!(position, DropPosition::Inside)
                && attached
                && !source_layer.is_some_and(Layer::is_effect)
            {
                return;
            }
            let stroke = Stroke::new(2.0_f32, theme::ACCENT);
            actions.drop_indicator = Some(match position {
                DropPosition::Above => egui::Shape::line_segment(
                    [response.rect.left_top(), response.rect.right_top()],
                    stroke,
                ),
                DropPosition::Below => egui::Shape::line_segment(
                    [response.rect.left_bottom(), response.rect.right_bottom()],
                    stroke,
                ),
                DropPosition::Inside => {
                    egui::Shape::rect_stroke(response.rect, 2.0, stroke, StrokeKind::Inside)
                }
            });
            if let Some(source) = response.dnd_release_payload::<LayerDrag>() {
                actions.reorder =
                    Some((*source, layer.id, position, ui.input(|i| i.modifiers.alt)));
            }
        }
    }

    fn layer_thumbnail(
        &mut self,
        ui: &mut egui::Ui,
        layer: &Layer,
        mask: bool,
        selected: bool,
        actions: &mut Actions,
    ) {
        let session = &mut self.sessions[self.current];
        let document = &session.document;
        let side = if mask && !layer.standalone_mask {
            30.0
        } else {
            36.0
        };
        let canvas_size = vec2(document.width as f32, document.height as f32);
        let size = if layer.adjustment.is_some() || layer.filter.is_some() {
            vec2(side, side)
        } else {
            canvas_size * (side / canvas_size.max_elem())
        };
        let key = (layer.id, mask);
        if session
            .thumbnails
            .get(&key)
            .is_some_and(|thumbnail| !thumbnail.matches(document, layer, mask))
        {
            session.thumbnails.remove(&key);
        }
        let thumbnail = session.thumbnails.entry(key).or_insert_with(|| {
            let color = if mask {
                let pixels = &layer.mask.as_ref().unwrap().pixels;
                let sampled = image::GrayImage::from_fn(56, 56, |x, y| {
                    *pixels.get_pixel(
                        (x * 2 + 1) * pixels.width() / 112,
                        (y * 2 + 1) * pixels.height() / 112,
                    )
                });
                let image = image::imageops::resize(
                    &sampled,
                    28,
                    28,
                    image::imageops::FilterType::Triangle,
                );
                egui::ColorImage::from_gray([28, 28], image.as_raw())
            } else if layer.pixels.is_some() {
                let mut thumbnail_document = document.clone();
                let mut thumbnail_layer = layer.clone();
                thumbnail_layer.visible = true;
                thumbnail_layer.opacity = 1.0;
                thumbnail_layer.blend = BlendMode::Normal;
                thumbnail_layer.parent = None;
                thumbnail_layer.clip_to = None;
                thumbnail_layer.mask = None;
                thumbnail_document.layers = vec![thumbnail_layer];
                let image = xuan::render::render_thumbnail(
                    &thumbnail_document,
                    (size.x * 2.0).round().max(1.0) as u32,
                    (size.y * 2.0).round().max(1.0) as u32,
                );
                egui::ColorImage::from_rgba_unmultiplied(
                    [image.width() as usize, image.height() as usize],
                    image.as_raw(),
                )
            } else {
                egui::ColorImage::filled([38, 30], Color32::from_gray(48))
            };
            let texture = ui.ctx().load_texture(
                format!("thumbnail-{}-{mask}", layer.id),
                color,
                TextureOptions::LINEAR,
            );
            LayerThumbnail {
                texture,
                canvas: [document.width, document.height],
                transform: layer.transform,
                pixels: layer.pixels.as_ref().map(Arc::downgrade),
                mask: layer.mask.as_ref().map(|mask| Arc::downgrade(&mask.pixels)),
            }
        });
        let (slot, response) = ui.allocate_exact_size(vec2(side, 36.0), Sense::click_and_drag());
        response.dnd_set_drag_payload(LayerDrag {
            project: document.id,
            layer: layer.id,
        });
        let rect = egui::Rect::from_center_size(slot.center(), size);
        widgets::checkerboard(ui, rect, 4.0);
        ui.painter().image(
            thumbnail.id(),
            rect,
            egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        ui.painter().rect_stroke(
            rect,
            2.0,
            Stroke::new(1.0_f32, Color32::from_white_alpha(75)),
            StrokeKind::Inside,
        );
        if response.clicked() {
            actions.select = Some((layer.id, mask));
        }
        if response.double_clicked() && !mask && layer.raw.is_some() {
            actions.edit_raw = Some(layer.id);
        }
        if selected && mask == self.editing_mask() {
            ui.painter().rect_stroke(
                rect.expand(2.0),
                2.0,
                Stroke::new(1.0_f32, theme::TEXT),
                StrokeKind::Outside,
            );
        }
        if mask && !layer.mask.as_ref().unwrap().enabled {
            ui.painter().line_segment(
                [response.rect.left_top(), response.rect.right_bottom()],
                Stroke::new(1.5_f32, Color32::LIGHT_RED),
            );
        }
        if !mask && layer.pixels.is_none() {
            let center = response.rect.center();
            if layer.adjustment.is_some() || layer.filter.is_some() {
                ui.painter().circle_filled(center, 7.0, theme::MUTED);
                ui.painter().rect_filled(
                    egui::Rect::from_min_max(center - vec2(0.0, 7.0), center + vec2(7.0, 7.0)),
                    0.0,
                    Color32::from_gray(48),
                );
            }
            ui.painter().rect_stroke(
                response.rect,
                2.0,
                Stroke::new(1.0_f32, Color32::from_gray(85)),
                StrokeKind::Inside,
            );
        }
    }

    fn layer_footer(&self, ui: &mut egui::Ui, actions: &mut Actions) {
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(9, 3))
            .show(ui, |ui| {
                ui.add_enabled_ui(!self.sessions.is_empty(), |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for (tip, command) in [
                            ("New layer (Ctrl+Shift+N)", "new_layer"),
                            ("Group layers (Ctrl+G)", "group"),
                            (
                                if self
                                    .session()
                                    .is_some_and(|s| s.document.active().is_none())
                                {
                                    "Add mask layer affecting layers below"
                                } else {
                                    "Add layer mask"
                                },
                                "mask",
                            ),
                        ] {
                            if icons::action_button(ui, command)
                                .on_hover_text(tip)
                                .clicked()
                            {
                                actions.command = Some(command);
                            }
                        }
                        let adjustment = icons::action_button(ui, "adjustment")
                            .on_hover_text("New adjustment layer");
                        egui::Popup::menu(&adjustment).show(|ui| {
                            actions.adjustment = menus::adjustment_menu(ui);
                            ui.separator();
                            ui.menu_button("New Filter Layer", |ui| {
                                actions.filter = menus::filter_menu(ui);
                            });
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if icons::action_button(ui, "delete_layer")
                                .on_hover_text("Delete layer")
                                .clicked()
                            {
                                actions.command = Some("delete_layer");
                            }
                        });
                    });
                });
            });
    }

    fn apply_layer_actions(&mut self, ctx: &egui::Context, actions: Actions) {
        if let Some(apply) = actions.finish_rename {
            self.finish_layer_rename(ctx, apply);
        }
        if actions.deselect {
            if let Some(session) = self.session_mut() {
                session.document.selected.clear();
                session.document.active = None;
            }
            self.mask_target = false;
        }
        if let Some(id) = actions.visibility {
            self.edit("Layer Visibility", |doc| {
                if let Some(layer) = doc.layers.iter_mut().find(|l| l.id == id) {
                    layer.visible = !layer.visible;
                }
                Ok(())
            });
        }
        if let Some((id, mask)) = actions.select {
            let extend = ctx.input(|i| i.modifiers.shift || i.modifiers.ctrl);
            if let Some(session) = self.session_mut() {
                session.document.select(id, extend);
            }
            self.mask_target = mask;
        }
        if let Some(id) = actions.collapse
            && let Some(session) = self.session_mut()
            && !session.collapsed.remove(&id)
        {
            session.collapsed.insert(id);
        }
        if let Some((blend, opacity, locked)) = actions.appearance {
            self.edit_continuous("Layer Appearance", |doc| {
                if let Some(layer) = doc.active_mut() {
                    layer.blend = blend;
                    layer.opacity = opacity;
                    layer.locked = locked;
                }
                Ok(())
            });
        }
        if let Some((source, target, position, duplicate)) = actions.reorder {
            if self
                .session()
                .is_some_and(|s| s.document.id != source.project)
            {
                self.copy_layer_to_project(source, self.current);
            } else {
                self.reorder_layer(source.layer, target, position, duplicate);
            }
        }
        if let Some(id) = actions.edit_adjustment {
            self.edit_adjustment_layer(id);
        }
        if let Some(id) = actions.edit_filter {
            self.edit_filter_layer(id);
        }
        if let Some(id) = actions.edit_text {
            self.start_text(Some(id), xuan::document::Point::default());
        }
        if let Some(id) = actions.edit_raw {
            self.start_develop_layer(id);
        }
        if let Some(id) = actions.rename {
            self.start_layer_rename(id);
            ctx.request_repaint();
        }
        if let Some(command) = actions.command {
            self.command(command);
        }
        if let Some(adjustment) = actions.adjustment {
            self.start_adjustment(adjustment, true);
        }
        if let Some(filter) = actions.filter {
            self.start_filter_layer(filter);
        }
    }

    fn layer_name(
        &mut self,
        ui: &mut egui::Ui,
        layer: &Layer,
        color: Color32,
        actions: &mut Actions,
    ) -> egui::Response {
        let Some(edit) = self.rename.as_mut().filter(|edit| edit.layer == layer.id) else {
            return ui.add(
                egui::Label::new(RichText::new(&layer.name).size(13.0).color(color))
                    .truncate()
                    .sense(Sense::hover()),
            );
        };
        let id = edit.input_id();
        let cancel = ui.input(|input| input.key_pressed(egui::Key::Escape));
        let mut output = egui::TextEdit::singleline(&mut edit.name)
            .id(id)
            .font(egui::FontId::proportional(13.0))
            .desired_width(ui.available_width())
            .show(ui);
        if edit.focus {
            output.response.request_focus();
            output
                .state
                .cursor
                .set_char_range(Some(egui::text::CCursorRange::two(
                    egui::text::CCursor::new(0),
                    egui::text::CCursor::new(edit.name.chars().count()),
                )));
            output.state.store(ui.ctx(), id);
            edit.focus = false;
        } else if cancel {
            actions.finish_rename = Some(false);
        } else if output.response.lost_focus() || output.response.clicked_elsewhere() {
            actions.finish_rename = Some(true);
        }
        output.response
    }

    pub(super) fn start_layer_rename(&mut self, id: Uuid) {
        let Some(session) = self.session() else {
            return;
        };
        let Some(layer) = session.document.layers.iter().find(|layer| layer.id == id) else {
            return;
        };
        self.rename = Some(LayerRename {
            project: session.document.id,
            layer: id,
            name: layer.name.clone(),
            focus: true,
        });
    }

    fn finish_layer_rename(&mut self, ctx: &egui::Context, apply: bool) {
        let Some(rename) = self.rename.take() else {
            return;
        };
        ctx.memory_mut(|memory| memory.surrender_focus(rename.input_id()));
        if !apply || rename.name.trim().is_empty() || rename.name.len() > 16_384 {
            return;
        }
        let changed = self.session().is_some_and(|session| {
            session.document.id == rename.project
                && session
                    .document
                    .layers
                    .iter()
                    .any(|layer| layer.id == rename.layer && layer.name != rename.name)
        });
        if changed {
            self.edit("Rename Layer", |doc| {
                if let Some(layer) = doc.layers.iter_mut().find(|layer| layer.id == rename.layer) {
                    layer.name = rename.name;
                }
                Ok(())
            });
        }
    }

    fn reorder_layer(
        &mut self,
        source: Uuid,
        target: Uuid,
        position: DropPosition,
        duplicate: bool,
    ) {
        let Some(session) = self.session() else {
            return;
        };
        if session.document.descendants(source).contains(&target)
            || !session
                .document
                .layers
                .iter()
                .any(|layer| layer.id == source)
            || !session
                .document
                .layers
                .iter()
                .any(|layer| layer.id == target)
        {
            return;
        }
        self.edit("Reorder Layer", |doc| {
            let Some(destination) = doc.layers.iter().find(|l| l.id == target).cloned() else {
                return Ok(());
            };
            if duplicate {
                doc.select(source, false);
                xuan::operations::duplicate(doc);
            }
            let source = if duplicate {
                doc.active.unwrap()
            } else {
                source
            };
            let Some(index) = doc.layers.iter().position(|l| l.id == source) else {
                return Ok(());
            };
            let mut layer = doc.layers.remove(index);
            layer.parent = if matches!(position, DropPosition::Inside) {
                Some(target)
            } else {
                destination.parent
            };
            layer.clip_to = None;
            // The panel lists siblings from top to bottom, opposite their paint order.
            let target_index = doc.layers.iter().position(|l| l.id == target).unwrap();
            let index = match position {
                DropPosition::Above => target_index + 1,
                DropPosition::Below => target_index,
                DropPosition::Inside => doc
                    .layers
                    .iter()
                    .rposition(|l| l.parent == Some(target))
                    .map_or(target_index + 1, |i| i + 1),
            };
            doc.layers.insert(index, layer);
            doc.select(source, false);
            doc.validate()
        });
        self.mask_target = false;
        if matches!(position, DropPosition::Inside)
            && let Some(session) = self.session_mut()
        {
            session.collapsed.remove(&target);
        }
    }
}
