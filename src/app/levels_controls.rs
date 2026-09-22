use egui::{Color32, Key, Rect, Sense, Stroke, Ui, pos2, vec2};
use xuan::{document::Adjustment, effects};

use super::{theme, widgets};

pub fn controls(
    ui: &mut Ui,
    range: &mut [f32; 5],
    image: &image::RgbaImage,
    channel: usize,
) -> bool {
    let before = *range;
    histogram(ui, image, channel);
    handles(ui, range, false);
    ui.horizontal(|ui| {
        let maximum = range[2] - 1.0;
        field(ui, "Input black", &mut range[0], 0.0..=maximum, 0);
        let gap =
            ((ui.available_width() - 160.0 - ui.spacing().item_spacing.x * 2.0) / 2.0).max(0.0);
        ui.add_space(gap);
        field(ui, "Gamma", &mut range[1], 0.1..=9.99, 2);
        ui.add_space(gap);
        let minimum = range[0] + 1.0;
        field(ui, "Input white", &mut range[2], minimum..=255.0, 0);
    });
    ui.add_space(4.0);
    let (ramp, _) = ui.allocate_exact_size(vec2(ui.available_width(), 14.0), Sense::hover());
    for i in 0..128 {
        let left = egui::lerp(ramp.x_range(), i as f32 / 128.0);
        let right = egui::lerp(ramp.x_range(), (i + 1) as f32 / 128.0);
        ui.painter().rect_filled(
            Rect::from_min_max(pos2(left, ramp.top()), pos2(right, ramp.bottom())),
            0.0,
            Color32::from_gray((i * 2) as u8),
        );
    }
    handles(ui, range, true);
    ui.horizontal(|ui| {
        field(ui, "Output black", &mut range[3], 0.0..=255.0, 0);
        ui.add_space((ui.available_width() - 80.0 - ui.spacing().item_spacing.x).max(0.0));
        field(ui, "Output white", &mut range[4], 0.0..=255.0, 0);
    });
    ui.horizontal(|ui| {
        if widgets::button(ui, "Auto").clicked()
            && let Adjustment::Levels {
                black,
                gamma,
                white,
                output_black,
                output_white,
            } = effects::auto_levels(image)
        {
            *range = [black, gamma, white, output_black, output_white];
        }
        if widgets::button(ui, "Reset").clicked() {
            *range = xuan::color::DEFAULT_LEVELS;
        }
    });
    ui.label(
        egui::RichText::new("Original pixels · alpha-weighted histogram")
            .size(11.0)
            .color(theme::MUTED),
    );
    before != *range
}

fn field(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    decimals: usize,
) {
    ui.allocate_ui_with_layout(
        vec2(80.0, 42.0),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            ui.spacing_mut().item_spacing.y = 5.0;
            ui.label(egui::RichText::new(label).size(11.0).color(theme::MUTED));
            ui.add(
                widgets::Number::new(value)
                    .range(range)
                    .max_decimals(decimals)
                    .speed(if decimals == 0 { 1.0 } else { 0.01 }),
            );
        },
    );
}

fn histogram(ui: &mut Ui, image: &image::RgbaImage, channel: usize) {
    let mut bins = [0.0_f32; 256];
    for pixel in image.pixels() {
        let index = match channel {
            1..=3 => pixel[channel - 1] as usize,
            _ => (0.2126 * pixel[0] as f32 + 0.7152 * pixel[1] as f32 + 0.0722 * pixel[2] as f32)
                .round() as usize,
        };
        bins[index] += pixel[3] as f32 / 255.0;
    }
    let peak = bins.iter().copied().fold(1.0_f32, f32::max);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 150.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, Color32::from_gray(27));
    let color = match channel {
        1 => Color32::from_rgb(220, 102, 99),
        2 => Color32::from_rgb(110, 192, 117),
        3 => Color32::from_rgb(106, 151, 229),
        _ => Color32::from_gray(187),
    };
    for (index, count) in bins.into_iter().enumerate() {
        let x = rect.left() + (index as f32 + 0.5) / 256.0 * rect.width();
        ui.painter().line_segment(
            [
                pos2(x, rect.bottom()),
                pos2(
                    x,
                    rect.bottom() - (count / peak).sqrt() * (rect.height() - 4.0),
                ),
            ],
            Stroke::new(rect.width() / 256.0, color),
        );
    }
}

fn handles(ui: &mut Ui, range: &mut [f32; 5], output: bool) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::hover());
    let indices: &[usize] = if output { &[3, 4] } else { &[0, 1, 2] };
    for &index in indices {
        let position = if index == 1 {
            range[0] + (range[2] - range[0]) * 0.5_f32.powf(range[1])
        } else {
            range[index]
        };
        let x = rect.left() + position / 255.0 * rect.width();
        let hit = Rect::from_center_size(pos2(x, rect.center().y), vec2(18.0, 20.0));
        let mut response = ui.interact(
            hit,
            ui.id().with(("level_handle", index)),
            Sense::click_and_drag(),
        );
        let name = [
            "Input black",
            "Gamma",
            "Input white",
            "Output black",
            "Output white",
        ][index];
        response
            .widget_info(|| egui::WidgetInfo::slider(ui.is_enabled(), range[index] as f64, name));
        if response.clicked() {
            response.request_focus();
        }
        if (response.dragged() || response.is_pointer_button_down_on())
            && let Some(pointer) = response.interact_pointer_pos()
        {
            let value = ((pointer.x - rect.left()) / rect.width() * 255.0).clamp(0.0, 255.0);
            range[index] = match index {
                0 => value.round().min(range[2] - 1.0),
                2 => value.round().max(range[0] + 1.0),
                1 => (((value - range[0]) / (range[2] - range[0]))
                    .clamp(0.001, 0.999)
                    .ln()
                    / 0.5_f32.ln())
                .clamp(0.1, 9.99),
                _ => value.round(),
            };
        }
        if response.has_focus() {
            let delta = ui.input(|i| {
                (i.key_pressed(Key::ArrowRight) as i32 - i.key_pressed(Key::ArrowLeft) as i32)
                    as f32
                    * if i.modifiers.shift { 10.0 } else { 1.0 }
            });
            range[index] += delta * if index == 1 { 0.01 } else { 1.0 };
            range[index] = match index {
                0 => range[0].clamp(0.0, range[2] - 1.0),
                1 => range[1].clamp(0.1, 9.99),
                2 => range[2].clamp(range[0] + 1.0, 255.0),
                _ => range[index].clamp(0.0, 255.0),
            };
        }
        let bounds = match index {
            0 => 0.0..=f64::from(range[2] - 1.0),
            1 => 0.1..=9.99,
            2 => f64::from(range[0] + 1.0)..=255.0,
            _ => 0.0..=255.0,
        };
        widgets::wheel_value(
            ui,
            &mut response,
            &mut range[index],
            bounds,
            if index == 1 { 0.01 } else { 1.0 },
            Some(if index == 1 { 2 } else { 0 }),
        );
        let color = match index {
            0 | 3 => Color32::BLACK,
            1 => Color32::from_gray(130),
            _ => Color32::WHITE,
        };
        ui.painter().add(egui::Shape::convex_polygon(
            vec![
                pos2(x, rect.top()),
                pos2(x - 5.0, rect.top() + 9.0),
                pos2(x + 5.0, rect.top() + 9.0),
            ],
            color,
            Stroke::new(
                1.0_f32,
                if response.has_focus() {
                    theme::ACCENT
                } else {
                    theme::MUTED
                },
            ),
        ));
        response.on_hover_text(name);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_handle_wheels_use_field_steps_and_bounds() {
        for index in 0..5 {
            let context = egui::Context::default();
            let mut range = [64.0, 1.0, 192.0, 16.0, 240.0];
            let initial = range[index];
            let position = if index == 1 { 128.0 } else { initial };
            let pos = pos2(8.0 + position / 255.0 * 400.0, 14.0);
            for delta in [0.0, 1.0, -1000.0] {
                let _ = context.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::Pos2::ZERO,
                            vec2(416.0, 200.0),
                        )),
                        events: vec![
                            egui::Event::PointerMoved(pos),
                            egui::Event::MouseWheel {
                                unit: egui::MouseWheelUnit::Line,
                                delta: vec2(0.0, delta),
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            handles(ui, &mut range, index >= 3);
                        });
                    },
                );
                let expected = if delta == 0.0 {
                    initial
                } else if delta > 0.0 {
                    initial + if index == 1 { 0.01 } else { 1.0 }
                } else {
                    [0.0, 0.1, 65.0, 0.0, 0.0][index]
                };
                assert!((range[index] - expected).abs() < 1e-6, "{index}: {range:?}");
            }
        }
    }

    #[test]
    fn histogram_handles_drag_and_keep_input_bounds_ordered() {
        let context = egui::Context::default();
        super::super::theme::apply(&context);
        let mut range = xuan::color::DEFAULT_LEVELS;
        let mut draw = |position: egui::Pos2, pressed: Option<bool>| {
            let mut events = vec![egui::Event::PointerMoved(position)];
            if let Some(pressed) = pressed {
                events.push(egui::Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
            }
            let _ = context.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(416.0, 200.0))),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        handles(ui, &mut range, false);
                    });
                },
            );
            range
        };
        draw(pos2(8.0, 14.0), None);
        draw(pos2(8.0, 14.0), Some(true));
        let moved = draw(pos2(108.0, 14.0), None);
        assert!((moved[0] - 64.0).abs() < 1.0, "{moved:?}");
        let clamped = draw(pos2(600.0, 14.0), None);
        assert_eq!(clamped[0], clamped[2] - 1.0);
        draw(pos2(600.0, 14.0), Some(false));
    }
}
