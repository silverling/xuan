use super::widgets;
use egui::{Color32, RichText, Sense, Stroke, StrokeKind, TextureOptions, vec2};
use uuid::Uuid;
use xuan::{
    blend::BlendMode,
    document::{Adjustment, Document, Layer},
};

use super::{EditorApp, LayerDrag, icons, menus, theme};

#[derive(Default)]
struct Actions {
    command: Option<&'static str>,
    adjustment: Option<Adjustment>,
    visibility: Option<Uuid>,
    select: Option<(Uuid, bool)>,
    collapse: Option<Uuid>,
    reorder: Option<(LayerDrag, Uuid, DropPosition, bool)>,
    appearance: Option<(BlendMode, f32, bool)>,
    rename: Option<(Uuid, String)>,
    edit_adjustment: Option<Uuid>,
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
            if layer.group && !collapsed.contains(&layer.id) {
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
                    let height = (ui.available_height() - 55.0).max(40.0);
                    egui::ScrollArea::vertical()
                        .id_salt("layers_scroll")
                        .max_height(height)
                        .min_scrolled_height(height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 2.0;
                            if let Some(session) = self.session() {
                                for (layer, depth) in rows(&session.document, &session.collapsed) {
                                    self.layer_row(ui, &layer, depth, &mut actions);
                                }
                            } else {
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
                        });
                    ui.separator();
                    self.layer_footer(ui, &mut actions);
                });
            });
        self.apply_layer_actions(ctx, actions);
    }

    fn layer_controls(&self, ui: &mut egui::Ui, actions: &mut Actions) {
        egui::Frame::new()
            .inner_margin(egui::Margin::symmetric(18, 15))
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
                        if icons::lock(ui, locked).clicked() {
                            locked = !locked;
                            changed = true;
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Opacity").size(11.0));
                        ui.spacing_mut().slider_width = (ui.available_width() - 72.0).max(40.0);
                        changed |= ui
                            .add(widgets::Slider::new(&mut opacity, 0.0..=1.0).percentage())
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
        let project = session.document.id;
        let width = ui.available_width();
        let row = egui::Frame::new()
            .fill(if selected {
                Color32::from_gray(57)
            } else {
                theme::PANEL
            })
            .inner_margin(egui::Margin::symmetric(8, 8))
            .show(ui, |ui| {
                ui.set_min_width((width - 16.0).max(0.0));
                ui.horizontal(|ui| {
                    ui.set_min_height(36.0);
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if icons::eye(ui, layer.visible).clicked() {
                        actions.visibility = Some(layer.id);
                    }
                    ui.add_space((depth as f32 * 24.0).min(72.0));
                    if layer.group {
                        let collapsed = self.sessions[self.current].collapsed.contains(&layer.id);
                        if ui.small_button(if collapsed { "▸" } else { "▾" }).clicked() {
                            actions.collapse = Some(layer.id);
                        }
                        if icons::action_button(ui, "group").clicked() {
                            actions.select = Some((layer.id, false));
                        }
                    } else {
                        if layer.clip_to.is_some() {
                            ui.label(RichText::new("↳").small().color(theme::MUTED));
                        }
                        self.layer_thumbnail(ui, layer, false, selected, actions);
                    }
                    if layer.mask.is_some() {
                        self.layer_thumbnail(ui, layer, true, selected, actions);
                    }
                    let color = if layer.visible {
                        theme::TEXT
                    } else {
                        theme::MUTED
                    };
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        ui.add(
                            egui::Label::new(RichText::new(&layer.name).size(13.0).color(color))
                                .truncate()
                                .sense(Sense::hover()),
                        );
                        let detail = if layer.group {
                            "Folder".to_owned()
                        } else if let Some(adjustment) = &layer.adjustment {
                            adjustment.name().to_owned()
                        } else {
                            format!(
                                "{:.0} × {:.0} px{}",
                                layer.transform.width,
                                layer.transform.height,
                                if layer.locked { " · Locked" } else { "" }
                            )
                        };
                        ui.add(
                            egui::Label::new(RichText::new(detail).size(10.0).color(theme::MUTED))
                                .truncate(),
                        );
                    });
                });
            });
        let response = row.response.interact(Sense::click_and_drag());
        response.dnd_set_drag_payload(LayerDrag {
            project,
            layer: layer.id,
        });
        if response.clicked() {
            actions.select = Some((layer.id, false));
        }
        if response.double_clicked() {
            if layer.adjustment.is_some() {
                actions.edit_adjustment = Some(layer.id);
            } else {
                actions.rename = Some((layer.id, layer.name.clone()));
            }
        }
        ui.painter().line_segment(
            [response.rect.left_bottom(), response.rect.right_bottom()],
            Stroke::new(0.5_f32, Color32::from_white_alpha(14)),
        );
        response.context_menu(|ui| {
            if layer.adjustment.is_some() && ui.button("Edit adjustment…").clicked() {
                actions.edit_adjustment = Some(layer.id);
                ui.close();
            }
            if ui.button("Rename…").clicked() {
                actions.rename = Some((layer.id, layer.name.clone()));
                ui.close();
            }
            for (label, command) in [
                ("Duplicate", "duplicate"),
                ("New Group", "group"),
                ("Move Out of Group", "move_out"),
                ("Merge Down / Selected", "merge"),
                ("Add Mask", "mask"),
                ("Clipping Mask", "clip"),
                ("Delete", "delete_layer"),
            ] {
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
            let position = DropPosition::at(response.rect, pointer.y, layer.group);
            let stroke = Stroke::new(2.0_f32, theme::ACCENT);
            match position {
                DropPosition::Above => {
                    ui.painter().line_segment(
                        [response.rect.left_top(), response.rect.right_top()],
                        stroke,
                    );
                }
                DropPosition::Below => {
                    ui.painter().line_segment(
                        [response.rect.left_bottom(), response.rect.right_bottom()],
                        stroke,
                    );
                }
                DropPosition::Inside => {
                    ui.painter()
                        .rect_stroke(response.rect, 2.0, stroke, StrokeKind::Inside);
                }
            }
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
        let side = if mask { 30.0 } else { 36.0 };
        let canvas_size = vec2(document.width as f32, document.height as f32);
        let size = if layer.adjustment.is_some() {
            vec2(side, side)
        } else {
            canvas_size * (side / canvas_size.max_elem())
        };
        let thumbnail = session
            .thumbnails
            .entry((layer.id, mask))
            .or_insert_with(|| {
                let color = if mask {
                    let image = image::imageops::resize(
                        &*layer.mask.as_ref().unwrap().pixels,
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
                    let image = xuan::render::render_scaled(
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
                ui.ctx().load_texture(
                    format!("thumbnail-{}-{mask}", layer.id),
                    color,
                    TextureOptions::LINEAR,
                )
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
        if selected && mask == self.mask_target {
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
            if layer.adjustment.is_some() {
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
            .inner_margin(egui::Margin::symmetric(9, 7))
            .show(ui, |ui| {
                ui.add_enabled_ui(!self.sessions.is_empty(), |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        for (tip, command) in [
                            ("New layer (Ctrl+Shift+N)", "new_layer"),
                            ("Group layers (Ctrl+G)", "group"),
                            ("Add layer mask", "mask"),
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
        if let Some(rename) = actions.rename {
            self.rename = Some(rename);
        }
        if let Some(command) = actions.command {
            self.command(command);
        }
        if let Some(adjustment) = actions.adjustment {
            self.start_adjustment(adjustment, true);
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
