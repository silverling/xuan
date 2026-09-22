use super::widgets;
use std::sync::Arc;

use egui::{Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2, pos2, vec2};
use xuan::{
    document::Point,
    operations,
    paint::{self, PaintMode},
    render,
    selection::{self, SelectionMode},
};

use super::{EditorApp, Gesture, Tool, TransformDrag, theme};

const HANDLES: [Point; 8] = [
    Point::new(0.0, 0.0),
    Point::new(0.5, 0.0),
    Point::new(1.0, 0.0),
    Point::new(1.0, 0.5),
    Point::new(1.0, 1.0),
    Point::new(0.5, 1.0),
    Point::new(0.0, 1.0),
    Point::new(0.0, 0.5),
];

fn drag_transform(
    old: xuan::document::Transform,
    start: Point,
    point: Point,
    kind: TransformDrag,
    lock_ratio: bool,
    shift: bool,
) -> xuan::document::Transform {
    let mut t = old;
    match kind {
        TransformDrag::Move | TransformDrag::Pixels => {
            t.x += point.x - start.x;
            t.y += point.y - start.y;
        }
        TransformDrag::Rotate => {
            let center = old.center();
            let a = (start.y - center.y).atan2(start.x - center.x);
            let b = (point.y - center.y).atan2(point.x - center.x);
            let angle = (b - a).to_degrees();
            t.rotation += if shift {
                (angle / 15.0).round() * 15.0
            } else {
                angle
            };
        }
        TransformDrag::Scale(index) => {
            let handle = HANDLES[index];
            let anchor = Point::new(1.0 - handle.x, 1.0 - handle.y);
            let unit = old.inverse(point);
            let anchor_point = old.point(anchor);
            let mut sx = if handle.x == 0.5 {
                1.0
            } else {
                ((unit.x - anchor.x) / (handle.x - anchor.x)).max(1.0 / old.width)
            };
            let mut sy = if handle.y == 0.5 {
                1.0
            } else {
                ((unit.y - anchor.y) / (handle.y - anchor.y)).max(1.0 / old.height)
            };
            if lock_ratio != shift {
                let factor = if handle.x == 0.5 {
                    sy
                } else if handle.y == 0.5 {
                    sx
                } else {
                    sx.max(sy)
                };
                sx = factor;
                sy = factor;
            }
            t.width = (old.width * sx).clamp(1.0, 300_000.0);
            t.height = (old.height * sy).clamp(1.0, 300_000.0);
            let moved_anchor = t.point(anchor);
            t.x += anchor_point.x - moved_anchor.x;
            t.y += anchor_point.y - moved_anchor.y;
        }
        TransformDrag::Distort(index) => {
            let mut affine = old;
            affine.warp = None;
            let mut quad = old
                .warp
                .unwrap_or([HANDLES[0], HANDLES[2], HANDLES[4], HANDLES[6]]);
            let mut moved = point;
            if shift {
                if (point.x - start.x).abs() > (point.y - start.y).abs() {
                    moved.y = start.y;
                } else {
                    moved.x = start.x;
                }
            }
            quad[index] = affine.inverse(moved);
            if xuan::geometry::Homography::from_quad(quad).is_some() {
                t.warp = Some(quad);
            }
        }
        TransformDrag::Selection => {}
    }
    t
}

impl EditorApp {
    pub(super) fn canvas(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::CANVAS))
            .show(ctx, |ui| {
                let (viewport, response) =
                    ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
                if self.sessions.is_empty() {
                    self.welcome(ui, viewport);
                    return;
                }
                let mask_target = self.transforming_mask();
                let session = &mut self.sessions[self.current];
                if session.fit {
                    session.zoom = ((viewport.width() - 100.0) / session.document.width as f32)
                        .min((viewport.height() - 90.0) / session.document.height as f32)
                        .clamp(0.01, 8.0);
                    session.pan = Vec2::ZERO;
                    session.fit = false;
                }
                session.refresh(ctx, self.gpu_state.as_ref());
                let zoom = session.zoom;
                let size = vec2(
                    session.document.width as f32,
                    session.document.height as f32,
                ) * zoom;
                let origin = viewport.center() - size * 0.5 + session.pan;
                let canvas = Rect::from_min_size(origin, size);
                self.canvas_rect = Some(canvas);
                let visible = canvas.intersect(viewport);
                let painter = ui.painter().with_clip_rect(viewport);
                painter.rect_filled(canvas.expand(3.0), 0.0, Color32::from_black_alpha(60));
                if visible.is_positive() {
                    let checker = 12.0;
                    let min_x = ((visible.left() - origin.x) / checker).floor() as i32;
                    let max_x = ((visible.right() - origin.x) / checker).ceil() as i32;
                    let min_y = ((visible.top() - origin.y) / checker).floor() as i32;
                    let max_y = ((visible.bottom() - origin.y) / checker).ceil() as i32;
                    let checker_painter = painter.with_clip_rect(visible);
                    for y in min_y..max_y {
                        for x in min_x..max_x {
                            checker_painter.rect_filled(
                                Rect::from_min_size(
                                    origin + vec2(x as f32 * checker, y as f32 * checker),
                                    Vec2::splat(checker),
                                ),
                                0.0,
                                Color32::from_gray(if (x + y) % 2 == 0 { 66 } else { 80 }),
                            );
                        }
                    }
                    if let Some(texture) = session
                        .gpu
                        .as_ref()
                        .and_then(|g| g.texture)
                        .or_else(|| session.texture.as_ref().map(|t| t.id()))
                    {
                        painter.image(
                            texture,
                            canvas,
                            Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)),
                            Color32::WHITE,
                        );
                    }
                }
                painter.rect_stroke(
                    canvas,
                    0.0,
                    Stroke::new(1.0_f32, Color32::from_gray(17)),
                    StrokeKind::Outside,
                );
                let map = |p: Point| origin + vec2(p.x, p.y) * zoom;
                if zoom >= 8.0 {
                    let start = ((visible.left() - origin.x) / zoom).floor().max(0.0) as u32;
                    let end = ((visible.right() - origin.x) / zoom)
                        .ceil()
                        .min(session.document.width as f32) as u32;
                    for x in start..=end {
                        painter.line_segment(
                            [
                                pos2(origin.x + x as f32 * zoom, visible.top()),
                                pos2(origin.x + x as f32 * zoom, visible.bottom()),
                            ],
                            Stroke::new(0.5_f32, Color32::from_white_alpha(28)),
                        );
                    }
                    let start = ((visible.top() - origin.y) / zoom).floor().max(0.0) as u32;
                    let end = ((visible.bottom() - origin.y) / zoom)
                        .ceil()
                        .min(session.document.height as f32) as u32;
                    for y in start..=end {
                        painter.line_segment(
                            [
                                pos2(visible.left(), origin.y + y as f32 * zoom),
                                pos2(visible.right(), origin.y + y as f32 * zoom),
                            ],
                            Stroke::new(0.5_f32, Color32::from_white_alpha(28)),
                        );
                    }
                }
                if let Some(mask) = &session.document.selection {
                    let step = (1.0 / zoom).ceil().max(1.0) as usize;
                    let start_x = ((visible.left() - origin.x) / zoom).floor().max(0.0) as u32;
                    let start_y = ((visible.top() - origin.y) / zoom).floor().max(0.0) as u32;
                    let end_x = ((visible.right() - origin.x) / zoom)
                        .ceil()
                        .min(mask.width() as f32) as u32;
                    let end_y = ((visible.bottom() - origin.y) / zoom)
                        .ceil()
                        .min(mask.height() as f32) as u32;
                    for y in (start_y..end_y).step_by(step) {
                        for x in (start_x..end_x).step_by(step) {
                            if mask.get_pixel(x, y)[0] < 128 {
                                continue;
                            }
                            let s = step as u32;
                            let color = if (x + y) / s % 8 < 4 {
                                Color32::WHITE
                            } else {
                                Color32::BLACK
                            };
                            if y < s || mask.get_pixel(x, y - s)[0] < 128 {
                                painter.line_segment(
                                    [
                                        map(Point::new(x as f32, y as f32)),
                                        map(Point::new((x + s) as f32, y as f32)),
                                    ],
                                    Stroke::new(1.0_f32, color),
                                );
                            }
                            if x < s || mask.get_pixel(x - s, y)[0] < 128 {
                                painter.line_segment(
                                    [
                                        map(Point::new(x as f32, y as f32)),
                                        map(Point::new(x as f32, (y + s) as f32)),
                                    ],
                                    Stroke::new(1.0_f32, color),
                                );
                            }
                            if y + s >= mask.height() || mask.get_pixel(x, y + s)[0] < 128 {
                                painter.line_segment(
                                    [
                                        map(Point::new(x as f32, (y + s) as f32)),
                                        map(Point::new((x + s) as f32, (y + s) as f32)),
                                    ],
                                    Stroke::new(1.0_f32, color),
                                );
                            }
                            if x + s >= mask.width() || mask.get_pixel(x + s, y)[0] < 128 {
                                painter.line_segment(
                                    [
                                        map(Point::new((x + s) as f32, y as f32)),
                                        map(Point::new((x + s) as f32, (y + s) as f32)),
                                    ],
                                    Stroke::new(1.0_f32, color),
                                );
                            }
                        }
                    }
                }
                let mut hover_handle = None;
                if self.tool == Tool::Move
                    && self.show_controls
                    && let Some(t) = operations::transform_box(&session.document, mask_target)
                {
                    let corners = t.corners().map(map);
                    painter.add(egui::Shape::closed_line(
                        corners.to_vec(),
                        Stroke::new(1.0_f32, Color32::from_gray(225)),
                    ));
                    for (index, unit) in HANDLES.iter().enumerate() {
                        let point = map(t.point(*unit));
                        painter.rect(
                            Rect::from_center_size(point, Vec2::splat(6.0)),
                            0.0,
                            Color32::from_gray(245),
                            Stroke::new(1.0_f32, Color32::from_gray(55)),
                            StrokeKind::Outside,
                        );
                        if response
                            .hover_pos()
                            .is_some_and(|p| p.distance(point) < 8.0)
                        {
                            hover_handle = Some(TransformDrag::Scale(index));
                        }
                    }
                    let top = map(t.point(Point::new(0.5, 0.0)));
                    let center = map(t.center());
                    let rotate = top + (top - center).normalized() * 23.0;
                    painter.line_segment([top, rotate], Stroke::new(1.0_f32, theme::TEXT));
                    painter.circle_filled(rotate, 3.5, theme::TEXT);
                    if response
                        .hover_pos()
                        .is_some_and(|p| p.distance(rotate) < 8.0)
                    {
                        hover_handle = Some(TransformDrag::Rotate);
                    }
                }
                for (horizontal, coordinate) in &self.guides {
                    let line = if *horizontal {
                        [
                            pos2(origin.x + coordinate * zoom, viewport.top()),
                            pos2(origin.x + coordinate * zoom, viewport.bottom()),
                        ]
                    } else {
                        [
                            pos2(viewport.left(), origin.y + coordinate * zoom),
                            pos2(viewport.right(), origin.y + coordinate * zoom),
                        ]
                    };
                    painter
                        .line_segment(line, Stroke::new(1.0_f32, Color32::from_rgb(219, 115, 213)));
                }
                if let Some((start, end)) = self.crop_rect {
                    let rect = Rect::from_two_pos(map(start), map(end));
                    painter.rect_stroke(
                        rect,
                        0.0,
                        Stroke::new(1.5_f32, Color32::WHITE),
                        StrokeKind::Inside,
                    );
                    for f in [1.0 / 3.0, 2.0 / 3.0] {
                        painter.line_segment(
                            [
                                pos2(rect.left() + rect.width() * f, rect.top()),
                                pos2(rect.left() + rect.width() * f, rect.bottom()),
                            ],
                            Stroke::new(0.7_f32, Color32::from_white_alpha(140)),
                        );
                        painter.line_segment(
                            [
                                pos2(rect.left(), rect.top() + rect.height() * f),
                                pos2(rect.right(), rect.top() + rect.height() * f),
                            ],
                            Stroke::new(0.7_f32, Color32::from_white_alpha(140)),
                        );
                    }
                }
                if let Some(gesture) = &self.gesture {
                    let rect = Rect::from_two_pos(map(gesture.start), map(gesture.last));
                    if matches!(self.tool, Tool::Marquee | Tool::Shape)
                        && !matches!(gesture.kind, TransformDrag::Selection)
                    {
                        if (self.tool == Tool::Marquee && self.ellipse)
                            || (self.tool == Tool::Shape
                                && self.shape_kind == xuan::paint::ShapeKind::Ellipse)
                        {
                            painter.add(egui::epaint::EllipseShape::stroke(
                                rect.center(),
                                rect.size() * 0.5,
                                Stroke::new(1.0_f32, Color32::WHITE),
                            ));
                        } else {
                            painter.rect_stroke(
                                rect,
                                0.0,
                                Stroke::new(1.0_f32, Color32::WHITE),
                                StrokeKind::Inside,
                            );
                        }
                    }
                    if matches!(self.tool, Tool::Lasso | Tool::Heal) && gesture.points.len() > 1 {
                        painter.add(egui::Shape::line(
                            gesture.points.iter().copied().map(map).collect(),
                            Stroke::new(1.0_f32, Color32::WHITE),
                        ));
                    }
                    if self.tool == Tool::Gradient {
                        painter.line_segment(
                            [map(gesture.start), map(gesture.last)],
                            Stroke::new(1.5_f32, Color32::WHITE),
                        );
                        painter.circle_filled(map(gesture.start), 3.0, Color32::WHITE);
                        painter.circle_filled(map(gesture.last), 3.0, Color32::WHITE);
                    }
                }
                if !self.polygon.is_empty() {
                    let mut points: Vec<_> = self.polygon.iter().copied().map(map).collect();
                    if let Some(p) = response.hover_pos() {
                        points.push(p);
                    }
                    painter.add(egui::Shape::line(
                        points,
                        Stroke::new(1.0_f32, Color32::WHITE),
                    ));
                }
                if let Some(source) = self.clone_source {
                    let p = map(source);
                    painter.line_segment(
                        [p - vec2(5.0, 0.0), p + vec2(5.0, 0.0)],
                        Stroke::new(1.0_f32, Color32::WHITE),
                    );
                    painter.line_segment(
                        [p - vec2(0.0, 5.0), p + vec2(0.0, 5.0)],
                        Stroke::new(1.0_f32, Color32::WHITE),
                    );
                }
                let blocked = self.job.is_some()
                    || self.develop.is_some()
                    || self.dialog.is_some()
                    || self.error.is_some()
                    || self.close_app
                    || self.close_tab.is_some()
                    || self.rename.is_some();
                if blocked {
                    return;
                }
                let pointer = response
                    .interact_pointer_pos()
                    .or_else(|| ctx.input(|i| i.pointer.hover_pos()));
                let doc_point =
                    pointer.map(|p| Point::new((p.x - origin.x) / zoom, (p.y - origin.y) / zoom));
                let modifiers = ctx.input(|i| i.modifiers);
                let panning = self.tool == Tool::Hand
                    || ctx.input(|i| i.key_down(egui::Key::Space))
                    || ctx.input(|i| i.pointer.button_down(egui::PointerButton::Middle));
                if response.hovered() {
                    let scroll = ctx.input_mut(|i| std::mem::take(&mut i.smooth_scroll_delta));
                    if scroll != Vec2::ZERO {
                        let session = &mut self.sessions[self.current];
                        let old = session.zoom;
                        let new = (old * (scroll.y * 0.003).exp()).clamp(0.01, 64.0);
                        if let Some(point) = doc_point {
                            session.pan -= (vec2(
                                point.x - session.document.width as f32 * 0.5,
                                point.y - session.document.height as f32 * 0.5,
                            )) * (new - old);
                        }
                        session.pan.x += scroll.x;
                        session.zoom = new;
                        session.fit = false;
                    }
                    let cursor = if panning {
                        egui::CursorIcon::Grab
                    } else if hover_handle.is_some() {
                        egui::CursorIcon::ResizeNwSe
                    } else if self.tool == Tool::Move {
                        egui::CursorIcon::Move
                    } else if self.tool == Tool::Text {
                        egui::CursorIcon::Text
                    } else {
                        egui::CursorIcon::Crosshair
                    };
                    ctx.set_cursor_icon(cursor);
                    if self.tool.is_brush()
                        && !panning
                        && let Some(p) = pointer
                    {
                        painter.circle_stroke(
                            p,
                            self.brush.diameter * zoom * 0.5,
                            Stroke::new(2.5_f32, Color32::from_black_alpha(130)),
                        );
                        painter.circle_stroke(
                            p,
                            self.brush.diameter * zoom * 0.5,
                            Stroke::new(1.0_f32, Color32::WHITE),
                        );
                    }
                }
                let started = response.drag_started()
                    || response.drag_started_by(egui::PointerButton::Middle);
                if started {
                    if let (Some(screen), Some(point)) = (pointer, doc_point) {
                        let press = ctx.input(|i| i.pointer.press_origin()).unwrap_or(screen);
                        let start =
                            Point::new((press.x - origin.x) / zoom, (press.y - origin.y) / zoom);
                        self.begin_gesture(start, press, panning, hover_handle, modifiers);
                        self.update_gesture(point, screen, modifiers);
                    }
                } else if self.gesture.is_some()
                    && ctx.input(|i| i.pointer.any_down())
                    && let (Some(screen), Some(point)) = (pointer, doc_point)
                {
                    self.update_gesture(point, screen, modifiers);
                }
                if self.gesture.is_some() && !ctx.input(|i| i.pointer.any_down()) {
                    self.end_gesture(modifiers);
                }
                if response.clicked()
                    && !panning
                    && hover_handle.is_none()
                    && let Some(point) = doc_point
                {
                    self.canvas_click(point, modifiers);
                }
                if response.double_clicked() && self.tool == Tool::Lasso && self.polygonal {
                    self.finish_polygon();
                }
                if response.double_clicked()
                    && self.tool == Tool::Move
                    && !panning
                    && let Some(point) = doc_point
                    && let Some(id) =
                        render::hit_test_bounds(&self.sessions[self.current].document, point)
                    && self.sessions[self.current]
                        .document
                        .layers
                        .iter()
                        .any(|l| l.id == id && l.raw.is_some())
                {
                    self.start_develop_layer(id);
                }
                if !ctx.input(|i| i.raw.hovered_files.is_empty()) {
                    painter.rect_stroke(
                        viewport.shrink(5.0),
                        8.0,
                        Stroke::new(2.0_f32, theme::ACCENT),
                        StrokeKind::Inside,
                    );
                }
            });
    }

    fn welcome(&mut self, ui: &mut egui::Ui, viewport: Rect) {
        let width = 500.0_f32.min(viewport.width() - 40.0);
        let rect = Rect::from_center_size(viewport.center(), vec2(width, 260.0));
        let mut create = false;
        let mut open = false;
        let mut import = false;
        ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
            ui.heading("New canvas");
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new("A blank space for your next composition.")
                    .size(14.0)
                    .color(theme::MUTED),
            );
            ui.add_space(24.0);
            egui::Grid::new("welcome_dimensions")
                .num_columns(3)
                .min_col_width(0.0)
                .min_row_height(0.0)
                .show(ui, |ui| {
                    ui.label("Width");
                    ui.label("");
                    ui.label("Height");
                    ui.end_row();

                    ui.add_sized(
                        vec2(180.0, 36.0),
                        widgets::Number::new(&mut self.dimensions[0])
                            .range(1..=30_000)
                            .suffix(" px"),
                    );
                    ui.label("×");
                    ui.add_sized(
                        vec2(180.0, 36.0),
                        widgets::Number::new(&mut self.dimensions[1])
                            .range(1..=30_000)
                            .suffix(" px"),
                    );
                    ui.end_row();
                });
            ui.add_space(15.0);
            ui.label(egui::RichText::new("Transparent canvas · sRGB").color(theme::MUTED));
            ui.add_space(20.0);
            ui.horizontal(|ui| {
                open = widgets::button(ui, "Open project").clicked();
                import = widgets::button(ui, "Import image").clicked();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    create = ui
                        .add(widgets::Button::new("Create canvas").primary())
                        .clicked();
                });
            });
        });
        if create {
            self.new_document();
        }
        if open {
            self.open_dialog(false);
        }
        if import {
            self.open_dialog(false);
        }
    }

    fn canvas_click(&mut self, point: Point, modifiers: egui::Modifiers) {
        match self.tool {
            Tool::Text => self.text_click(point),
            Tool::Move => {
                if self.auto_select || modifiers.ctrl {
                    self.select_canvas_layer(point, modifiers.shift, false);
                }
            }
            Tool::Wand => {
                let tolerance = self.tolerance;
                let contiguous = self.contiguous;
                let mode = self.selection_mode(modifiers);
                self.edit_selection("Magic Wand", |doc| {
                    let pixels = render::render(doc);
                    selection::combine(
                        doc,
                        selection::wand(&pixels, point, tolerance, contiguous),
                        mode,
                    );
                });
            }
            Tool::Dropper => {
                if let Some(session) = self.session() {
                    let pixel = render::pixel_at(&session.document, point);
                    self.brush.color = pixel.map(|v| (v * 255.0).round() as u8);
                }
            }
            Tool::Zoom => {
                if let Some(s) = self.session_mut() {
                    s.zoom = (s.zoom * if modifiers.alt { 0.8 } else { 1.25 }).clamp(0.01, 64.0);
                }
            }
            Tool::Lasso if self.polygonal => {
                if self.polygon.len() > 2
                    && self.polygon[0].distance(point) * self.session().unwrap().zoom < 8.0
                {
                    self.finish_polygon();
                } else {
                    self.polygon.push(point);
                }
            }
            Tool::Clone if modifiers.alt => {
                self.clone_source = Some(point);
                self.clone_offset = None;
            }
            tool if tool.is_brush() => {
                let from = if modifiers.shift {
                    self.last_brush.unwrap_or(point)
                } else {
                    point
                };
                self.begin_gesture(from, Pos2::ZERO, false, None, modifiers);
                self.update_gesture(point, Pos2::ZERO, modifiers);
                self.end_gesture(modifiers);
            }
            _ => {}
        }
    }

    fn select_canvas_layer(&mut self, point: Point, extend: bool, dragging: bool) -> bool {
        let ignore_transparent_pixels = self.ignore_transparent_pixels;
        let Some(session) = self.session_mut() else {
            return false;
        };
        let document = &mut session.document;
        let inside = point.x >= 0.0
            && point.y >= 0.0
            && point.x < document.width as f32
            && point.y < document.height as f32;
        let hit = inside
            .then(|| {
                if ignore_transparent_pixels {
                    render::hit_test(document, point)
                } else {
                    render::hit_test_bounds(document, point)
                }
            })
            .flatten();
        if let Some(id) = hit {
            // Moving an already selected layer keeps the other selected layers and mask target.
            if !dragging || !document.transform_targets().contains(&id) {
                document.select(id, extend);
                self.mask_target = false;
            }
        } else if !extend {
            document.selected.clear();
            document.active = None;
            self.mask_target = false;
        }
        hit.is_some()
    }

    fn selection_mode(&self, modifiers: egui::Modifiers) -> SelectionMode {
        if modifiers.shift {
            SelectionMode::Add
        } else if modifiers.alt {
            SelectionMode::Subtract
        } else {
            self.selection_mode
        }
    }

    pub(super) fn finish_polygon(&mut self) {
        if self.polygon.len() < 3 {
            return;
        }
        let points = std::mem::take(&mut self.polygon);
        let mode = self.selection_mode;
        self.edit_selection("Polygonal Lasso", |doc| {
            selection::combine(
                doc,
                selection::polygon(doc.width, doc.height, &points),
                mode,
            );
        });
    }

    fn begin_gesture(
        &mut self,
        point: Point,
        screen: Pos2,
        panning: bool,
        mut handle: Option<TransformDrag>,
        modifiers: egui::Modifiers,
    ) {
        if self.gesture.is_some() || self.sessions.is_empty() {
            return;
        }
        if panning {
            // View navigation must not start an edit or run tool-specific setup.
            let session = &self.sessions[self.current];
            self.gesture = Some(Gesture {
                start: point,
                last: point,
                screen_start: screen,
                pan_start: session.pan,
                points: Vec::new(),
                original: session.document.clone(),
                kind: TransformDrag::Move,
                panning: true,
                clone_offset: Point::default(),
                source: None,
                reference: None,
            });
            return;
        }
        if self.tool == Tool::Text {
            return;
        }
        if self.tool == Tool::Clone && modifiers.alt {
            self.clone_source = Some(point);
            self.clone_offset = None;
            return;
        }
        if self.tool == Tool::Clone && self.clone_source.is_none() {
            self.status = "Alt-click on the canvas to set a clone source".into();
            return;
        }
        if self.tool == Tool::Move && self.show_controls {
            // The press location determines the handle, even if the pointer has moved since.
            handle = None;
            let session = &self.sessions[self.current];
            if let Some(t) = operations::transform_box(&session.document, self.transforming_mask())
            {
                if let Some(index) = HANDLES
                    .iter()
                    .position(|unit| t.point(*unit).distance(point) * session.zoom < 9.0)
                {
                    handle = Some(if modifiers.ctrl && index % 2 == 0 {
                        TransformDrag::Distort(index / 2)
                    } else {
                        TransformDrag::Scale(index)
                    });
                } else {
                    let top = t.point(Point::new(0.5, 0.0));
                    let center = t.center();
                    let distance = top.distance(center).max(0.01);
                    let rotate = Point::new(
                        top.x + (top.x - center.x) / distance * 23.0 / session.zoom,
                        top.y + (top.y - center.y) / distance * 23.0 / session.zoom,
                    );
                    if rotate.distance(point) * session.zoom < 9.0 {
                        handle = Some(TransformDrag::Rotate);
                    }
                }
            }
        }
        let mut kind = handle.unwrap_or(TransformDrag::Move);
        if self.tool == Tool::Move
            && (self.auto_select || modifiers.ctrl)
            && handle.is_none()
            && !self.select_canvas_layer(point, modifiers.shift, true)
        {
            return;
        }
        let mask_target = self.transforming_mask();
        let session = &mut self.sessions[self.current];
        if self.tool == Tool::Move && session.document.active.is_none() {
            return;
        }
        session.history.begin(self.tool.label(), &session.document);
        if self.tool == Tool::Move && modifiers.alt {
            operations::duplicate(&mut session.document);
        }
        if self.tool.is_selection()
            && !modifiers.shift
            && (!modifiers.alt || modifiers.ctrl)
            && session
                .document
                .selection
                .as_ref()
                .is_some_and(|m| selection::coverage(Some(m), point) > 0.0)
        {
            if modifiers.ctrl {
                if let Some((pixels, origin)) = operations::copy_pixels(&session.document, false) {
                    if !modifiers.alt
                        && let Err(error) = paint::fill(&mut session.document, [0; 4], true, false)
                    {
                        session.history.cancel(&mut session.document);
                        self.error = Some(error.to_string());
                        return;
                    }
                    let mut layer = xuan::document::Layer::image("Selection", pixels);
                    layer.transform.x = origin.x;
                    layer.transform.y = origin.y;
                    session.document.insert(layer);
                    session.document.selection = None;
                    kind = TransformDrag::Pixels;
                }
            } else {
                kind = TransformDrag::Selection;
            }
        }
        let source = if matches!(self.tool, Tool::Clone | Tool::Blur) {
            if self.tool == Tool::Clone && self.clone_all {
                Some(Arc::new(render::render(&session.document)))
            } else {
                let mut isolated = session.document.clone();
                let id = isolated.active;
                for l in &mut isolated.layers {
                    l.visible = Some(l.id) == id || l.group;
                }
                Some(Arc::new(render::render(&isolated)))
            }
        } else {
            None
        };
        let offset = if self.clone_aligned {
            self.clone_offset
        } else {
            None
        }
        .unwrap_or_else(|| {
            self.clone_source.map_or(Point::default(), |p| {
                Point::new(p.x - point.x, p.y - point.y)
            })
        });
        if self.tool == Tool::Clone {
            self.clone_offset = Some(offset);
        }
        self.gesture = Some(Gesture {
            start: point,
            last: point,
            screen_start: screen,
            pan_start: session.pan,
            points: vec![point],
            original: session.document.clone(),
            kind,
            panning: false,
            clone_offset: offset,
            source,
            reference: operations::transform_box(&session.document, mask_target),
        });
    }

    fn update_gesture(&mut self, mut point: Point, screen: Pos2, modifiers: egui::Modifiers) {
        let Some(mut gesture) = self.gesture.take() else {
            return;
        };
        if gesture.panning {
            self.sessions[self.current].pan = gesture.pan_start + (screen - gesture.screen_start);
            self.gesture = Some(gesture);
            return;
        }
        if modifiers.shift && matches!(self.tool, Tool::Shape | Tool::Marquee | Tool::Crop) {
            let dx = point.x - gesture.start.x;
            let dy = point.y - gesture.start.y;
            let size = dx.abs().max(dy.abs());
            point = Point::new(
                gesture.start.x + size * dx.signum(),
                gesture.start.y + size * dy.signum(),
            );
        }
        let mask_target = self.editing_mask();
        let transform_mask = self.transforming_mask();
        let session = &mut self.sessions[self.current];
        let result = if matches!(gesture.kind, TransformDrag::Selection) && self.tool.is_selection()
        {
            if let Some(mask) = &gesture.original.selection {
                session.document.selection = Some(Arc::new(selection::translate(
                    mask,
                    (point.x - gesture.start.x).round() as i32,
                    (point.y - gesture.start.y).round() as i32,
                )));
            }
            Ok(())
        } else {
            match self.tool {
                Tool::Heal => {
                    gesture.points.push(point);
                    Ok(())
                }
                tool if tool.is_brush() => {
                    let mode = match self.tool {
                        Tool::Erase => PaintMode::Erase,
                        Tool::Clone => PaintMode::Clone,
                        Tool::Heal => PaintMode::Heal,
                        Tool::Blur => self.blur_mode,
                        _ => PaintMode::Paint,
                    };
                    let offset = if mode == PaintMode::Smudge {
                        Point::new(gesture.last.x - point.x, gesture.last.y - point.y)
                    } else {
                        gesture.clone_offset
                    };
                    paint::stroke(
                        &mut session.document,
                        gesture.last,
                        point,
                        &self.brush,
                        paint::StrokeOptions {
                            mode,
                            mask_target,
                            source: gesture.source.as_deref(),
                            clone_offset: offset,
                        },
                    )
                }
                _ if self.tool == Tool::Move || matches!(gesture.kind, TransformDrag::Pixels) => {
                    let mut dx = point.x - gesture.start.x;
                    let mut dy = point.y - gesture.start.y;
                    if modifiers.shift && matches!(gesture.kind, TransformDrag::Move) {
                        if dx.abs() > dy.abs() {
                            dy = 0.0;
                        } else {
                            dx = 0.0;
                        }
                    }
                    self.guides.clear();
                    if self.snap
                        && !modifiers.ctrl
                        && matches!(gesture.kind, TransformDrag::Move)
                        && let Some(t) = gesture.reference
                    {
                        let mut xs = vec![
                            0.0,
                            session.document.width as f32 * 0.5,
                            session.document.width as f32,
                        ];
                        let mut ys = vec![
                            0.0,
                            session.document.height as f32 * 0.5,
                            session.document.height as f32,
                        ];
                        for l in &gesture.original.layers {
                            if !gesture.original.selected.contains(&l.id) && l.visible {
                                xs.extend([
                                    l.transform.x,
                                    l.transform.center().x,
                                    l.transform.x + l.transform.width,
                                ]);
                                ys.extend([
                                    l.transform.y,
                                    l.transform.center().y,
                                    l.transform.y + l.transform.height,
                                ]);
                            }
                        }
                        let snap = |guides: [f32; 3], targets: &[f32]| -> Option<(f32, f32)> {
                            let mut best = None;
                            let mut distance = 6.0 / session.zoom;
                            for g in guides {
                                for target in targets {
                                    let d = *target - g;
                                    if d.abs() < distance {
                                        distance = d.abs();
                                        best = Some((d, *target));
                                    }
                                }
                            }
                            best
                        };
                        if let Some((delta, x)) =
                            snap([t.x + dx, t.center().x + dx, t.x + t.width + dx], &xs)
                        {
                            dx += delta;
                            self.guides.push((true, x));
                        }
                        if let Some((delta, y)) =
                            snap([t.y + dy, t.center().y + dy, t.y + t.height + dy], &ys)
                        {
                            dy += delta;
                            self.guides.push((false, y));
                        }
                    }
                    let targets = if transform_mask {
                        gesture.original.active.into_iter().collect()
                    } else {
                        gesture.original.movement_targets()
                    };
                    if let Some(reference) = gesture.reference {
                        let moved = Point::new(gesture.start.x + dx, gesture.start.y + dy);
                        let transformed = drag_transform(
                            reference,
                            gesture.start,
                            moved,
                            gesture.kind,
                            self.lock_ratio,
                            modifiers.shift,
                        );
                        if transformed.valid() {
                            for layer in &mut session.document.layers {
                                if !targets.contains(&layer.id) || layer.locked {
                                    continue;
                                }
                                let Some(original) =
                                    gesture.original.layers.iter().find(|l| l.id == layer.id)
                                else {
                                    continue;
                                };
                                let old = if transform_mask {
                                    original
                                        .mask
                                        .as_ref()
                                        .and_then(|m| m.placement)
                                        .unwrap_or(original.transform)
                                } else {
                                    original.transform
                                };
                                let transform = if targets.len() == 1 {
                                    transformed
                                } else {
                                    old.following(reference, transformed)
                                };
                                if transform_mask {
                                    if let Some(mask) = &mut layer.mask {
                                        mask.placement = Some(transform);
                                        mask.linked = false;
                                    }
                                } else {
                                    layer.transform = original.transform;
                                    layer.mask = original.mask.clone();
                                    layer.set_transform(transform);
                                }
                            }
                        }
                    }
                    Ok(())
                }
                Tool::Lasso => {
                    if !self.polygonal {
                        gesture.points.push(point);
                    }
                    Ok(())
                }
                Tool::Crop => {
                    self.crop_rect = Some((gesture.start, point));
                    Ok(())
                }
                _ => Ok(()),
            }
        };
        if let Err(error) = result {
            session.history.cancel(&mut session.document);
            self.error = Some(error.to_string());
            session.invalidate();
            return;
        }
        gesture.last = point;
        if gesture.changes_composition(self.tool)
            && !matches!(self.tool, Tool::Gradient | Tool::Shape)
        {
            session.invalidate();
        }
        self.gesture = Some(gesture);
    }

    fn end_gesture(&mut self, modifiers: egui::Modifiers) {
        let Some(gesture) = self.gesture.take() else {
            return;
        };
        if gesture.panning {
            return;
        }
        if self.tool == Tool::Heal {
            let points = gesture.points;
            let brush = self.brush.clone();
            self.start_job("Spot Healing", move |document, cancel| {
                xuan::retouch::heal_path(document, &points, &brush, cancel)
            });
            return;
        }
        let mode = self.selection_mode(modifiers);
        let mask_target = self.editing_mask();
        let session = &mut self.sessions[self.current];
        let start = gesture.start;
        let end = gesture.last;
        let changes_composition = gesture.changes_composition(self.tool);
        let result = if matches!(
            gesture.kind,
            TransformDrag::Selection | TransformDrag::Pixels
        ) && self.tool.is_selection()
        {
            Ok(())
        } else {
            match self.tool {
                Tool::Marquee => {
                    selection::combine(
                        &mut session.document,
                        selection::rectangle(
                            gesture.original.width,
                            gesture.original.height,
                            start,
                            end,
                            self.ellipse,
                        ),
                        mode,
                    );
                    Ok(())
                }
                Tool::Lasso if !self.polygonal => {
                    selection::combine(
                        &mut session.document,
                        selection::polygon(
                            gesture.original.width,
                            gesture.original.height,
                            &gesture.points,
                        ),
                        mode,
                    );
                    Ok(())
                }
                Tool::Gradient => paint::gradient(
                    &mut session.document,
                    start,
                    end,
                    paint::GradientOptions {
                        foreground: self.brush.color,
                        background: self.background,
                        radial: self.radial,
                        opacity: self.brush.opacity,
                        mask_target,
                    },
                ),
                Tool::Shape => {
                    let start = if modifiers.alt {
                        Point::new(start.x - (end.x - start.x), start.y - (end.y - start.y))
                    } else {
                        start
                    };
                    paint::shape(
                        start,
                        end,
                        self.shape_kind,
                        self.brush.color,
                        self.corner_radius,
                    )
                    .map(|layer| session.document.insert(layer))
                }
                Tool::Crop => {
                    session.history.cancel(&mut session.document);
                    return;
                }
                _ => Ok(()),
            }
        };
        if !changes_composition && result.is_ok() {
            session.history.commit();
            return;
        }
        match result {
            Ok(()) => match paint::refresh_shapes(&mut session.document) {
                Ok(()) => session.history.commit(),
                Err(error) => {
                    session.history.cancel(&mut session.document);
                    self.error = Some(error.to_string());
                }
            },
            Err(error) => {
                session.history.cancel(&mut session.document);
                self.error = Some(error.to_string());
            }
        }
        session.invalidate();
        self.guides.clear();
        if self.tool.is_brush() {
            self.last_brush = Some(end);
        }
    }
}
