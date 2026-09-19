use egui::{Color32, Pos2, Rect, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};

use super::{Tool, theme};

pub fn tool_button(ui: &mut Ui, tool: Tool, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(36.0, 36.0), egui::Sense::click());
    let painter = ui.painter();
    if selected || response.hovered() {
        painter.rect(
            rect,
            theme::BUTTON_RADIUS,
            Color32::from_gray(if selected { 62 } else { 48 }),
            Stroke::new(
                1.0_f32,
                if selected {
                    Color32::from_gray(80)
                } else {
                    Color32::TRANSPARENT
                },
            ),
            StrokeKind::Inside,
        );
    }
    draw(ui, tool, rect.shrink(9.0), theme::TEXT);
    response.on_hover_text(format!("{} ({})", tool.label(), tool.shortcut()))
}

pub fn draw(ui: &Ui, tool: Tool, rect: Rect, color: Color32) {
    let painter = ui.painter();
    let s = Stroke::new(1.35_f32, color);
    let p = |x: f32, y: f32| {
        pos2(
            rect.left() + x * rect.width(),
            rect.top() + y * rect.height(),
        )
    };
    let line = |a: (f32, f32), b: (f32, f32)| {
        painter.line_segment([p(a.0, a.1), p(b.0, b.1)], s);
    };
    match tool {
        Tool::Move => {
            line((0.5, 0.0), (0.5, 1.0));
            line((0.0, 0.5), (1.0, 0.5));
            for (a, b) in [
                ((0.5, 0.0), (0.32, 0.2)),
                ((0.5, 0.0), (0.68, 0.2)),
                ((0.5, 1.0), (0.32, 0.8)),
                ((0.5, 1.0), (0.68, 0.8)),
                ((0.0, 0.5), (0.2, 0.32)),
                ((0.0, 0.5), (0.2, 0.68)),
                ((1.0, 0.5), (0.8, 0.32)),
                ((1.0, 0.5), (0.8, 0.68)),
            ] {
                line(a, b);
            }
        }
        Tool::Marquee => {
            for t in [0.0, 0.4, 0.8] {
                line((t, 0.0), ((t + 0.2).min(1.0), 0.0));
                line((t, 1.0), ((t + 0.2).min(1.0), 1.0));
                line((0.0, t), (0.0, (t + 0.2).min(1.0)));
                line((1.0, t), (1.0, (t + 0.2).min(1.0)));
            }
        }
        Tool::Lasso => {
            painter.add(egui::Shape::closed_line(
                (0..32)
                    .map(|i| {
                        let a = i as f32 / 32.0 * std::f32::consts::TAU;
                        p(0.5 + a.cos() * 0.47, 0.37 + a.sin() * 0.32)
                    })
                    .collect(),
                s,
            ));
            line((0.2, 0.6), (0.4, 0.86));
            line((0.4, 0.86), (0.1, 1.0));
        }
        Tool::Wand => {
            line((0.0, 1.0), (0.76, 0.24));
            line((0.1, 1.0), (0.82, 0.3));
            for (x, y) in [(0.15, 0.18), (0.75, 0.04), (0.95, 0.58)] {
                line((x - 0.1, y), (x + 0.1, y));
                line((x, y - 0.1), (x, y + 0.1));
            }
        }
        Tool::Crop => {
            line((0.2, 0.0), (0.2, 0.8));
            line((0.2, 0.8), (1.0, 0.8));
            line((0.0, 0.2), (0.8, 0.2));
            line((0.8, 0.2), (0.8, 1.0));
        }
        Tool::Brush | Tool::Erase => {
            painter.add(egui::Shape::closed_line(
                vec![p(0.2, 0.65), p(0.72, 0.04), p(0.96, 0.26), p(0.42, 0.83)],
                s,
            ));
            if tool == Tool::Brush {
                painter.add(egui::Shape::convex_polygon(
                    vec![p(0.17, 0.62), p(0.43, 0.84), p(0.02, 1.0)],
                    color,
                    Stroke::NONE,
                ));
            } else {
                line((0.2, 0.65), (0.0, 0.88));
                line((0.0, 0.88), (0.2, 1.0));
                line((0.2, 1.0), (1.0, 1.0));
            }
        }
        Tool::Heal => {
            painter.add(egui::Shape::closed_line(
                vec![p(0.0, 0.65), p(0.65, 0.0), p(1.0, 0.35), p(0.35, 1.0)],
                s,
            ));
            for (x, y) in [(0.4, 0.4), (0.6, 0.4), (0.4, 0.6), (0.6, 0.6)] {
                painter.circle_filled(p(x, y), 0.8, color);
            }
        }
        Tool::Clone => {
            painter.circle_stroke(p(0.5, 0.2), 3.0, s);
            line((0.4, 0.36), (0.35, 0.65));
            line((0.6, 0.36), (0.65, 0.65));
            painter.rect_stroke(
                Rect::from_two_pos(p(0.1, 0.65), p(0.9, 0.87)),
                2.0,
                s,
                StrokeKind::Inside,
            );
            line((0.08, 1.0), (0.92, 1.0));
        }
        Tool::Blur => {
            painter.add(egui::Shape::closed_line(
                vec![
                    p(0.5, 0.0),
                    p(0.86, 0.58),
                    p(0.88, 0.8),
                    p(0.7, 1.0),
                    p(0.3, 1.0),
                    p(0.12, 0.8),
                    p(0.14, 0.58),
                ],
                s,
            ));
        }
        Tool::Gradient => {
            for i in 0..16 {
                let shade = (i * 14 + 30) as u8;
                painter.rect_filled(
                    Rect::from_min_size(
                        p(i as f32 / 16.0, 0.05),
                        vec2(rect.width() / 16.0 + 0.5, rect.height() * 0.9),
                    ),
                    0.0,
                    Color32::from_gray(shade),
                );
            }
            painter.rect_stroke(rect, 1.0, s, StrokeKind::Inside);
        }
        Tool::Shape => {
            painter.circle_stroke(p(0.64, 0.63), 6.0, s);
            painter.rect(
                Rect::from_two_pos(p(0.0, 0.0), p(0.65, 0.65)),
                2.0,
                theme::PANEL,
                s,
                StrokeKind::Inside,
            );
        }
        Tool::Text => {
            line((0.08, 0.1), (0.92, 0.1));
            line((0.08, 0.1), (0.08, 0.3));
            line((0.92, 0.1), (0.92, 0.3));
            line((0.5, 0.1), (0.5, 0.95));
            line((0.3, 0.95), (0.7, 0.95));
        }
        Tool::Dropper => {
            line((0.3, 0.48), (0.0, 0.95));
            line((0.0, 0.95), (0.18, 1.0));
            line((0.18, 1.0), (0.65, 0.58));
            line((0.24, 0.36), (0.73, 0.85));
            line((0.38, 0.5), (0.8, 0.06));
            line((0.8, 0.06), (1.0, 0.25));
            line((1.0, 0.25), (0.58, 0.67));
        }
        Tool::Hand => {
            painter.add(egui::Shape::closed_line(
                vec![
                    p(0.25, 0.95),
                    p(0.02, 0.48),
                    p(0.14, 0.4),
                    p(0.3, 0.58),
                    p(0.28, 0.08),
                    p(0.42, 0.05),
                    p(0.45, 0.45),
                    p(0.5, 0.0),
                    p(0.63, 0.0),
                    p(0.63, 0.43),
                    p(0.73, 0.09),
                    p(0.85, 0.14),
                    p(0.8, 0.54),
                    p(0.95, 0.28),
                    p(1.0, 0.42),
                    p(0.82, 0.94),
                ],
                s,
            ));
        }
        Tool::Zoom => {
            painter.circle_stroke(p(0.4, 0.4), 6.0, s);
            line((0.68, 0.68), (1.0, 1.0));
            line((0.2, 0.4), (0.6, 0.4));
            line((0.4, 0.2), (0.4, 0.6));
        }
    }
}

pub fn eye(ui: &mut Ui, visible: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(20.0), egui::Sense::click());
    let c = rect.center();
    let color = if visible {
        theme::TEXT
    } else {
        Color32::from_gray(80)
    };
    let points: Vec<Pos2> = (0..24)
        .map(|i| {
            let a = i as f32 / 24.0 * std::f32::consts::TAU;
            c + vec2(a.cos() * 7.0, a.sin() * 4.0)
        })
        .collect();
    ui.painter().add(egui::Shape::closed_line(
        points,
        Stroke::new(1.0_f32, color),
    ));
    if visible {
        ui.painter().circle_filled(c, 2.0, color);
    } else {
        ui.painter().line_segment(
            [c + vec2(-6.0, -5.0), c + vec2(6.0, 5.0)],
            Stroke::new(1.0_f32, color),
        );
    }
    response.on_hover_text("Toggle visibility")
}

pub fn action_button(ui: &mut Ui, kind: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(28.0), egui::Sense::click());
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, 5.0, Color32::from_gray(55));
    }
    let rect = rect.shrink(6.0);
    let stroke = Stroke::new(1.2_f32, super::theme::MUTED);
    let p = |x: f32, y: f32| rect.min + vec2(x * rect.width(), y * rect.height());
    match kind {
        "new_layer" => {
            painter.rect_stroke(rect, 2.0, stroke, StrokeKind::Inside);
            painter.line_segment([p(0.25, 0.5), p(0.75, 0.5)], stroke);
            painter.line_segment([p(0.5, 0.25), p(0.5, 0.75)], stroke);
        }
        "group" => {
            painter.add(egui::Shape::closed_line(
                vec![
                    p(0.0, 0.1),
                    p(0.4, 0.1),
                    p(0.52, 0.3),
                    p(1.0, 0.3),
                    p(1.0, 0.9),
                    p(0.0, 0.9),
                ],
                stroke,
            ));
        }
        "adjustment" => {
            painter.circle_stroke(rect.center(), 7.0, stroke);
            let points = (0..=24)
                .map(|i| {
                    let a = std::f32::consts::FRAC_PI_2 + i as f32 / 24.0 * std::f32::consts::PI;
                    rect.center() + vec2(a.cos(), a.sin()) * 6.5
                })
                .collect();
            painter.add(egui::Shape::convex_polygon(
                points,
                theme::MUTED,
                Stroke::NONE,
            ));
        }
        "mask" => {
            painter.rect_stroke(rect, 1.0, stroke, StrokeKind::Inside);
            painter.circle_filled(rect.center(), 3.5, super::theme::MUTED);
        }
        "delete_layer" => {
            painter.rect_stroke(
                Rect::from_min_max(p(0.2, 0.25), p(0.8, 1.0)),
                1.0,
                stroke,
                StrokeKind::Inside,
            );
            painter.line_segment([p(0.05, 0.2), p(0.95, 0.2)], stroke);
            painter.line_segment([p(0.35, 0.0), p(0.65, 0.0)], stroke);
            for x in [0.4, 0.6] {
                painter.line_segment([p(x, 0.4), p(x, 0.8)], stroke);
            }
        }
        _ => {}
    }
    response
}

pub fn lock(ui: &mut Ui, locked: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(16.0, 22.0), egui::Sense::click());
    let center = rect.center();
    let color = if locked { theme::TEXT } else { theme::MUTED };
    let stroke = Stroke::new(1.0_f32, color);
    ui.painter().rect_stroke(
        Rect::from_center_size(center + vec2(0.0, 2.0), vec2(9.0, 7.0)),
        1.5,
        stroke,
        StrokeKind::Inside,
    );
    let offset = if locked { 0.0 } else { 3.0 };
    let points = (0..=12)
        .map(|i| {
            let angle = std::f32::consts::PI + i as f32 / 12.0 * std::f32::consts::PI;
            center + vec2(offset + angle.cos() * 3.0, -2.0 + angle.sin() * 3.0)
        })
        .collect();
    ui.painter().add(egui::Shape::line(points, stroke));
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            ui.is_enabled(),
            locked,
            "Lock layer",
        )
    });
    response.on_hover_text(if locked { "Unlock layer" } else { "Lock layer" })
}
