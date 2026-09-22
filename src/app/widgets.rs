//! AppKit-like controls. Keep egui's input, focus and accessibility behavior where
//! possible, and paint the small details its stock theme cannot express.
use std::ops::RangeInclusive;

use egui::{
    Color32, CornerRadius, FontId, Rect, Response, Sense, Stroke, StrokeKind, Ui, Widget, pos2,
    vec2,
};

use super::theme;

pub fn gradient(ui: &Ui, rect: Rect, radius: f32, top: Color32, bottom: Color32) {
    if !rect.is_positive() {
        return;
    }

    // Reuse egui's rounded outline and pixel-scaled feathering. Raw triangle
    // meshes are passed through unchanged by the UI renderer.
    let mut mesh = egui::Mesh::default();
    let mut tessellator = egui::epaint::Tessellator::new(
        ui.pixels_per_point(),
        ui.ctx().tessellation_options(|options| *options),
        [1, 1],
        Vec::new(),
    );
    tessellator.tessellate_rect(
        &egui::epaint::RectShape::filled(rect, radius, Color32::WHITE),
        &mut mesh,
    );

    for vertex in &mut mesh.vertices {
        let t = ((vertex.pos.y - rect.top()) / rect.height()).clamp(0.0, 1.0);
        let color = Color32::from_rgba_premultiplied(
            egui::lerp(top.r() as f32..=bottom.r() as f32, t) as u8,
            egui::lerp(top.g() as f32..=bottom.g() as f32, t) as u8,
            egui::lerp(top.b() as f32..=bottom.b() as f32, t) as u8,
            egui::lerp(top.a() as f32..=bottom.a() as f32, t) as u8,
        );
        // Preserve edge coverage when tinting the white fill with the gradient.
        vertex.color = color.gamma_multiply_u8(vertex.color.a());
    }
    ui.painter().add(mesh);
}

pub fn bezel(ui: &Ui, response: &Response, radius: f32, primary: bool) {
    let rect = response.rect;
    let pressed = response.is_pointer_button_down_on();
    let (top, bottom) = if primary {
        if pressed {
            (
                Color32::from_rgb(30, 103, 210),
                Color32::from_rgb(24, 89, 183),
            )
        } else {
            (
                Color32::from_rgb(65, 155, 255),
                Color32::from_rgb(22, 112, 231),
            )
        }
    } else {
        let lift = if pressed {
            -12
        } else if response.hovered() {
            8
        } else {
            0
        };
        (
            Color32::from_gray((86 + lift) as u8),
            Color32::from_gray((67 + lift) as u8),
        )
    };
    ui.painter().rect_filled(
        rect.translate(vec2(0.0, 1.0)),
        radius,
        Color32::from_black_alpha(65),
    );
    gradient(ui, rect, radius, top, bottom);
    ui.painter().rect_stroke(
        rect.shrink(0.5),
        radius,
        Stroke::new(1.0_f32, Color32::from_white_alpha(28)),
        StrokeKind::Inside,
    );
    focus_ring(ui, response, radius);
}

fn focus_ring(ui: &Ui, response: &Response, radius: f32) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.expand(2.0),
            radius + 2.0,
            Stroke::new(2.0_f32, theme::ACCENT.gamma_multiply(0.8)),
            StrokeKind::Outside,
        );
    }
}

pub struct Button {
    label: String,
    primary: bool,
    size: egui::Vec2,
}

impl Button {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            primary: false,
            size: vec2(0.0, 22.0),
        }
    }
    pub fn primary(mut self) -> Self {
        self.primary = true;
        self
    }
    pub fn min_size(mut self, size: egui::Vec2) -> Self {
        self.size = size;
        self
    }
}

impl Widget for Button {
    fn ui(self, ui: &mut Ui) -> Response {
        let galley = ui.painter().layout_no_wrap(
            self.label.clone(),
            FontId::proportional(12.0),
            theme::TEXT,
        );
        let size = vec2(galley.size().x + 24.0, 22.0).max(self.size);
        let (rect, response) = ui.allocate_exact_size(size, Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), &self.label)
        });
        if ui.is_rect_visible(rect) {
            bezel(ui, &response, theme::BUTTON_RADIUS as f32, self.primary);
            ui.painter()
                .galley(rect.center() - galley.size() / 2.0, galley, theme::TEXT);
        }
        response
    }
}

pub fn button(ui: &mut Ui, label: impl Into<String>) -> Response {
    ui.add(Button::new(label))
}
pub fn primary_button(ui: &mut Ui, label: impl Into<String>) -> Response {
    ui.add(Button::new(label).primary())
}

/// Consume vertical wheel motion over a control, leaving horizontal scrolling to its parent.
fn wheel_steps(ui: &Ui, response: &Response) -> f64 {
    let id = response.id.with("wheel_remainder");
    if !response.enabled() || !response.hovered() {
        ui.data_mut(|data| data.remove::<f64>(id));
        return 0.0;
    }
    let delta = ui.input_mut(|input| {
        if input.smooth_scroll_delta.y == 0.0 {
            return 0.0;
        }
        // Consume the smoothing tail too, so the containing panel stays still.
        input.smooth_scroll_delta.y = 0.0;
        std::mem::take(&mut input.raw_scroll_delta.y)
    });
    let line_height = ui
        .ctx()
        .options(|options| options.input_options.line_scroll_speed);
    ui.data_mut(|data| {
        let remainder = data.get_temp_mut_or_default::<f64>(id);
        *remainder += f64::from(delta / line_height);
        let steps = remainder.trunc();
        *remainder -= steps;
        steps
    })
}

pub(super) fn wheel_value<N: egui::emath::Numeric>(
    ui: &Ui,
    response: &mut Response,
    value: &mut N,
    range: RangeInclusive<f64>,
    step: f64,
    decimals: Option<usize>,
) {
    let steps = wheel_steps(ui, response);
    if steps == 0.0 {
        return;
    }
    let old = value.to_f64();
    let step = if N::INTEGRAL { step.max(1.0) } else { step };
    let mut new = old + steps * step;
    if let Some(decimals) = decimals {
        new = egui::emath::round_to_decimals(new, decimals);
    }
    *value = N::from_f64(new.clamp(
        range.start().min(*range.end()),
        range.start().max(*range.end()),
    ));
    if value.to_f64() != old {
        response.mark_changed();
        // DragValue caches its text while focused. Refresh it after a wheel edit.
        ui.data_mut(|data| data.remove::<String>(response.id));
        ui.ctx().request_repaint();
    }
}

/// A recessed field in place of DragValue's raised button.
pub struct Number<'a, N> {
    value: &'a mut N,
    speed: f64,
    range: RangeInclusive<f64>,
    suffix: String,
    max_decimals: Option<usize>,
}
impl<'a, N: egui::emath::Numeric> Number<'a, N> {
    pub fn new(value: &'a mut N) -> Self {
        Self {
            value,
            speed: if N::INTEGRAL { 0.25 } else { 1.0 },
            range: N::MIN.to_f64()..=N::MAX.to_f64(),
            suffix: String::new(),
            max_decimals: N::INTEGRAL.then_some(0),
        }
    }
    pub fn speed(mut self, speed: impl Into<f64>) -> Self {
        self.speed = speed.into();
        self
    }
    pub fn range<T: egui::emath::Numeric>(mut self, range: RangeInclusive<T>) -> Self {
        self.range = range.start().to_f64()..=range.end().to_f64();
        self
    }
    /// Show a unit inside the field while idle; egui edits the numeric text alone.
    pub fn suffix(mut self, suffix: impl ToString) -> Self {
        self.suffix = suffix.to_string();
        self
    }
    pub fn max_decimals(mut self, decimals: usize) -> Self {
        self.max_decimals = Some(decimals);
        self
    }
}
impl<N: egui::emath::Numeric> Widget for Number<'_, N> {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.scope(|ui| {
            ui.spacing_mut().button_padding = vec2(6.0, 3.0);
            ui.spacing_mut().interact_size = vec2(44.0, 22.0);
            let visuals = ui.visuals_mut();
            visuals.selection.bg_fill = theme::ACCENT.gamma_multiply(0.5);
            for widget in [
                &mut visuals.widgets.inactive,
                &mut visuals.widgets.hovered,
                &mut visuals.widgets.active,
            ] {
                widget.corner_radius = CornerRadius::same(4);
                widget.bg_fill = theme::FIELD;
                widget.weak_bg_fill = theme::FIELD;
                widget.bg_stroke = Stroke::new(1.0_f32, Color32::from_gray(83));
                widget.expansion = 0.0;
            }
            let mut number = egui::DragValue::new(&mut *self.value)
                .speed(self.speed)
                .range(self.range.clone())
                .suffix(self.suffix);
            if let Some(decimals) = self.max_decimals {
                number = number.max_decimals(decimals);
            }
            let mut response = ui.add(number);
            wheel_value(
                ui,
                &mut response,
                self.value,
                self.range,
                self.speed,
                self.max_decimals,
            );
            response
        })
        .inner
    }
}

pub fn checkbox(ui: &mut Ui, value: &mut bool, label: &str) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(label.into(), FontId::proportional(12.0), theme::TEXT);
    let width = 14.0
        + if label.is_empty() {
            0.0
        } else {
            6.0 + galley.size().x
        };
    let (rect, mut response) = ui.allocate_exact_size(vec2(width, 22.0), Sense::click());
    if response.clicked() {
        *value = !*value;
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Checkbox, ui.is_enabled(), *value, label)
    });
    let box_rect =
        Rect::from_center_size(pos2(rect.left() + 7.0, rect.center().y), vec2(14.0, 14.0));
    let (top, bottom) = if *value {
        (
            Color32::from_rgb(62, 151, 255),
            Color32::from_rgb(24, 113, 228),
        )
    } else if response.hovered() {
        (Color32::from_gray(95), Color32::from_gray(71))
    } else {
        (Color32::from_gray(78), Color32::from_gray(57))
    };
    gradient(ui, box_rect, 3.5, top, bottom);
    ui.painter().rect_stroke(
        box_rect,
        3.5,
        Stroke::new(0.7_f32, Color32::from_white_alpha(45)),
        StrokeKind::Inside,
    );
    if *value {
        ui.painter().add(egui::Shape::line(
            vec![
                box_rect.min + vec2(3.0, 7.0),
                box_rect.min + vec2(6.0, 10.0),
                box_rect.min + vec2(11.0, 4.0),
            ],
            Stroke::new(1.6_f32, Color32::WHITE),
        ));
    }
    ui.painter().galley(
        pos2(rect.left() + 20.0, rect.center().y - galley.size().y / 2.0),
        galley,
        theme::TEXT,
    );
    focus_ring(ui, &response, 4.0);
    response
}

pub struct Slider<'a, N> {
    value: &'a mut N,
    range: RangeInclusive<N>,
    label: String,
    suffix: String,
    logarithmic: bool,
    percentage: bool,
}
impl<'a, N: egui::emath::Numeric> Slider<'a, N> {
    pub fn new(value: &'a mut N, range: RangeInclusive<N>) -> Self {
        Self {
            value,
            range,
            label: String::new(),
            suffix: String::new(),
            logarithmic: false,
            percentage: false,
        }
    }
    pub fn text(mut self, label: impl ToString) -> Self {
        self.label = label.to_string();
        self
    }
    pub fn suffix(mut self, suffix: impl ToString) -> Self {
        self.suffix = suffix.to_string();
        self
    }
    pub fn logarithmic(mut self, logarithmic: bool) -> Self {
        self.logarithmic = logarithmic;
        self
    }
    pub fn percentage(mut self) -> Self {
        self.percentage = true;
        self.suffix = "%".into();
        self
    }
}
impl<N: egui::emath::Numeric> Widget for Slider<'_, N> {
    fn ui(self, ui: &mut Ui) -> Response {
        let old = self.value.to_f64();
        let mut value = old;
        let range = self.range.start().to_f64()..=self.range.end().to_f64();
        let scale = if self.percentage { 100.0 } else { 1.0 };
        let decimals = if N::INTEGRAL || self.percentage || range.end() - range.start() > 20.0 {
            0
        } else {
            2
        };
        let speed = if decimals == 0 { 1.0 } else { 0.01 };
        let result = ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if !self.label.is_empty() {
                ui.add_sized(
                    [82.0, 22.0],
                    egui::Label::new(&self.label).halign(egui::Align::Min),
                );
            }
            // An invisible native slider retains keyboard navigation and range semantics.
            // Only its painting is replaced; input remains enabled.
            let mut response = ui
                .scope(|ui| {
                    ui.set_opacity(0.0);
                    ui.spacing_mut().interact_size.y = 18.0;
                    let mut slider = egui::Slider::new(&mut value, range.clone())
                        .show_value(false)
                        .logarithmic(self.logarithmic)
                        .handle_shape(egui::style::HandleShape::Circle);
                    if N::INTEGRAL {
                        slider = slider.integer();
                    }
                    ui.add(slider)
                })
                .inner;
            wheel_value(
                ui,
                &mut response,
                &mut value,
                range.clone(),
                speed / scale,
                Some(decimals + if self.percentage { 2 } else { 0 }),
            );
            let r = response.rect;
            let radius = r.height() / 2.5;
            let x_range = (r.left() + radius)..=(r.right() - radius);
            let t = if self.logarithmic && *range.start() > 0.0 {
                (value.ln() - range.start().ln()) / (range.end().ln() - range.start().ln())
            } else {
                (value - range.start()) / (range.end() - range.start())
            };
            let x = egui::lerp(x_range.clone(), t.clamp(0.0, 1.0) as f32);
            let rail = Rect::from_min_max(
                pos2(*x_range.start(), r.center().y - 1.5),
                pos2(*x_range.end(), r.center().y + 1.5),
            );
            ui.painter().rect_filled(
                rail.translate(vec2(0.0, 1.0)),
                2.0,
                Color32::from_white_alpha(12),
            );
            ui.painter().rect_filled(rail, 2.0, Color32::from_gray(70));
            ui.painter().rect_filled(
                Rect::from_min_max(rail.min, pos2(x, rail.bottom())),
                2.0,
                theme::ACCENT,
            );
            let thumb = Rect::from_center_size(pos2(x, r.center().y), vec2(14.0, 14.0));
            ui.painter().circle_filled(
                thumb.center() + vec2(0.0, 1.0),
                7.5,
                Color32::from_black_alpha(85),
            );
            gradient(
                ui,
                thumb,
                7.0,
                Color32::from_gray(255),
                Color32::from_gray(if response.is_pointer_button_down_on() {
                    190
                } else {
                    221
                }),
            );
            ui.painter().circle_stroke(
                thumb.center(),
                7.0,
                Stroke::new(0.6_f32, Color32::from_gray(175)),
            );
            focus_ring(ui, &response, 4.0);
            let mut display = value * scale;
            let number = ui.add(
                Number::new(&mut display)
                    .range(range.start() * scale..=range.end() * scale)
                    .speed(speed)
                    .suffix(self.suffix)
                    .max_decimals(decimals),
            );
            if number.changed() {
                value = display / scale;
            }
            response.union(number)
        });
        *self.value = N::from_f64(value);
        let mut response = result.inner;
        if self.value.to_f64() != old {
            response.mark_changed();
        }
        response
    }
}

pub fn segmented<T: Copy + PartialEq>(
    ui: &mut Ui,
    value: &mut T,
    options: &[(T, &str)],
) -> Response {
    let result = ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        let mut responses = Vec::new();
        for &(option, label) in options {
            let galley =
                ui.painter()
                    .layout_no_wrap(label.into(), FontId::proportional(12.0), theme::TEXT);
            let (rect, mut response) =
                ui.allocate_exact_size(vec2(galley.size().x + 20.0, 22.0), Sense::click());
            if response.clicked() && *value != option {
                *value = option;
                response.mark_changed();
            }
            response.widget_info(|| {
                egui::WidgetInfo::selected(
                    egui::WidgetType::SelectableLabel,
                    ui.is_enabled(),
                    *value == option,
                    label,
                )
            });
            responses.push((rect, response, galley, *value == option));
        }
        let rect = responses
            .iter()
            .fold(Rect::NOTHING, |rect, (r, ..)| rect.union(*r));
        gradient(
            ui,
            rect,
            5.0,
            Color32::from_gray(47),
            Color32::from_gray(43),
        );
        ui.painter().rect_stroke(
            rect,
            5.0,
            Stroke::new(1.0_f32, Color32::from_gray(66)),
            StrokeKind::Inside,
        );
        let mut combined = ui.interact(rect, ui.next_auto_id(), Sense::hover());
        for (index, (rect, response, galley, selected)) in responses.into_iter().enumerate() {
            if selected {
                bezel(
                    ui,
                    &Response {
                        rect: rect.shrink(1.0),
                        ..response.clone()
                    },
                    4.0,
                    false,
                );
            } else if index > 0 {
                ui.painter().line_segment(
                    [
                        rect.left_top() + vec2(0.0, 5.0),
                        rect.left_bottom() - vec2(0.0, 5.0),
                    ],
                    Stroke::new(1.0_f32, Color32::from_gray(68)),
                );
            }
            ui.painter()
                .galley(rect.center() - galley.size() / 2.0, galley, theme::TEXT);
            combined = combined.union(response);
        }
        combined
    });
    result.inner
}

/// Rounded pop-up with the paired AppKit chevrons; the menu itself stays native egui.
pub struct PopUp {
    id: egui::Id,
    text: String,
    width: f32,
}
impl PopUp {
    pub fn from_id_salt(id: impl std::hash::Hash) -> Self {
        Self {
            id: egui::Id::new(id),
            text: String::new(),
            width: 160.0,
        }
    }
    pub fn selected_text(mut self, text: impl ToString) -> Self {
        self.text = text.to_string();
        self
    }
    pub fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }
    pub fn show_ui<R>(
        self,
        ui: &mut Ui,
        content: impl FnOnce(&mut Ui) -> R,
    ) -> Option<egui::InnerResponse<R>> {
        let response = self.button(ui);
        egui::Popup::menu(&response).width(self.width).show(content)
    }

    pub fn button(&self, ui: &mut Ui) -> Response {
        let (rect, _) = ui.allocate_exact_size(vec2(self.width, 22.0), Sense::hover());
        let response = ui.interact(rect, ui.make_persistent_id(self.id), Sense::click());
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, ui.is_enabled(), &self.text)
        });
        bezel(ui, &response, theme::BUTTON_RADIUS as f32, false);
        let painter = ui.painter().with_clip_rect(Rect::from_min_max(
            rect.min + vec2(10.0, 0.0),
            rect.max - vec2(26.0, 0.0),
        ));
        painter.text(
            pos2(rect.left() + 10.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            &self.text,
            FontId::proportional(12.0),
            theme::TEXT,
        );
        let center = pos2(rect.right() - 12.0, rect.center().y);
        for direction in [-1.0, 1.0] {
            ui.painter().add(egui::Shape::line(
                vec![
                    center + vec2(-3.0, direction * 1.5),
                    center + vec2(0.0, direction * 4.0),
                    center + vec2(3.0, direction * 1.5),
                ],
                Stroke::new(1.2_f32, theme::TEXT),
            ));
        }
        response
    }
}

pub fn color_well(ui: &mut Ui, color: &mut [u8; 4]) -> Response {
    let (rect, mut response) = ui.allocate_exact_size(vec2(34.0, 20.0), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::ColorButton, ui.is_enabled(), "Color")
    });
    ui.painter().rect_filled(rect, 4.0, Color32::BLACK);
    checkerboard(ui, rect.shrink(2.0), 4.0);
    ui.painter().rect_filled(
        rect.shrink(2.0),
        2.0,
        Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]),
    );
    ui.painter().rect_stroke(
        rect.shrink(1.5),
        3.0,
        Stroke::new(1.0_f32, Color32::from_gray(225)),
        StrokeKind::Inside,
    );
    focus_ring(ui, &response, 4.0);
    egui::Popup::menu(&response)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show(|ui| {
            let mut hsva = egui::ecolor::Hsva::from_srgba_unmultiplied(*color);
            if egui::color_picker::color_picker_hsva_2d(
                ui,
                &mut hsva,
                egui::color_picker::Alpha::BlendOrAdditive,
            ) {
                *color = hsva.to_srgba_unmultiplied();
                response.mark_changed();
            }
        });
    response
}

pub fn checkerboard(ui: &Ui, rect: Rect, cell: f32) {
    ui.painter().rect_filled(rect, 2.0, Color32::from_gray(115));
    for row in 0..(rect.height() / cell).ceil() as usize {
        for col in 0..(rect.width() / cell).ceil() as usize {
            if (row + col) % 2 == 0 {
                ui.painter().rect_filled(
                    Rect::from_min_size(
                        rect.min + vec2(col as f32 * cell, row as f32 * cell),
                        vec2(cell, cell),
                    )
                    .intersect(rect),
                    0.0,
                    Color32::from_gray(160),
                );
            }
        }
    }
}

pub fn project_tab(ui: &mut Ui, title: &str, dirty: bool, selected: bool) -> (Response, bool) {
    let galley = ui
        .painter()
        .layout_no_wrap(title.into(), FontId::proportional(12.0), theme::TEXT);
    let width = galley.size().x.clamp(35.0, 155.0) + 40.0 + if dirty { 9.0 } else { 0.0 };
    let (rect, _) = ui.allocate_exact_size(vec2(width, 28.0), Sense::hover());
    let close_rect = Rect::from_min_max(pos2(rect.right() - 22.0, rect.top()), rect.max);
    let response = ui.interact(
        Rect::from_min_max(rect.min, close_rect.left_bottom()),
        ui.next_auto_id().with("tab"),
        Sense::click(),
    );
    let close = ui
        .interact(close_rect, response.id.with("close"), Sense::click())
        .on_hover_text(format!("Close {title}"));
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            title,
        )
    });
    ui.painter().rect_filled(
        rect,
        theme::BUTTON_RADIUS,
        Color32::from_white_alpha(if selected {
            31
        } else if response.hovered() {
            19
        } else {
            9
        }),
    );
    ui.painter().rect_stroke(
        rect,
        theme::BUTTON_RADIUS,
        Stroke::new(
            1.0_f32,
            Color32::from_white_alpha(if selected { 56 } else { 20 }),
        ),
        StrokeKind::Inside,
    );
    if dirty {
        ui.painter()
            .circle_filled(pos2(rect.left() + 13.0, rect.center().y), 2.5, theme::TEXT);
    }
    let text_rect = Rect::from_min_max(
        rect.min + vec2(if dirty { 21.0 } else { 11.0 }, 0.0),
        close_rect.left_bottom(),
    );
    ui.painter().with_clip_rect(text_rect).galley(
        pos2(text_rect.left(), rect.center().y - galley.size().y / 2.0),
        galley,
        theme::TEXT,
    );
    if close.hovered() {
        ui.painter()
            .circle_filled(close_rect.center(), 7.0, Color32::from_white_alpha(22));
    }
    let center = close_rect.center() - vec2(1.0, 0.0);
    let stroke = Stroke::new(1.0_f32, theme::MUTED);
    ui.painter()
        .line_segment([center - vec2(2.5, 2.5), center + vec2(2.5, 2.5)], stroke);
    ui.painter()
        .line_segment([center + vec2(-2.5, 2.5), center + vec2(2.5, -2.5)], stroke);
    (response.on_hover_text(title), close.clicked())
}

/// Floating utility panel: compact centered title, a close control on the left,
/// and 24-point content insets, as in FloatingPanelController / the SwiftUI sheets.
pub struct Window<'a> {
    title: String,
    open: Option<&'a mut bool>,
    width: f32,
}
impl<'a> Window<'a> {
    pub fn new(title: impl ToString) -> Self {
        Self {
            title: title.to_string(),
            open: None,
            width: 410.0,
        }
    }
    pub fn open(mut self, open: &'a mut bool) -> Self {
        self.open = Some(open);
        self
    }
    pub fn default_width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }
    pub fn show<R>(mut self, ctx: &egui::Context, content: impl FnOnce(&mut Ui) -> R) {
        if self.open.as_deref() == Some(&false) {
            return;
        }
        let mut close = false;
        egui::Window::new(&self.title)
            .title_bar(false)
            .auto_sized()
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .default_width(self.width)
            .frame(egui::Frame::window(&ctx.style()).inner_margin(0))
            .show(ctx, |ui| {
                ui.set_width(self.width);
                ui.spacing_mut().item_spacing.y = 0.0;
                let (rect, response) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::hover());
                ui.painter().rect_filled(
                    rect,
                    CornerRadius {
                        nw: 10,
                        ne: 10,
                        sw: 0,
                        se: 0,
                    },
                    theme::TITLEBAR,
                );
                ui.painter().line_segment(
                    [rect.left_bottom(), rect.right_bottom()],
                    Stroke::new(1.0_f32, Color32::from_gray(24)),
                );
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    &self.title,
                    FontId::proportional(12.0),
                    theme::TEXT,
                );
                if self.open.is_some() {
                    let center = pos2(rect.left() + 15.0, rect.center().y);
                    let response = ui.interact(
                        Rect::from_center_size(center, vec2(22.0, 22.0)),
                        response.id.with("close"),
                        Sense::click(),
                    );
                    ui.painter().circle_filled(
                        center,
                        5.0,
                        Color32::from_gray(if response.hovered() { 143 } else { 98 }),
                    );
                    if response.hovered() {
                        ui.painter().line_segment(
                            [center - vec2(2.0, 2.0), center + vec2(2.0, 2.0)],
                            Stroke::new(1.0_f32, theme::PANEL),
                        );
                        ui.painter().line_segment(
                            [center + vec2(-2.0, 2.0), center + vec2(2.0, -2.0)],
                            Stroke::new(1.0_f32, theme::PANEL),
                        );
                    }
                    close = response.on_hover_text("Close panel").clicked();
                }
                egui::Frame::new().inner_margin(24).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 12.0;
                    ui.spacing_mut().slider_width = (self.width - 48.0 - 152.0).max(90.0);
                    ui.set_width(self.width - 48.0);
                    egui::ScrollArea::vertical()
                        .max_height((ctx.content_rect().height() - 98.0).max(120.0))
                        .auto_shrink([false, true])
                        .show(ui, content)
                });
            });
        if close && let Some(open) = self.open.as_mut() {
            **open = false;
        }
    }
}

pub fn palette(ui: &mut Ui, foreground: &mut [u8; 4], background: &mut [u8; 4]) {
    let (rect, _) = ui.allocate_exact_size(vec2(36.0, 40.0), Sense::hover());
    for (offset, color, label) in [
        (
            vec2(12.0, 12.0),
            background as &mut [u8; 4],
            "Background color",
        ),
        (
            vec2(0.0, 0.0),
            foreground as &mut [u8; 4],
            "Foreground color",
        ),
    ] {
        let swatch = Rect::from_min_size(rect.min + offset, vec2(24.0, 24.0));
        let response = ui
            .interact(swatch, ui.id().with(label), Sense::click())
            .on_hover_text(label);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::ColorButton, ui.is_enabled(), label)
        });
        ui.painter().rect_filled(swatch, 6.0, Color32::BLACK);
        ui.painter()
            .rect_filled(swatch.shrink(1.0), 5.0, Color32::WHITE);
        ui.painter().rect_filled(
            swatch.shrink(2.5),
            3.5,
            Color32::from_rgba_unmultiplied(color[0], color[1], color[2], color[3]),
        );
        egui::Popup::menu(&response)
            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
            .show(|ui| {
                ui.label(label);
                let mut hsva = egui::ecolor::Hsva::from_srgba_unmultiplied(*color);
                if egui::color_picker::color_picker_hsva_2d(
                    ui,
                    &mut hsva,
                    egui::color_picker::Alpha::BlendOrAdditive,
                ) {
                    *color = hsva.to_srgba_unmultiplied();
                }
            });
    }
    let swap_rect = Rect::from_min_size(rect.min + vec2(26.0, -4.0), vec2(13.0, 13.0));
    let swap = ui
        .interact(swap_rect, ui.id().with("swap_colors"), Sense::click())
        .on_hover_text("Swap colors (X)");
    let c = swap_rect.center();
    let stroke = Stroke::new(1.0_f32, theme::MUTED);
    ui.painter().add(egui::Shape::line(
        vec![
            c + vec2(-4.0, -2.0),
            c + vec2(3.0, -2.0),
            c + vec2(1.0, -4.0),
        ],
        stroke,
    ));
    ui.painter().add(egui::Shape::line(
        vec![c + vec2(4.0, 2.0), c + vec2(-3.0, 2.0), c + vec2(-1.0, 4.0)],
        stroke,
    ));
    if swap.clicked() {
        std::mem::swap(foreground, background);
    }
    let reset_rect = Rect::from_min_size(rect.min + vec2(-1.0, 27.0), vec2(12.0, 12.0));
    let reset = ui
        .interact(reset_rect, ui.id().with("reset_colors"), Sense::click())
        .on_hover_text("Default colors (D)");
    ui.painter().rect_filled(
        reset_rect.shrink(3.0).translate(vec2(1.5, 1.5)),
        1.0,
        Color32::WHITE,
    );
    ui.painter().rect(
        reset_rect.shrink(3.0).translate(vec2(-1.5, -1.5)),
        1.0,
        Color32::BLACK,
        stroke,
        StrokeKind::Inside,
    );
    if reset.clicked() {
        *foreground = [0, 0, 0, 255];
        *background = [255; 4];
    }
}

pub fn menu_choice<T: PartialEq>(
    ui: &mut Ui,
    value: &mut T,
    option: T,
    label: impl ToString,
) -> Response {
    let label = label.to_string();
    let selected = *value == option;
    let galley =
        ui.painter()
            .layout_no_wrap(label.clone(), FontId::proportional(12.0), theme::TEXT);
    let (rect, mut response) = ui.allocate_exact_size(
        vec2(ui.available_width().max(galley.size().x + 32.0), 22.0),
        Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            &label,
        )
    });
    if response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, 4.0, theme::ACCENT);
    }
    if selected {
        let center = pos2(rect.left() + 10.0, rect.center().y);
        ui.painter().add(egui::Shape::line(
            vec![
                center + vec2(-3.0, 0.0),
                center + vec2(-1.0, 2.5),
                center + vec2(4.0, -3.0),
            ],
            Stroke::new(1.3_f32, theme::TEXT),
        ));
    }
    ui.painter().galley(
        pos2(rect.left() + 23.0, rect.center().y - galley.size().y / 2.0),
        galley,
        theme::TEXT,
    );
    if response.clicked() {
        if !selected {
            *value = option;
            response.mark_changed();
        }
        ui.close();
    }
    response
}
