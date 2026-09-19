use egui::{Color32, FontId, Rect, Sense, Stroke, StrokeKind, pos2, vec2};

use super::{EditorApp, theme};

impl EditorApp {
    pub(super) fn window_controls(&mut self, ui: &mut egui::Ui) {
        let focused = ui.input(|i| i.viewport().focused.unwrap_or(true));
        let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
        let (group, _) = ui.allocate_exact_size(vec2(62.0, 22.0), Sense::hover());
        let hovered = ui.rect_contains_pointer(group);
        for (index, color, label) in [
            (0, Color32::from_rgb(255, 95, 87), "Close window"),
            (1, Color32::from_rgb(254, 188, 46), "Minimize window"),
            (
                2,
                Color32::from_rgb(40, 200, 64),
                if maximized {
                    "Restore window"
                } else {
                    "Maximize window"
                },
            ),
        ] {
            let center = pos2(group.left() + 7.0 + index as f32 * 20.0, group.center().y);
            let rect = Rect::from_center_size(center, vec2(18.0, 22.0));
            let response = ui.interact(
                rect,
                ui.id().with(("window_control", index)),
                Sense::click(),
            );
            response
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
            let color = if focused || hovered {
                color
            } else {
                Color32::from_gray(83)
            };
            ui.painter().circle_filled(
                center,
                6.0,
                if response.is_pointer_button_down_on() {
                    color.gamma_multiply(0.75)
                } else {
                    color
                },
            );
            ui.painter().circle_stroke(
                center,
                6.0,
                Stroke::new(0.6_f32, Color32::from_black_alpha(65)),
            );
            if hovered || response.has_focus() {
                let stroke = Stroke::new(1.0_f32, Color32::from_black_alpha(170));
                match index {
                    0 => {
                        ui.painter().line_segment(
                            [center - vec2(2.3, 2.3), center + vec2(2.3, 2.3)],
                            stroke,
                        );
                        ui.painter().line_segment(
                            [center + vec2(-2.3, 2.3), center + vec2(2.3, -2.3)],
                            stroke,
                        );
                    }
                    1 => {
                        ui.painter().line_segment(
                            [center - vec2(3.0, 0.0), center + vec2(3.0, 0.0)],
                            stroke,
                        );
                    }
                    _ => {
                        ui.painter().rect_stroke(
                            Rect::from_center_size(center, vec2(5.0, 5.0)),
                            0.0,
                            stroke,
                            StrokeKind::Inside,
                        );
                    }
                }
            }
            if response.clicked() {
                match index {
                    0 => {
                        // Route through the same save/cancel flow as a window-manager close.
                        if self.sessions.iter().any(|s| s.history.dirty()) {
                            if let Some(job) = &self.job {
                                job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                            }
                            self.close_app = true;
                        } else {
                            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                    1 => ui
                        .ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
                    _ => ui
                        .ctx()
                        .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized)),
                }
            }
            response.on_hover_text(label);
        }
        ui.add_space(8.0);
    }

    pub(super) fn titlebar_drag(&self, ui: &mut egui::Ui) {
        let (rect, response) = ui.allocate_exact_size(
            vec2(ui.available_width().max(0.0), 22.0),
            Sense::click_and_drag(),
        );
        let title = self.session().map_or("Xuan".into(), |s| {
            format!("{}{}", s.title, if s.history.dirty() { "  •" } else { "" })
        });
        if rect.width() > 180.0 {
            let painter = ui.painter().with_clip_rect(rect.shrink2(vec2(12.0, 0.0)));
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                title,
                FontId::proportional(12.0),
                theme::MUTED,
            );
        }
        if response.double_clicked() {
            let maximized = ui.input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
        } else if response.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed())
        {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
    }

    /// Undecorated Wayland/X11 windows need client-provided edge hit targets.
    pub(super) fn window_resize(&self, ctx: &egui::Context) {
        if ctx.input(|i| {
            i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)
        }) {
            return;
        }
        let screen = ctx.content_rect();
        for (index, (rect, direction, cursor)) in resize_regions(screen).into_iter().enumerate() {
            egui::Area::new(egui::Id::new(("window_resize", index)))
                .order(egui::Order::Foreground)
                .fixed_pos(rect.min)
                .movable(false)
                .show(ctx, |ui| {
                    let (_, response) = ui.allocate_exact_size(rect.size(), Sense::drag());
                    if response.hovered() || response.dragged() {
                        ctx.set_cursor_icon(cursor);
                    }
                    if response.is_pointer_button_down_on()
                        && ui.input(|i| i.pointer.primary_pressed())
                    {
                        ctx.send_viewport_cmd(egui::ViewportCommand::BeginResize(direction));
                    }
                });
        }
    }
}

fn resize_regions(rect: Rect) -> [(Rect, egui::ResizeDirection, egui::CursorIcon); 8] {
    use egui::{CursorIcon as C, ResizeDirection as D};
    let (l, r, t, b) = (rect.left(), rect.right(), rect.top(), rect.bottom());
    let edge = 4.0;
    let corner = 10.0;
    [
        (
            Rect::from_min_max(pos2(l + corner, t), pos2(r - corner, t + edge)),
            D::North,
            C::ResizeVertical,
        ),
        (
            Rect::from_min_max(pos2(l + corner, b - edge), pos2(r - corner, b)),
            D::South,
            C::ResizeVertical,
        ),
        (
            Rect::from_min_max(pos2(l, t + corner), pos2(l + edge, b - corner)),
            D::West,
            C::ResizeHorizontal,
        ),
        (
            Rect::from_min_max(pos2(r - edge, t + corner), pos2(r, b - corner)),
            D::East,
            C::ResizeHorizontal,
        ),
        (
            Rect::from_min_max(pos2(l, t), pos2(l + corner, t + corner)),
            D::NorthWest,
            C::ResizeNwSe,
        ),
        (
            Rect::from_min_max(pos2(r - corner, t), pos2(r, t + corner)),
            D::NorthEast,
            C::ResizeNeSw,
        ),
        (
            Rect::from_min_max(pos2(l, b - corner), pos2(l + corner, b)),
            D::SouthWest,
            C::ResizeNeSw,
        ),
        (
            Rect::from_min_max(pos2(r - corner, b - corner), pos2(r, b)),
            D::SouthEast,
            C::ResizeNwSe,
        ),
    ]
}
