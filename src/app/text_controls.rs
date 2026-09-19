use egui::RichText;
use uuid::Uuid;
use xuan::{
    document::{Layer, Point},
    render,
    text::{self, TextRenderer, TextStyle},
};

use super::{Dialog, EditorApp, Tool, theme, widgets};

pub(super) struct TextEdit {
    target: Uuid,
    original: Layer,
    pub(super) style: TextStyle,
    filter: String,
    focus: bool,
    changed: bool,
    error: Option<String>,
}

impl EditorApp {
    pub(super) fn text_options(&mut self, ui: &mut egui::Ui) {
        let active = self
            .session()
            .and_then(|s| s.document.active())
            .filter(|layer| layer.text.is_some() && !layer.locked)
            .map(|layer| layer.id);
        if ui
            .add_enabled(active.is_some(), widgets::Button::new("Edit text…"))
            .clicked()
        {
            self.start_text(active, Point::default());
        }
        if ui
            .add_enabled(self.session().is_some(), widgets::Button::new("Add text…"))
            .clicked()
            && let Some(session) = self.session()
        {
            let point = Point::new(
                session.document.width as f32 * 0.25,
                session.document.height as f32 * 0.25,
            );
            self.start_text(None, point);
        }
        ui.label(RichText::new("Click the canvas to place text").color(theme::MUTED));
    }

    pub(super) fn text_click(&mut self, point: Point) {
        let Some(session) = self.session() else {
            return;
        };
        let document = &session.document;
        if !(0.0..document.width as f32).contains(&point.x)
            || !(0.0..document.height as f32).contains(&point.y)
        {
            return;
        }
        let hit = render::hit_test(document, point);
        let target = hit
            .and_then(|id| {
                document
                    .layers
                    .iter()
                    .find(|layer| layer.id == id && layer.text.is_some())
            })
            .or_else(|| {
                document.active().filter(|layer| {
                    let unit = layer.transform.inverse(point);
                    hit.is_none()
                        && layer.visible
                        && layer.text.is_some()
                        && (0.0..=1.0).contains(&unit.x)
                        && (0.0..=1.0).contains(&unit.y)
                })
            })
            .map(|layer| layer.id);
        self.start_text(target, point);
    }

    pub(super) fn start_text(&mut self, target: Option<Uuid>, point: Point) {
        if self.dialog.is_some() || self.job.is_some() || self.session().is_none() {
            return;
        }
        self.cancel_gesture();
        let (layer, is_new) = if let Some(id) = target {
            let Some(layer) = self
                .session()
                .unwrap()
                .document
                .layers
                .iter()
                .find(|layer| layer.id == id && layer.text.is_some())
                .cloned()
            else {
                return;
            };
            if layer.locked {
                self.status = "Unlock the text layer to edit it".into();
                return;
            }
            (layer, false)
        } else {
            let mut style = self.text_style.clone();
            style.content = "Text".into();
            style.color = self.brush.color;
            let renderer = self.text_renderer.get_or_insert_with(TextRenderer::default);
            let pixels = match renderer.render(&style) {
                Ok(pixels) => pixels,
                Err(error) => {
                    self.error = Some(error.to_string());
                    return;
                }
            };
            let mut layer = Layer::image(style.layer_name(), pixels);
            layer.transform.x = point.x;
            layer.transform.y = point.y;
            layer.text = Some(style);
            (layer, true)
        };
        self.text_renderer.get_or_insert_with(TextRenderer::default);
        let session = self.session_mut().unwrap();
        session.history.commit();
        session.history.begin(
            if is_new { "Add Text" } else { "Edit Text" },
            &session.document,
        );
        if is_new {
            session.document.insert(layer.clone());
        } else {
            session.document.select(layer.id, false);
        }
        if let Err(error) = session.document.validate() {
            session.history.cancel(&mut session.document);
            self.error = Some(error.to_string());
            return;
        }
        let layer = session.document.active().unwrap().clone();
        session.invalidate();
        self.text_edit = Some(TextEdit {
            target: layer.id,
            style: layer.text.clone().unwrap(),
            original: layer,
            filter: String::new(),
            focus: true,
            changed: is_new,
            error: None,
        });
        self.tool = Tool::Text;
        self.mask_target = false;
        self.dialog = Some(Dialog::Text);
    }

    pub(super) fn preview_text(&mut self) {
        let Some(edit) = &self.text_edit else { return };
        let mut layer = edit.original.clone();
        let style = edit.style.clone();
        let result = self
            .text_renderer
            .as_mut()
            .unwrap()
            .render(&style)
            .and_then(|pixels| text::update_layer(&mut layer, style, pixels));
        let result = result.and_then(|()| {
            let session = self.session_mut().unwrap();
            let index = session
                .document
                .layers
                .iter()
                .position(|item| item.id == layer.id)
                .unwrap();
            let old = std::mem::replace(&mut session.document.layers[index], layer);
            if let Err(error) = session.document.validate() {
                session.document.layers[index] = old;
                return Err(error);
            }
            session.invalidate();
            Ok(())
        });
        let edit = self.text_edit.as_mut().unwrap();
        edit.changed = true;
        edit.error = result.err().map(|error| error.to_string());
    }

    pub(super) fn finish_text(&mut self, apply: bool) {
        if apply
            && self
                .text_edit
                .as_ref()
                .is_some_and(|edit| edit.error.is_some())
        {
            return;
        }
        let Some(edit) = self.text_edit.take() else {
            return;
        };
        if apply && edit.changed {
            self.text_style = edit.style;
            self.brush.color = self.text_style.color;
            let session = self.session_mut().unwrap();
            session.document.select(edit.target, false);
            session.history.commit();
            self.status = "Text applied".into();
        } else if let Some(session) = self.session_mut() {
            session.history.cancel(&mut session.document);
            session.invalidate();
        }
        self.dialog = None;
    }

    pub(super) fn text_dialog(&mut self, ctx: &egui::Context) {
        let apply_shortcut =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Enter));
        let Some(edit) = &mut self.text_edit else {
            return;
        };
        let renderer = self.text_renderer.as_ref().unwrap();
        let before = edit.style.clone();
        let mut open = true;
        let mut apply = false;
        let mut cancel = false;
        widgets::Window::new("Text")
            .default_width(440.0)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 10.0;
                let response = egui::ScrollArea::vertical()
                    .id_salt("text_content_scroll")
                    .max_height(140.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut edit.style.content)
                                .id_salt("text_content")
                                .desired_width(f32::INFINITY)
                                .desired_rows(4)
                                .char_limit(text::MAX_TEXT_BYTES),
                        )
                    })
                    .inner;
                if edit.focus {
                    response.request_focus();
                    if let Some(mut state) = egui::TextEdit::load_state(ctx, response.id) {
                        state
                            .cursor
                            .set_char_range(Some(egui::text::CCursorRange::two(
                                egui::text::CCursor::new(0),
                                egui::text::CCursor::new(edit.style.content.chars().count()),
                            )));
                        state.store(ctx, response.id);
                    }
                    edit.focus = false;
                }
                ui.horizontal(|ui| {
                    ui.label("Font");
                    widgets::PopUp::from_id_salt("text_family")
                        .width(255.0)
                        .selected_text(&edit.style.family)
                        .show_ui(ui, |ui| {
                            ui.add(
                                egui::TextEdit::singleline(&mut edit.filter)
                                    .hint_text("Search installed fonts"),
                            );
                            let filter = edit.filter.to_lowercase();
                            egui::ScrollArea::vertical()
                                .max_height(240.0)
                                .show(ui, |ui| {
                                    for family in renderer
                                        .families()
                                        .iter()
                                        .filter(|name| name.to_lowercase().contains(&filter))
                                    {
                                        widgets::menu_choice(
                                            ui,
                                            &mut edit.style.family,
                                            family.clone(),
                                            family,
                                        );
                                    }
                                });
                        });
                });
                if !renderer.has_family(&edit.style.family) {
                    ui.label(
                        RichText::new("This font is unavailable. Editing uses a fallback font.")
                            .color(theme::MUTED)
                            .small(),
                    );
                }
                ui.horizontal(|ui| {
                    ui.label("Size");
                    ui.add(
                        widgets::Number::new(&mut edit.style.size)
                            .range(1.0..=1024.0)
                            .suffix(" px")
                            .max_decimals(1),
                    );
                    ui.add_space(12.0);
                    ui.label("Color");
                    widgets::color_well(ui, &mut edit.style.color);
                });
                ui.horizontal(|ui| {
                    widgets::checkbox(ui, &mut edit.style.bold, "Bold");
                    widgets::checkbox(ui, &mut edit.style.italic, "Italic");
                    widgets::checkbox(ui, &mut edit.style.underline, "Underline");
                    widgets::checkbox(ui, &mut edit.style.strikethrough, "Strikethrough");
                });
                if let Some(error) = &edit.error {
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Live preview · Ctrl+Enter to apply")
                            .small()
                            .color(theme::MUTED),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        apply = ui
                            .add_enabled(
                                edit.error.is_none(),
                                widgets::Button::new("Apply").primary(),
                            )
                            .clicked();
                        cancel = widgets::button(ui, "Cancel").clicked();
                    });
                });
            });
        if cancel || !open || ctx.input(|input| input.key_pressed(egui::Key::Escape)) {
            self.finish_text(false);
            return;
        }
        if edit.style != before {
            self.preview_text();
        }
        if apply || apply_shortcut {
            self.finish_text(true);
        }
    }
}
