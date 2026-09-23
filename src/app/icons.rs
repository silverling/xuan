use egui::{Color32, Rect, Stroke, StrokeKind, Ui, Vec2, vec2};

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
    let icon_padding = if tool == Tool::Gradient { 9.0 } else { 7.0 };
    draw(ui, tool, rect.shrink(icon_padding), theme::TEXT);
    response.on_hover_text(format!("{} ({})", tool.label(), tool.shortcut()))
}

pub fn draw(ui: &Ui, tool: Tool, rect: Rect, color: Color32) {
    let source = match tool {
        Tool::Move => egui::include_image!("../../assets/svg/transform.svg"),
        Tool::Marquee => egui::include_image!("../../assets/svg/marquee.svg"),
        Tool::Lasso => egui::include_image!("../../assets/svg/lasso.svg"),
        Tool::Wand => egui::include_image!("../../assets/svg/wand.svg"),
        Tool::Crop => egui::include_image!("../../assets/svg/crop.svg"),
        Tool::Brush => egui::include_image!("../../assets/svg/brush.svg"),
        Tool::Erase => egui::include_image!("../../assets/svg/eraser.svg"),
        Tool::Heal => egui::include_image!("../../assets/svg/bandage.svg"),
        Tool::Clone => egui::include_image!("../../assets/svg/stamp.svg"),
        Tool::Blur => egui::include_image!("../../assets/svg/smudge.svg"),
        Tool::Shape => egui::include_image!("../../assets/svg/shape.svg"),
        Tool::Text => egui::include_image!("../../assets/svg/text.svg"),
        Tool::Dropper => egui::include_image!("../../assets/svg/color-picker.svg"),
        Tool::Hand => egui::include_image!("../../assets/svg/hand.svg"),
        Tool::Zoom => egui::include_image!("../../assets/svg/zoom.svg"),
        Tool::Gradient => {
            let painter = ui.painter();
            for i in 0..16 {
                let shade = (i * 14 + 30) as u8;
                painter.rect_filled(
                    Rect::from_min_size(
                        rect.min + vec2(i as f32 / 16.0 * rect.width(), 0.05 * rect.height()),
                        vec2(rect.width() / 16.0 + 0.5, rect.height() * 0.9),
                    ),
                    0.0,
                    Color32::from_gray(shade),
                );
            }
            painter.rect_stroke(rect, 1.0, Stroke::new(1.35_f32, color), StrokeKind::Inside);
            return;
        }
    };
    svg(ui, source, rect, color);
}

fn svg(ui: &Ui, source: egui::ImageSource<'_>, rect: Rect, color: Color32) {
    egui::Image::new(source).tint(color).paint_at(ui, rect);
}

pub fn disclosure(ui: &mut Ui, collapsed: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(20.0), egui::Sense::click());
    if response.hovered() {
        ui.painter().rect_filled(rect, 4.0, Color32::from_gray(55));
    }
    let center = rect.center();
    let offsets = if collapsed {
        [vec2(-2.0, -4.0), vec2(2.0, 0.0), vec2(-2.0, 4.0)]
    } else {
        [vec2(-4.0, -2.0), vec2(0.0, 2.0), vec2(4.0, -2.0)]
    };
    ui.painter().add(egui::Shape::line(
        offsets.into_iter().map(|offset| center + offset).collect(),
        Stroke::new(1.5_f32, theme::MUTED),
    ));
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::CollapsingHeader,
            ui.is_enabled(),
            !collapsed,
            "Layer contents",
        )
    });
    response.on_hover_text(if collapsed {
        "Expand layers"
    } else {
        "Collapse layers"
    })
}

pub fn eye(ui: &mut Ui, visible: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(20.0), egui::Sense::click());
    let source = if visible {
        egui::include_image!("../../assets/svg/eye.svg")
    } else {
        egui::include_image!("../../assets/svg/eye-off.svg")
    };
    let color = if visible {
        theme::TEXT
    } else {
        Color32::from_gray(80)
    };
    svg(ui, source, rect, color);
    response.on_hover_text("Toggle visibility")
}

pub fn action_button(ui: &mut Ui, kind: &str) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(28.0), egui::Sense::click());
    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect, 5.0, Color32::from_gray(55));
    }
    let source = match kind {
        "new_layer" => egui::include_image!("../../assets/svg/square-plus.svg"),
        "group" => egui::include_image!("../../assets/svg/folder.svg"),
        "adjustment" => egui::include_image!("../../assets/svg/adjustment.svg"),
        "filter" => egui::include_image!("../../assets/svg/fx.svg"),
        "mask" => egui::include_image!("../../assets/svg/mask.svg"),
        "delete_layer" => egui::include_image!("../../assets/svg/trash.svg"),
        _ => return response,
    };
    svg(ui, source, rect.shrink(6.0), theme::MUTED);
    response
}

pub fn lock(ui: &mut Ui, locked: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(16.0, 22.0), egui::Sense::click());
    let source = if locked {
        egui::include_image!("../../assets/svg/lock.svg")
    } else {
        egui::include_image!("../../assets/svg/lock-open.svg")
    };
    let color = if locked { theme::TEXT } else { theme::MUTED };
    svg(
        ui,
        source,
        Rect::from_center_size(rect.center(), Vec2::splat(16.0)),
        color,
    );
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
