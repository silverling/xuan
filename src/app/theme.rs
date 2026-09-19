use egui::{Color32, CornerRadius, FontId, Stroke, TextStyle};

// ContentView.swift uses 14% white; AppKit's elevated controls sit above it.
pub const PANEL: Color32 = Color32::from_rgb(36, 36, 36);
pub const CANVAS: Color32 = Color32::from_rgb(29, 29, 29);
pub const FIELD: Color32 = Color32::from_rgb(30, 30, 30);
pub const DIVIDER: Color32 = Color32::from_rgb(54, 54, 54);
pub const MUTED: Color32 = Color32::from_rgb(154, 154, 157);
pub const TEXT: Color32 = Color32::from_rgb(235, 235, 237);
pub const ACCENT: Color32 = Color32::from_rgb(10, 132, 255);
pub const TITLEBAR: Color32 = Color32::from_rgb(45, 45, 45);
pub const BUTTON_RADIUS: u8 = 5;

pub fn window_corner_radius(ctx: &egui::Context) -> u8 {
    let fills_screen = ctx.input(|i| {
        i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)
    });
    if fills_screen { 0 } else { 12 }
}

pub fn apply(ctx: &egui::Context) {
    // Inter is an OFL-licensed, portable substitute for the macOS system font.
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Inter".into(),
        egui::FontData::from_static(include_bytes!("../../assets/fonts/InterVariable.ttf")).into(),
    );
    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .insert(0, "Inter".into());
    ctx.set_fonts(fonts);

    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = PANEL;
    style.visuals.window_fill = Color32::from_gray(43);
    style.visuals.extreme_bg_color = FIELD;
    style.visuals.text_edit_bg_color = Some(FIELD);
    style.visuals.faint_bg_color = Color32::from_gray(40);
    style.visuals.override_text_color = Some(TEXT);
    style.visuals.window_corner_radius = CornerRadius::same(10);
    style.visuals.menu_corner_radius = CornerRadius::same(7);
    style.visuals.window_stroke = Stroke::new(1.0_f32, Color32::from_gray(83));
    style.visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 12],
        blur: 32,
        spread: 2,
        color: Color32::from_black_alpha(125),
    };
    style.visuals.popup_shadow = egui::epaint::Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 1,
        color: Color32::from_black_alpha(110),
    };
    style.visuals.selection.bg_fill = ACCENT.gamma_multiply(0.65);
    style.visuals.selection.stroke = Stroke::new(1.0_f32, TEXT);
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, DIVIDER);
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    for widget in [
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(BUTTON_RADIUS);
        widget.fg_stroke = Stroke::new(1.0_f32, TEXT);
        widget.expansion = 0.0;
    }
    style.visuals.widgets.inactive.bg_fill = Color32::from_gray(72);
    style.visuals.widgets.inactive.weak_bg_fill = Color32::from_gray(62);
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(0.7_f32, Color32::from_gray(91));
    style.visuals.widgets.hovered.bg_fill = Color32::from_gray(88);
    style.visuals.widgets.hovered.weak_bg_fill = Color32::from_gray(78);
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(0.7_f32, Color32::from_gray(112));
    style.visuals.widgets.active.bg_fill = Color32::from_gray(52);
    style.visuals.widgets.active.weak_bg_fill = Color32::from_gray(52);
    style.visuals.widgets.open.weak_bg_fill = Color32::from_gray(68);
    style.spacing.item_spacing = egui::vec2(8.0, 8.0);
    style.spacing.button_padding = egui::vec2(10.0, 3.0);
    style.spacing.interact_size = egui::vec2(22.0, 22.0);
    style.spacing.icon_width = 14.0;
    style.spacing.icon_width_inner = 8.0;
    style.spacing.slider_width = 100.0;
    style.spacing.slider_rail_height = 3.0;
    style.spacing.menu_margin = egui::Margin::same(5);
    style.spacing.window_margin = egui::Margin::same(24);
    style.spacing.scroll.bar_width = 6.0;
    for (text, size) in [
        (TextStyle::Body, 12.0),
        (TextStyle::Button, 12.0),
        (TextStyle::Small, 11.0),
        (TextStyle::Heading, 21.0),
        (TextStyle::Monospace, 12.0),
    ] {
        style.text_styles.insert(text, FontId::proportional(size));
    }
    ctx.set_style(style);
}

pub fn frame() -> egui::Frame {
    egui::Frame::new()
        .fill(PANEL)
        .inner_margin(egui::Margin::symmetric(18, 10))
}

pub fn menu_style(style: &mut egui::Style) {
    egui::containers::menu::menu_style(style);
    style.spacing.button_padding = egui::vec2(9.0, 3.0);
    style.spacing.item_spacing.y = 2.0;
    for widget in [
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        widget.corner_radius = CornerRadius::same(4);
        widget.weak_bg_fill = ACCENT;
        widget.bg_fill = ACCENT;
    }
}
