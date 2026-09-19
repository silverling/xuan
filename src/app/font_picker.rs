use std::collections::HashMap;

use egui::{Color32, Context, Id, Key, Modifiers, Popup, Rect, Sense, TextureHandle, Ui, vec2};
use xuan::text::{TextRenderer, TextStyle};

use super::{theme, widgets};

const ROW_HEIGHT: f32 = 28.0;
const LIST_HEIGHT: f32 = 240.0;

#[derive(Default)]
pub(super) struct FontPicker {
    filter: String,
    popup_id: Option<Id>,
    scroll_to_selected: bool,
    focus_button: bool,
    previews: HashMap<String, Option<TextureHandle>>,
    pixels_per_point: f32,
}

impl FontPicker {
    fn matching_families(&self, families: &[String]) -> Vec<String> {
        let filter = self.filter.to_lowercase();
        families
            .iter()
            .filter(|name| name.to_lowercase().contains(&filter))
            .cloned()
            .collect()
    }

    // Run before drawing text fields so arrows cannot also move their cursors.
    pub(super) fn handle_keys(
        &mut self,
        ctx: &Context,
        families: &[String],
        selected: &mut String,
    ) {
        let Some(id) = self.popup_id.filter(|id| Popup::is_id_open(ctx, *id)) else {
            return;
        };
        let families = self.matching_families(families);
        let steps = ctx.input_mut(|input| {
            let mut steps = Vec::new();
            input.events.retain(|event| {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers: Modifiers::NONE,
                    ..
                } = event
                {
                    match key {
                        Key::ArrowUp => steps.push(-1),
                        Key::ArrowDown => steps.push(1),
                        _ => return true,
                    }
                    return false;
                }
                true
            });
            steps
        });
        for step in steps {
            if !families.is_empty() {
                let index = families.iter().position(|family| family == selected);
                let next = match (index, step) {
                    (Some(index), -1) => index.saturating_sub(1),
                    (Some(index), _) => (index + 1).min(families.len() - 1),
                    (None, -1) => families.len() - 1,
                    (None, _) => 0,
                };
                *selected = families[next].clone();
                self.scroll_to_selected = true;
            }
        }
        if ctx.input_mut(|input| {
            input.consume_key(Modifiers::NONE, Key::Escape)
                || input.consume_key(Modifiers::NONE, Key::Enter)
        }) {
            Popup::close_id(ctx, id);
            self.focus_button = true;
        }
    }

    pub(super) fn show(&mut self, ui: &mut Ui, renderer: &mut TextRenderer, selected: &mut String) {
        let button = widgets::PopUp::from_id_salt("text_family")
            .width(255.0)
            .selected_text(selected.as_str())
            .button(ui);
        let popup_id = Popup::default_response_id(&button);
        self.popup_id = Some(popup_id);
        if std::mem::take(&mut self.focus_button) {
            button.request_focus();
        }
        let was_open = Popup::is_id_open(ui.ctx(), popup_id);
        Popup::menu(&button)
            .width(300.0)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                let search = ui.add(
                    egui::TextEdit::singleline(&mut self.filter)
                        .id_salt("font_search")
                        .desired_width(f32::INFINITY)
                        .hint_text("Search installed fonts"),
                );
                if !was_open {
                    search.request_focus();
                    self.scroll_to_selected = true;
                }
                let families = self.matching_families(renderer.families());
                if families.is_empty() {
                    ui.label("No matching fonts");
                    return;
                }
                ui.spacing_mut().item_spacing.y = 0.0;
                let mut scroll = egui::ScrollArea::vertical()
                    .id_salt("font_options")
                    .max_height(LIST_HEIGHT);
                if std::mem::take(&mut self.scroll_to_selected) || search.changed() {
                    let index = families
                        .iter()
                        .position(|family| family == selected)
                        .unwrap_or(0);
                    scroll = scroll.vertical_scroll_offset(
                        (index as f32 * ROW_HEIGHT - (LIST_HEIGHT - ROW_HEIGHT) * 0.5).max(0.0),
                    );
                }
                let scale = ui.ctx().pixels_per_point();
                if self.pixels_per_point != scale {
                    self.previews.clear();
                    self.pixels_per_point = scale;
                }
                scroll.show_rows(ui, ROW_HEIGHT, families.len(), |ui, rows| {
                    // Cache just the visible options, even with thousands of fonts.
                    self.previews
                        .retain(|family, _| families[rows.clone()].contains(family));
                    for family in &families[rows] {
                        let preview = self.previews.entry(family.clone()).or_insert_with(|| {
                            let style = TextStyle {
                                content: family.clone(),
                                family: family.clone(),
                                size: 15.0 * scale,
                                color: [255; 4],
                                ..Default::default()
                            };
                            renderer.render(&style).ok().map(|pixels| {
                                let image = egui::ColorImage::from_rgba_unmultiplied(
                                    [pixels.width() as usize, pixels.height() as usize],
                                    pixels.as_raw(),
                                );
                                ui.ctx().load_texture(
                                    format!("font-preview-{family}"),
                                    image,
                                    egui::TextureOptions::LINEAR,
                                )
                            })
                        });
                        let (rect, response) = ui.allocate_exact_size(
                            vec2(ui.available_width(), ROW_HEIGHT),
                            Sense::click(),
                        );
                        let is_selected = family == selected;
                        response.widget_info(|| {
                            egui::WidgetInfo::selected(
                                egui::WidgetType::SelectableLabel,
                                ui.is_enabled(),
                                is_selected,
                                family,
                            )
                        });
                        if is_selected || response.hovered() || response.has_focus() {
                            ui.painter().rect_filled(
                                rect,
                                4.0,
                                if is_selected {
                                    theme::ACCENT
                                } else {
                                    theme::DIVIDER
                                },
                            );
                        }
                        let label_rect = rect.shrink2(vec2(8.0, 2.0));
                        let painter = ui
                            .painter()
                            .with_clip_rect(label_rect.intersect(ui.clip_rect()));
                        if let Some(texture) = preview {
                            let mut size = texture.size_vec2() / scale;
                            size *= (label_rect.height() / size.y).min(1.0);
                            painter.image(
                                texture.id(),
                                Rect::from_min_size(
                                    egui::pos2(
                                        label_rect.left(),
                                        label_rect.center().y - size.y * 0.5,
                                    ),
                                    size,
                                ),
                                Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                                theme::TEXT,
                            );
                        } else {
                            painter.text(
                                label_rect.left_center(),
                                egui::Align2::LEFT_CENTER,
                                family,
                                egui::FontId::proportional(15.0),
                                Color32::LIGHT_RED,
                            );
                        }
                        if response.on_hover_text(family).clicked() {
                            selected.clone_from(family);
                            button.request_focus();
                            ui.close();
                        }
                    }
                });
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visible_options_use_font_rasters_and_cache_at_display_resolution() {
        let ctx = Context::default();
        theme::apply(&ctx);
        let mut renderer = TextRenderer::default();
        let mut picker = FontPicker::default();
        let mut selected = TextStyle::default().family;
        let draw = |picker: &mut FontPicker, renderer: &mut TextRenderer, selected: &mut String| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(640.0, 480.0))),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| picker.show(ui, renderer, selected));
                },
            )
        };
        draw(&mut picker, &mut renderer, &mut selected);
        Popup::open_id(&ctx, picker.popup_id.unwrap());
        let output = draw(&mut picker, &mut renderer, &mut selected);
        assert!(!picker.previews.is_empty());
        assert!(picker.previews.len() <= 11);
        for (family, preview) in &picker.previews {
            let texture = preview.as_ref().expect("font preview should render");
            let pixels = renderer
                .render(&TextStyle {
                    content: family.clone(),
                    family: family.clone(),
                    size: 15.0,
                    color: [255; 4],
                    ..Default::default()
                })
                .unwrap();
            let expected = egui::ColorImage::from_rgba_unmultiplied(
                [pixels.width() as usize, pixels.height() as usize],
                pixels.as_raw(),
            );
            let delta = &output
                .textures_delta
                .set
                .iter()
                .find(|(id, _)| *id == texture.id())
                .unwrap()
                .1;
            let egui::ImageData::Color(image) = &delta.image;
            assert_eq!(image.as_ref(), &expected);
        }
        let output = draw(&mut picker, &mut renderer, &mut selected);
        assert!(!output.textures_delta.set.iter().any(|(id, _)| {
            picker
                .previews
                .values()
                .flatten()
                .any(|preview| preview.id() == *id)
        }));
        let old_ids: Vec<_> = picker
            .previews
            .values()
            .flatten()
            .map(TextureHandle::id)
            .collect();
        ctx.set_pixels_per_point(2.0);
        draw(&mut picker, &mut renderer, &mut selected);
        assert_eq!(picker.pixels_per_point, 2.0);
        assert!(
            picker
                .previews
                .values()
                .flatten()
                .all(|texture| !old_ids.contains(&texture.id()))
        );
    }
}
