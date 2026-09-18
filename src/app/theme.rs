use egui::{Color32, CornerRadius, FontId, Stroke, TextStyle};

pub const PANEL: Color32 = Color32::from_rgb(36, 36, 36);
pub const CANVAS: Color32 = Color32::from_rgb(29, 29, 29);
pub const DIVIDER: Color32 = Color32::from_rgb(57, 57, 57);
pub const MUTED: Color32 = Color32::from_rgb(155, 155, 158);
pub const TEXT: Color32 = Color32::from_rgb(224, 224, 228);
pub const ACCENT: Color32 = Color32::from_rgb(106, 168, 233);

pub fn apply(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = PANEL;
    style.visuals.window_fill = Color32::from_gray(43);
    style.visuals.extreme_bg_color = Color32::from_gray(28);
    style.visuals.faint_bg_color = Color32::from_gray(40);
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.window_corner_radius = CornerRadius::same(12);
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, DIVIDER);
    style.visuals.window_stroke = Stroke::new(1.0_f32, Color32::from_gray(76));
    style.visuals.selection.bg_fill = Color32::from_gray(67);
    style.visuals.selection.stroke = Stroke::new(1.0_f32, TEXT);
    for widget in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
    ] {
        widget.corner_radius = CornerRadius::same(7);
        widget.fg_stroke = Stroke::new(1.3_f32, TEXT);
    }
    style.visuals.widgets.inactive.bg_fill = Color32::from_gray(51);
    style.visuals.widgets.inactive.weak_bg_fill = Color32::from_gray(43);
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, Color32::from_gray(69));
    style.visuals.widgets.hovered.bg_fill = Color32::from_gray(68);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_gray(60);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, Color32::from_gray(100));
    style.visuals.widgets.active.bg_fill = Color32::from_gray(78);
    style.visuals.widgets.active.weak_bg_fill = Color32::from_gray(73);
    style.spacing.item_spacing = egui::vec2(9.0, 8.0);
    style.spacing.button_padding = egui::vec2(11.0, 6.0);
    style.spacing.interact_size = egui::vec2(24.0, 26.0);
    style.spacing.slider_width = 100.0;
    style
        .text_styles
        .insert(TextStyle::Body, FontId::proportional(12.0));
    style
        .text_styles
        .insert(TextStyle::Button, FontId::proportional(12.0));
    style
        .text_styles
        .insert(TextStyle::Small, FontId::proportional(11.0));
    style
        .text_styles
        .insert(TextStyle::Heading, FontId::proportional(21.0));
    ctx.set_style(style);
}

pub fn frame() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .inner_margin(egui::Margin::symmetric(12, 7))
}
