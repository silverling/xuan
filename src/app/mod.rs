mod canvas;
mod chrome;
mod clipboard;
mod develop;
mod develop_controls;
mod dialogs;
mod filter_preview;
mod font_picker;
mod gpu_preview;
mod icons;
mod jobs;
mod layers;
mod levels_controls;
mod menus;
mod panels;
mod shortcuts;
#[cfg(test)]
mod tests;
mod text_controls;
mod theme;
mod widgets;

use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Result;
use egui::{Pos2, TextureHandle, Vec2};
use image::{GrayImage, RgbaImage};
use uuid::Uuid;
use xuan::{
    document::{Adjustment, Document, Layer, Mask, Point, Transform},
    effects::Filter,
    history::History,
    io, operations,
    paint::{self, Brush, PaintMode, ShapeKind},
    render,
    selection::SelectionMode,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tool {
    #[default]
    Move,
    Marquee,
    Lasso,
    Wand,
    Crop,
    Brush,
    Erase,
    Heal,
    Clone,
    Blur,
    Gradient,
    Shape,
    Text,
    Dropper,
    Hand,
    Zoom,
}

impl Tool {
    const ALL: [Self; 16] = [
        Self::Move,
        Self::Marquee,
        Self::Lasso,
        Self::Wand,
        Self::Crop,
        Self::Brush,
        Self::Erase,
        Self::Heal,
        Self::Clone,
        Self::Blur,
        Self::Gradient,
        Self::Shape,
        Self::Text,
        Self::Dropper,
        Self::Hand,
        Self::Zoom,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Move => "Move / Transform",
            Self::Marquee => "Marquee",
            Self::Lasso => "Lasso",
            Self::Wand => "Magic Wand",
            Self::Crop => "Crop",
            Self::Brush => "Brush",
            Self::Erase => "Eraser",
            Self::Heal => "Spot Healing",
            Self::Clone => "Clone Stamp",
            Self::Blur => "Blur / Smudge",
            Self::Gradient => "Gradient",
            Self::Shape => "Shape",
            Self::Text => "Text",
            Self::Dropper => "Eyedropper",
            Self::Hand => "Hand",
            Self::Zoom => "Zoom",
        }
    }
    fn shortcut(self) -> &'static str {
        match self {
            Self::Move => "V",
            Self::Marquee => "M",
            Self::Lasso => "L",
            Self::Wand => "W",
            Self::Crop => "C",
            Self::Brush => "B",
            Self::Erase => "E",
            Self::Heal => "J",
            Self::Clone => "S",
            Self::Blur => "R",
            Self::Gradient => "G",
            Self::Shape => "U",
            Self::Text => "T",
            Self::Dropper => "I",
            Self::Hand => "H",
            Self::Zoom => "Z",
        }
    }
    fn is_brush(self) -> bool {
        matches!(
            self,
            Self::Brush | Self::Erase | Self::Heal | Self::Clone | Self::Blur
        )
    }
    fn is_selection(self) -> bool {
        matches!(self, Self::Marquee | Self::Lasso | Self::Wand)
    }
    fn hint(self) -> &'static str {
        match self {
            Self::Move => {
                "Click to select · Click outside to deselect · Drag to move · Handles to resize · Space to pan"
            }
            Self::Marquee => {
                "Drag to select · Shift add · Alt subtract · Ctrl+D deselect · Delete clears"
            }
            Self::Lasso => {
                "Draw a selection · Shift add · Alt subtract · Enter closes polygon · Escape cancels"
            }
            Self::Wand => {
                "Click to select similar colors · Shift add · Alt subtract · Ctrl+D deselect"
            }
            Self::Crop => "Drag to crop · Enter applies · Escape cancels · Space to pan",
            Self::Brush | Self::Erase => {
                "Drag to paint · [ ] size · Shift-click straight line · 1–0 opacity · Space to pan"
            }
            Self::Heal => "Paint over blemishes · [ ] size · Space to pan",
            Self::Clone => "Alt-click to set source · Drag to clone · [ ] size · Space to pan",
            Self::Blur => "Drag to retouch · [ ] size · 1–0 strength · Space to pan",
            Self::Gradient => "Drag to draw gradient · Shift locks angle · Escape cancels",
            Self::Shape => {
                "Drag to draw a new shape · Shift constrains proportions · Alt draws from center"
            }
            Self::Text => "Click to add text · Click text to edit · Use Move to transform",
            Self::Dropper => "Click to sample the composition · X swaps foreground and background",
            Self::Hand => "Drag to pan · Scroll to zoom · Ctrl+0 fits canvas",
            Self::Zoom => "Click to zoom in · Alt-click to zoom out · Ctrl+1 actual pixels",
        }
    }
}

struct Session {
    document: Document,
    history: History,
    path: Option<PathBuf>,
    title: String,
    zoom: f32,
    pan: Vec2,
    fit: bool,
    dirty_preview: bool,
    texture: Option<TextureHandle>,
    gpu: Option<gpu_preview::GpuPreview>,
    motion_blur_preview: Option<[f32; 2]>,
    preview_size: [u32; 2],
    composite: Option<Arc<RgbaImage>>,
    thumbnails: HashMap<(Uuid, bool), layers::LayerThumbnail>,
    collapsed: HashSet<Uuid>,
}

impl Session {
    fn new(mut document: Document, title: String, path: Option<PathBuf>) -> Self {
        document.id = Uuid::new_v4();
        document.promote_image_masks();
        Self {
            document,
            history: History::default(),
            path,
            title,
            zoom: 1.0,
            pan: Vec2::ZERO,
            fit: true,
            dirty_preview: true,
            texture: None,
            gpu: None,
            motion_blur_preview: None,
            preview_size: [0, 0],
            composite: None,
            thumbnails: HashMap::new(),
            collapsed: HashSet::new(),
        }
    }

    fn invalidate(&mut self) {
        self.dirty_preview = true;
    }

    fn refresh(&mut self, ctx: &egui::Context, state: Option<&eframe::egui_wgpu::RenderState>) {
        let max_side = state.map_or(1600, |s| {
            s.device.limits().max_texture_dimension_2d.min(4096)
        });
        // Zoom only changes how the cached composition is drawn. Rebuilding it here
        // can also resize source images on the UI thread.
        let factor =
            (max_side as f32 / self.document.width.max(self.document.height) as f32).min(1.0);
        let size = [
            (self.document.width as f32 * factor).round().max(1.0) as u32,
            (self.document.height as f32 * factor).round().max(1.0) as u32,
        ];
        if !self.dirty_preview && size == self.preview_size {
            return;
        }
        self.thumbnails.retain(|(id, mask), _| {
            self.document
                .layers
                .iter()
                .any(|layer| layer.id == *id && (!mask || layer.mask.is_some()))
        });
        self.preview_size = size;
        if let Some(state) = state.filter(|s| {
            s.adapter.get_info().device_type != wgpu::DeviceType::Cpu
                && s.device.limits().max_compute_workgroups_per_dimension > 0
                && self.document.layers.iter().all(|l| {
                    l.pixels.as_ref().is_none_or(|p| {
                        p.width().max(p.height()) <= s.device.limits().max_texture_dimension_2d
                    })
                })
        }) {
            let preview = self
                .gpu
                .get_or_insert_with(|| gpu_preview::GpuPreview::new(state));
            if let Some(settings) = self.motion_blur_preview {
                preview.render_with_motion_blur(&self.document, size, Some(settings));
            } else {
                preview.render(&self.document, size);
            }
            self.texture = None;
            self.composite = None;
            self.dirty_preview = false;
            return;
        }
        self.gpu = None;
        let image = render::render_scaled(&self.document, size[0], size[1]);
        let color = egui::ColorImage::from_rgba_unmultiplied(
            [image.width() as usize, image.height() as usize],
            image.as_raw(),
        );
        if let Some(texture) = &mut self.texture {
            texture.set(color, egui::TextureOptions::LINEAR);
        } else {
            self.texture = Some(ctx.load_texture(
                format!("canvas-{}", self.document.id),
                color,
                egui::TextureOptions::LINEAR,
            ));
        }
        self.composite = Some(Arc::new(image));
        self.dirty_preview = false;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Dialog {
    New,
    CanvasSize,
    ImageSize,
    Effect,
    Text,
    Export,
    Shortcuts,
    About,
}

struct EffectEdit {
    original: Document,
    // The histogram uses the immutable original, independent of live preview edits.
    levels_source: Option<RgbaImage>,
    adjustment: Option<Adjustment>,
    filter: Option<Filter>,
    filter_preview: filter_preview::FilterPreview,
    as_layer: bool,
    preview: bool,
    refresh: bool,
    channel: usize,
    target: Option<Uuid>,
}

#[derive(Clone, Copy)]
enum TransformDrag {
    Move,
    Scale(usize),
    Rotate,
    Selection,
    Pixels,
    Distort(usize),
}

#[derive(Clone, Copy)]
struct LayerDrag {
    project: Uuid,
    layer: Uuid,
}

struct Gesture {
    start: Point,
    last: Point,
    screen_start: Pos2,
    pan_start: Vec2,
    points: Vec<Point>,
    original: Document,
    kind: TransformDrag,
    panning: bool,
    clone_offset: Point,
    source: Option<Arc<RgbaImage>>,
    reference: Option<Transform>,
}

impl Gesture {
    fn changes_composition(&self, tool: Tool) -> bool {
        !self.panning
            && (matches!(self.kind, TransformDrag::Pixels)
                || matches!(
                    tool,
                    Tool::Move
                        | Tool::Brush
                        | Tool::Erase
                        | Tool::Clone
                        | Tool::Blur
                        | Tool::Gradient
                        | Tool::Shape
                ))
    }
}

pub struct EditorApp {
    context: egui::Context,
    window_title: String,
    job: Option<jobs::Job>,
    develop: Option<develop::Develop>,
    raw_queue: std::collections::VecDeque<(PathBuf, develop::DevelopTarget)>,
    develop_close_requested: bool,
    gpu_state: Option<eframe::egui_wgpu::RenderState>,
    processor: Option<Arc<xuan::gpu::Processor>>,
    sessions: Vec<Session>,
    current: usize,
    tool: Tool,
    brush: Brush,
    background: [u8; 4],
    mask_target: bool,
    ellipse: bool,
    polygonal: bool,
    polygon: Vec<Point>,
    selection_mode: SelectionMode,
    tolerance: u8,
    contiguous: bool,
    radial: bool,
    shape_kind: ShapeKind,
    corner_radius: f32,
    text_style: xuan::text::TextStyle,
    text_renderer: Option<xuan::text::TextRenderer>,
    text_edit: Option<text_controls::TextEdit>,
    blur_mode: PaintMode,
    auto_select: bool,
    ignore_transparent_pixels: bool,
    show_controls: bool,
    snap: bool,
    lock_ratio: bool,
    clone_source: Option<Point>,
    clone_offset: Option<Point>,
    clone_aligned: bool,
    clone_all: bool,
    last_brush: Option<Point>,
    gesture: Option<Gesture>,
    crop_rect: Option<(Point, Point)>,
    guides: Vec<(bool, f32)>,
    dialog: Option<Dialog>,
    dimensions: [u32; 2],
    resolution: f32,
    anchor: [f32; 2],
    effect: Option<EffectEdit>,
    error: Option<String>,
    status: String,
    rename: Option<layers::LayerRename>,
    close_tab: Option<usize>,
    close_app: bool,
    allow_close: bool,
    clipboard: Option<(RgbaImage, Point)>,
    system_clipboard: Option<arboard::Clipboard>,
    jpeg_quality: u8,
    export_format: String,
    export_texture: Option<TextureHandle>,
    export_bytes: usize,
    export_changed: bool,
    screenshot: Option<PathBuf>,
    screenshot_requested: bool,
    frames: usize,
    canvas_rect: Option<egui::Rect>,
}

impl EditorApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        paths: Vec<PathBuf>,
        demo: bool,
        screenshot: Option<PathBuf>,
    ) -> Self {
        let processor = cc
            .wgpu_render_state
            .as_ref()
            .filter(|state| {
                state.adapter.get_info().device_type != wgpu::DeviceType::Cpu
                    && state.device.limits().max_storage_buffers_per_shader_stage >= 4
            })
            .map(|state| xuan::gpu::Processor::new(state.device.clone(), state.queue.clone()));
        let mut app = xuan::gpu::scope(processor.clone(), || {
            Self::with_context(&cc.egui_ctx, paths, demo, screenshot)
        });
        app.processor = processor;
        app.gpu_state = cc.wgpu_render_state.clone();
        app
    }

    pub fn preview_panel(&mut self, name: &str) {
        if self.screenshot.is_none() {
            return;
        }
        match name {
            "brush" => self.set_tool(Tool::Brush),
            "selection" => self.set_tool(Tool::Marquee),
            "gradient" => self.set_tool(Tool::Gradient),
            "shape" => self.set_tool(Tool::Shape),
            "text" => {
                self.set_tool(Tool::Text);
                self.start_text(None, Point::new(100.0, 120.0));
            }
            "export" => {
                self.export_format = "jpg".into();
                self.command(name);
            }
            "levels" | "hue" | "curves" | "new" => self.command(name),
            _ => {}
        }
    }

    fn with_context(
        ctx: &egui::Context,
        paths: Vec<PathBuf>,
        demo: bool,
        screenshot: Option<PathBuf>,
    ) -> Self {
        theme::apply(ctx);
        let mut app = Self {
            context: ctx.clone(),
            window_title: String::new(),
            job: None,
            develop: None,
            raw_queue: Default::default(),
            develop_close_requested: false,
            gpu_state: None,
            processor: None,
            sessions: Vec::new(),
            current: 0,
            tool: Tool::Move,
            brush: Brush::default(),
            background: [255; 4],
            mask_target: false,
            ellipse: false,
            polygonal: false,
            polygon: Vec::new(),
            selection_mode: SelectionMode::Replace,
            tolerance: 32,
            contiguous: true,
            radial: false,
            shape_kind: ShapeKind::Rectangle,
            corner_radius: 16.0,
            text_style: xuan::text::TextStyle::default(),
            text_renderer: None,
            text_edit: None,
            blur_mode: PaintMode::Blur,
            auto_select: true,
            ignore_transparent_pixels: true,
            show_controls: true,
            snap: true,
            lock_ratio: true,
            clone_source: None,
            clone_offset: None,
            clone_aligned: true,
            clone_all: true,
            last_brush: None,
            gesture: None,
            crop_rect: None,
            guides: Vec::new(),
            dialog: None,
            dimensions: [1920, 1080],
            resolution: 72.0,
            anchor: [0.5, 0.5],
            effect: None,
            error: None,
            status: String::new(),
            rename: None,
            close_tab: None,
            close_app: false,
            allow_close: false,
            clipboard: None,
            system_clipboard: None,
            jpeg_quality: 90,
            export_format: "png".into(),
            export_texture: None,
            export_bytes: 0,
            export_changed: true,
            screenshot,
            screenshot_requested: false,
            frames: 0,
            canvas_rect: None,
        };
        if demo {
            app.add_demo();
        }
        for path in paths {
            app.open_path(&path, false);
        }
        app
    }

    fn session(&self) -> Option<&Session> {
        self.sessions.get(self.current)
    }
    fn session_mut(&mut self) -> Option<&mut Session> {
        self.sessions.get_mut(self.current)
    }

    fn editing_mask(&self) -> bool {
        self.session()
            .and_then(|s| s.document.active())
            .is_some_and(|layer| {
                layer.standalone_mask || (self.mask_target && layer.mask.is_some())
            })
    }

    fn transforming_mask(&self) -> bool {
        self.editing_mask()
            && self
                .session()
                .and_then(|s| s.document.active())
                .is_some_and(|l| !l.standalone_mask)
    }

    fn edit(&mut self, name: &str, operation: impl FnOnce(&mut Document) -> Result<()>) {
        let Some(session) = self.session_mut() else {
            return;
        };
        session.history.begin(name, &session.document);
        match operation(&mut session.document)
            .and_then(|()| paint::refresh_shapes(&mut session.document))
        {
            Ok(()) => {
                session.document.promote_image_masks();
                session.history.commit();
                session.invalidate();
                self.status = name.into();
            }
            Err(error) => {
                session.history.cancel(&mut session.document);
                session.invalidate();
                self.error = Some(error.to_string());
            }
        }
    }

    fn edit_selection(&mut self, name: &str, operation: impl FnOnce(&mut Document)) {
        let Some(session) = self.session_mut() else {
            return;
        };
        session.history.begin(name, &session.document);
        operation(&mut session.document);
        session.history.commit();
        self.status = name.into();
    }

    fn edit_continuous(&mut self, name: &str, operation: impl FnOnce(&mut Document) -> Result<()>) {
        let Some(session) = self.session_mut() else {
            return;
        };
        session.history.begin(name, &session.document);
        if let Err(error) = operation(&mut session.document)
            .and_then(|()| paint::refresh_shapes(&mut session.document))
        {
            session.history.cancel(&mut session.document);
            self.error = Some(error.to_string());
        }
        if let Some(session) = self.session_mut() {
            session.invalidate();
        }
    }

    fn new_document(&mut self) {
        match Document::new(self.dimensions[0], self.dimensions[1]) {
            Ok(mut document) => {
                document.resolution = self.resolution;
                self.sessions
                    .push(Session::new(document, "Untitled".into(), None));
                self.current = self.sessions.len() - 1;
                self.dialog = None;
                self.mask_target = false;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    fn open_path(&mut self, path: &Path, as_layer: bool) {
        if xuan::raw::is_raw(path) {
            self.queue_raw(path, as_layer);
            return;
        }
        let project = path.is_dir() || path.extension().is_some_and(|e| e == "xuan");
        let result = if project {
            io::load(path)
        } else {
            io::import_image(path)
                .and_then(|image| {
                    if as_layer && !self.sessions.is_empty() {
                        let name = path
                            .file_stem()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_string();
                        self.edit("Import Image", |doc| {
                            let mut layer = Layer::image(name, image);
                            layer.transform.x = (doc.width as f32 - layer.transform.width) * 0.5;
                            layer.transform.y = (doc.height as f32 - layer.transform.height) * 0.5;
                            doc.insert(layer);
                            Ok(())
                        });
                        return Ok(None);
                    }
                    let mut document = Document::new(image.width(), image.height())?;
                    let layer = Layer::image(
                        path.file_stem().unwrap_or_default().to_string_lossy(),
                        image,
                    );
                    document.select(layer.id, false);
                    document.layers = vec![layer];
                    Ok(Some(document))
                })
                .map(|doc| doc.unwrap_or_else(|| self.session().unwrap().document.clone()))
        };
        match result {
            Ok(document) => {
                if !project && as_layer && !self.sessions.is_empty() {
                    return;
                }
                let path = if path.extension().is_some_and(|e| e == "xuan") {
                    Some(path.to_path_buf())
                } else {
                    None
                };
                let title = path
                    .as_ref()
                    .and_then(|p| p.file_stem())
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| {
                        document
                            .layers
                            .first()
                            .map_or("Untitled".into(), |l| l.name.clone())
                    });
                self.sessions.push(Session::new(document, title, path));
                self.current = self.sessions.len() - 1;
                self.mask_target = false;
                self.dialog = None;
            }
            Err(error) => {
                self.error = Some(format!("Could not open {}\n\n{error:#}", path.display()))
            }
        }
    }

    fn copy_layer_to_project(&mut self, drag: LayerDrag, destination: usize) {
        if self.dialog.is_some() || self.job.is_some() {
            return;
        }
        let Some(source) = self
            .sessions
            .iter()
            .position(|s| s.document.id == drag.project)
        else {
            return;
        };
        let id = drag.layer;
        if source == destination {
            return;
        }
        let document = self.sessions[source].document.clone();
        self.cancel_gesture();
        self.current = destination;
        self.edit("Copy Layers from Project", |target| {
            operations::copy_layers(&document, target, id)
        });
        self.mask_target = false;
    }

    fn open_dialog(&mut self, as_layer: bool) {
        if let Some(paths) = rfd::FileDialog::new()
            .add_filter(
                "Images and Xuan projects",
                &[
                    "xuan", "png", "jpg", "jpeg", "tif", "tiff", "webp", "bmp", "gif", "heic",
                    "heif", "nef", "nrw",
                ],
            )
            .pick_files()
        {
            for path in paths {
                self.open_path(&path, as_layer);
            }
        }
    }

    fn save_current(&mut self, save_as: bool) -> bool {
        let Some(session) = self.session() else {
            return false;
        };
        let path = if save_as || session.path.is_none() {
            rfd::FileDialog::new()
                .add_filter("xuan project", &["xuan"])
                .set_file_name(format!("{}.xuan", session.title))
                .save_file()
        } else {
            session.path.clone()
        };
        let Some(mut path) = path else {
            return false;
        };
        if path.extension().is_none() {
            path.set_extension("xuan");
        }
        let session = self.session_mut().unwrap();
        match io::save(&session.document, &path) {
            Ok(()) => {
                session.title = path
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into();
                session.path = Some(path);
                session.history.mark_saved();
                self.status = "Project saved".into();
                true
            }
            Err(error) => {
                self.error = Some(format!("Could not save project\n\n{error:#}"));
                false
            }
        }
    }

    fn set_tool(&mut self, tool: Tool) {
        self.cancel_gesture();
        self.tool = tool;
        self.polygon.clear();
        self.crop_rect = None;
    }

    fn cancel_gesture(&mut self) {
        if let Some(gesture) = self.gesture.take()
            && !gesture.panning
            && let Some(session) = self.session_mut()
        {
            session.history.cancel(&mut session.document);
            session.invalidate();
        }
        self.guides.clear();
    }

    fn start_adjustment(&mut self, adjustment: Adjustment, as_layer: bool) {
        let Some(session) = self.session_mut() else {
            return;
        };
        session.history.begin(adjustment.name(), &session.document);
        self.effect = Some(EffectEdit {
            original: session.document.clone(),
            levels_source: None,
            adjustment: Some(adjustment),
            filter: None,
            filter_preview: filter_preview::FilterPreview::default(),
            as_layer,
            preview: true,
            refresh: true,
            channel: 0,
            target: None,
        });
        self.dialog = Some(Dialog::Effect);
    }

    fn start_filter(&mut self, filter: Filter) {
        let Some(session) = self.session_mut() else {
            return;
        };
        session.history.begin(filter.name(), &session.document);
        self.effect = Some(EffectEdit {
            original: session.document.clone(),
            levels_source: None,
            adjustment: None,
            filter: Some(filter),
            filter_preview: filter_preview::FilterPreview::default(),
            as_layer: false,
            preview: true,
            refresh: true,
            channel: 0,
            target: None,
        });
        self.dialog = Some(Dialog::Effect);
    }

    fn edit_adjustment_layer(&mut self, id: Uuid) {
        let adjustment = self
            .session()
            .and_then(|s| s.document.layers.iter().find(|l| l.id == id))
            .and_then(|l| l.adjustment.clone());
        if let Some(adjustment) = adjustment {
            self.start_adjustment(adjustment, false);
            if let Some(edit) = &mut self.effect {
                edit.target = Some(id);
            }
        }
    }

    fn start_filter_layer(&mut self, filter: Filter) {
        self.start_filter(filter);
        if let Some(edit) = &mut self.effect {
            edit.as_layer = true;
        }
    }

    fn edit_filter_layer(&mut self, id: Uuid) {
        let filter = self
            .session()
            .and_then(|s| s.document.layers.iter().find(|l| l.id == id))
            .and_then(|l| l.filter.clone());
        if let Some(filter) = filter {
            self.start_filter(filter);
            if let Some(edit) = &mut self.effect {
                edit.target = Some(id);
            }
        }
    }

    fn add_demo(&mut self) {
        let mut document = Document::new(1200, 900).unwrap();
        document.layers.clear();
        let sky = RgbaImage::from_fn(1200, 900, |x, y| {
            let t = y as f32 / 900.0;
            let grain = ((x.wrapping_mul(73) ^ y.wrapping_mul(137)) % 7) as f32 - 3.0;
            image::Rgba([
                (221.0 - t * 53.0 + grain) as u8,
                (183.0 - t * 65.0 + grain) as u8,
                (143.0 - t * 56.0 + grain) as u8,
                255,
            ])
        });
        document.layers.push(Layer::image("Warm paper", sky));
        document.layers.push(
            paint::shape(
                Point::new(758.0, 142.0),
                Point::new(944.0, 328.0),
                ShapeKind::Ellipse,
                [248, 222, 162, 255],
                0.0,
            )
            .unwrap(),
        );
        document.layers.last_mut().unwrap().name = "Afternoon sun".into();
        for (name, base, amplitude, phase, color) in [
            ("Distant ridge", 435.0, 80.0, 0.4, [173, 115, 84, 255]),
            ("Sandstone", 550.0, 130.0, 2.6, [137, 80, 60, 255]),
            ("Foreground dune", 695.0, 105.0, 4.4, [84, 58, 53, 255]),
        ] {
            let pixels = RgbaImage::from_fn(1200, 900, |x, y| {
                let line = base + (x as f32 / 420.0 + phase).sin() * amplitude;
                let mut c = color;
                c[3] = ((y as f32 - line).clamp(0.0, 1.0) * 255.0) as u8;
                image::Rgba(c)
            });
            document.layers.push(Layer::image(name, pixels));
        }
        document.select(document.layers[1].id, false);
        self.sessions
            .push(Session::new(document, "Dune study".into(), None));
        self.current = self.sessions.len() - 1;
    }

    fn command(&mut self, command: &str) {
        if self.job.is_some() || self.develop.is_some() {
            return;
        }
        match command {
            "develop" => {
                if let Some(id) = self.session().and_then(|s| s.document.active) {
                    self.start_develop_layer(id);
                }
            }
            "rasterize_raw" => self.edit("Rasterize RAW Layer", |doc| {
                let layer = doc
                    .active_mut()
                    .ok_or_else(|| anyhow::anyhow!("Select a RAW layer"))?;
                anyhow::ensure!(!layer.locked, "The layer is locked");
                layer.raw = None;
                Ok(())
            }),
            "levels" => self.start_adjustment(
                Adjustment::LevelsChannels {
                    ranges: [xuan::color::DEFAULT_LEVELS; 4],
                },
                false,
            ),
            "hue" => self.start_adjustment(
                Adjustment::HueRanges {
                    settings: Box::default(),
                },
                false,
            ),
            "curves" => self.start_adjustment(
                Adjustment::CurvesChannels {
                    channels: std::array::from_fn(|_| {
                        vec![Point::new(0.0, 0.0), Point::new(1.0, 1.0)]
                    }),
                },
                false,
            ),
            "content_fill" => {
                self.start_job("Content-Aware Fill", xuan::retouch::content_aware_fill)
            }
            "remove_background" => {
                let tolerance = self.tolerance;
                self.start_job("Remove Background", move |document, cancel| {
                    xuan::retouch::remove_background(document, tolerance, cancel)
                });
            }
            "new" => self.dialog = Some(Dialog::New),
            "open" => self.open_dialog(false),
            "import" => self.open_dialog(true),
            "open_comp" => {
                if let Some(path) = rfd::FileDialog::new()
                    .set_title("Open Compositor .comp package folder")
                    .pick_folder()
                {
                    self.open_path(&path, false);
                }
            }
            "save" => {
                self.save_current(false);
            }
            "save_as" => {
                self.save_current(true);
            }
            "export" => {
                self.dialog = Some(Dialog::Export);
                self.export_changed = true;
            }
            "close" => self.close_tab = Some(self.current),
            "undo" | "redo" => {
                if let Some(session) = self.session_mut() {
                    if command == "undo" {
                        session.history.undo(&mut session.document);
                    } else {
                        session.history.redo(&mut session.document);
                    }
                    session.invalidate();
                }
            }
            "new_layer" => self.edit("New Layer", |doc| {
                doc.insert(Layer::blank(
                    format!("Layer {}", doc.layers.len() + 1),
                    doc.width,
                    doc.height,
                ));
                Ok(())
            }),
            "duplicate" => self.edit("Duplicate Layers", |doc| {
                operations::duplicate(doc);
                Ok(())
            }),
            "delete_layer" => {
                self.edit("Delete Layers", |doc| {
                    doc.delete_selected();
                    Ok(())
                });
                self.mask_target = false;
            }
            "group" => self.edit("Group Layers", |doc| {
                operations::group(doc);
                Ok(())
            }),
            "move_out" => self.edit("Move Out of Group", |doc| {
                let parent = doc.active().and_then(|l| l.parent);
                let outer = parent
                    .and_then(|id| doc.layers.iter().find(|l| l.id == id))
                    .and_then(|l| l.parent);
                for layer in &mut doc.layers {
                    if doc.selected.contains(&layer.id) && layer.parent == parent {
                        layer.parent = outer;
                    }
                }
                Ok(())
            }),
            "ungroup" => self.edit("Ungroup Layers", |doc| {
                operations::ungroup(doc);
                Ok(())
            }),
            "merge" => self.edit("Merge Layers", |doc| operations::merge_selected(doc, true)),
            "flatten" => self.edit("Flatten Image", |doc| {
                let layer = Layer::image("Flattened", render::render(doc));
                doc.select(layer.id, false);
                doc.layers = vec![layer];
                Ok(())
            }),
            "mask" | "new_mask_layer" => {
                self.edit("Add Layer Mask", |doc| {
                    if command == "new_mask_layer" || doc.active().is_none() {
                        let mut layer = Layer::mask("Mask", doc.width, doc.height);
                        layer.mask.as_mut().unwrap().pixels =
                            Arc::new(paint::mask_from_selection(doc, &layer));
                        doc.insert(layer);
                        return Ok(());
                    }
                    if let Some(owner) = doc.active().and_then(|l| doc.attachment_owner(l)) {
                        let image = doc.layers.iter().find(|l| l.id == owner).unwrap();
                        let mut layer = Layer::mask("Mask", doc.width, doc.height);
                        layer.transform = image.transform;
                        layer.parent = Some(owner);
                        layer.mask.as_mut().unwrap().pixels =
                            Arc::new(paint::mask_from_selection(doc, image));
                        doc.select(layer.id, false);
                        doc.layers.push(layer);
                        return Ok(());
                    }
                    if doc.active().is_some_and(|l| l.standalone_mask) {
                        return Ok(());
                    }
                    let pixels = doc.active().map(|l| paint::mask_from_selection(doc, l));
                    if let (Some(layer), Some(pixels)) = (doc.active_mut(), pixels) {
                        layer.mask = Some(Mask {
                            pixels: Arc::new(pixels),
                            ..Mask::white()
                        });
                    }
                    Ok(())
                });
                self.mask_target = true;
                if let Some(session) = self.session_mut()
                    && let Some(parent) = session.document.active().and_then(|l| l.parent)
                {
                    session.collapsed.remove(&parent);
                }
                self.brush.color = [255; 4];
                self.background = [0, 0, 0, 255];
            }
            "delete_mask" => {
                self.edit("Delete Mask", |doc| {
                    if let Some(id) = doc.active().filter(|l| l.standalone_mask).map(|l| l.id) {
                        doc.select(id, false);
                        doc.delete_selected();
                        return Ok(());
                    }
                    if let Some(layer) = doc.active_mut() {
                        layer.mask = None;
                    }
                    Ok(())
                });
                self.mask_target = false;
            }
            "disable_mask" => self.edit("Toggle Mask", |doc| {
                if let Some(mask) = doc.active_mut().and_then(|l| l.mask.as_mut()) {
                    mask.enabled = !mask.enabled;
                }
                Ok(())
            }),
            "link_mask" => self.edit("Link Mask", |doc| {
                let attached = doc
                    .active()
                    .is_some_and(|l| l.is_effect() && doc.attachment_owner(l).is_some());
                if let Some(layer) = doc.active_mut()
                    && (!layer.standalone_mask || attached)
                    && let Some(mask) = &mut layer.mask
                {
                    mask.linked = !mask.linked;
                    if mask.placement.is_none() {
                        mask.placement = Some(layer.transform);
                    }
                }
                Ok(())
            }),
            "clip" => self.edit("Clipping Mask", |doc| {
                if let Some(index) = doc.layers.iter().position(|l| Some(l.id) == doc.active) {
                    let lower = doc.layers[..index]
                        .iter()
                        .rev()
                        .find(|l| {
                            l.parent == doc.layers[index].parent && !l.group && !l.is_effect()
                        })
                        .map(|l| l.clip_to.unwrap_or(l.id));
                    if !doc.layers[index].group
                        && !doc.layers[index].standalone_mask
                        && doc.layers[index].filter.is_none()
                    {
                        doc.layers[index].clip_to = if doc.layers[index].clip_to.is_some() {
                            None
                        } else {
                            lower
                        };
                    }
                }
                Ok(())
            }),
            "select_all" => self.edit_selection("Select All", |doc| {
                doc.selection = Some(Arc::new(GrayImage::from_pixel(
                    doc.width,
                    doc.height,
                    image::Luma([255]),
                )));
            }),
            "deselect" => self.edit_selection("Deselect", |doc| {
                doc.selection = None;
            }),
            "invert_selection" => self.edit_selection("Invert Selection", |doc| {
                if let Some(selection) = &doc.selection {
                    let mut pixels = (**selection).clone();
                    image::imageops::invert(&mut pixels);
                    doc.selection = Some(Arc::new(pixels));
                }
            }),
            "load_selection" => {
                let mask = self.editing_mask();
                self.edit_selection("Load Selection", |doc| {
                    operations::selection_from_layer(doc, mask);
                });
            }
            "feather" => self.edit_selection("Feather Selection", |doc| {
                if let Some(selection) = &doc.selection {
                    doc.selection = Some(Arc::new(xuan::gpu::blur_gray(selection, 3.0)));
                }
            }),
            "fill_fg" | "fill_bg" | "clear" => {
                let color = if command == "fill_bg" {
                    self.background
                } else {
                    self.brush.color
                };
                let mask = self.editing_mask();
                self.edit(
                    if command == "clear" {
                        "Clear Pixels"
                    } else {
                        "Fill"
                    },
                    |doc| paint::fill(doc, color, command == "clear", mask),
                );
            }
            "copy" | "copy_merged" | "cut" => {
                let Some(session) = self.session() else {
                    return;
                };
                let document = &session.document;
                if command == "cut" && document.active.is_none() {
                    self.error = Some("Select a layer before cutting pixels.".into());
                    return;
                }
                // A marquee can remain active after the Move tool deselects every layer.
                // Copy its visible contents when there is no layer to copy from.
                let merged = command == "copy_merged"
                    || (document.active.is_none() && document.selection.is_some());
                let Some((pixels, point)) = operations::copy_pixels(document, merged) else {
                    self.error = Some(if document.selection.is_some() {
                        "The selection is empty.".into()
                    } else {
                        "Select a layer or make a selection before copying.".into()
                    });
                    return;
                };
                self.connect_clipboard();
                if let Some(clipboard) = &mut self.system_clipboard
                    && let Err(error) = clipboard.set_image(arboard::ImageData {
                        width: pixels.width() as usize,
                        height: pixels.height() as usize,
                        bytes: std::borrow::Cow::Borrowed(pixels.as_raw()),
                    })
                {
                    self.error = Some(format!("Could not copy to the system clipboard\n\n{error}"));
                    return;
                }
                self.status = format!("Copied {} × {} pixels", pixels.width(), pixels.height());
                if self.system_clipboard.is_none() {
                    self.status
                        .push_str(" within Xuan; system clipboard unavailable");
                }
                self.clipboard = Some((pixels, point));
                if command == "cut" {
                    self.command("clear");
                }
            }
            "paste" => self.paste_clipboard(None),
            "flip_h" | "flip_v" => self.edit("Flip Layer", |doc| {
                if let Some(mut transform) = operations::transform_box(doc, false) {
                    if command == "flip_h" {
                        transform.flip_x = !transform.flip_x;
                    } else {
                        transform.flip_y = !transform.flip_y;
                    }
                    operations::apply_transform(doc, transform, false)?;
                }
                Ok(())
            }),
            "flip_canvas_h" | "flip_canvas_v" => self.edit("Flip Canvas", |doc| {
                operations::flip_canvas(doc, command == "flip_canvas_h");
                Ok(())
            }),
            "canvas_size" | "image_size" => {
                if let Some(session) = self.session() {
                    let dimensions = [session.document.width, session.document.height];
                    let resolution = session.document.resolution;
                    self.dimensions = dimensions;
                    self.resolution = resolution;
                    self.dialog = Some(if command == "canvas_size" {
                        Dialog::CanvasSize
                    } else {
                        Dialog::ImageSize
                    });
                }
            }
            "fit" => {
                if let Some(session) = self.session_mut() {
                    session.fit = true;
                }
            }
            "actual" => {
                if let Some(session) = self.session_mut() {
                    session.zoom = 1.0;
                    session.pan = Vec2::ZERO;
                    session.fit = false;
                }
            }
            "zoom_in" | "zoom_out" => {
                if let Some(session) = self.session_mut() {
                    session.zoom = (session.zoom * if command == "zoom_in" { 1.25 } else { 0.8 })
                        .clamp(0.01, 64.0);
                    session.fit = false;
                }
            }
            "invert" => {
                let mask = self.editing_mask();
                self.edit("Invert", |doc| {
                    xuan::effects::apply_adjustment(doc, &Adjustment::Invert, mask)
                });
            }
            "shortcuts" => self.dialog = Some(Dialog::Shortcuts),
            "about" => self.dialog = Some(Dialog::About),
            _ => {}
        }
    }
}

impl eframe::App for EditorApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        // Let the desktop show through outside the rounded client frame.
        [0.0; 4]
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.show(ctx);
    }
}

impl EditorApp {
    fn show(&mut self, ctx: &egui::Context) {
        if self.processor.is_none() {
            self.processor = self
                .gpu_state
                .as_ref()
                .filter(|state| {
                    state.adapter.get_info().device_type != wgpu::DeviceType::Cpu
                        && state.device.limits().max_storage_buffers_per_shader_stage >= 4
                })
                .map(|state| xuan::gpu::Processor::new(state.device.clone(), state.queue.clone()));
        }
        xuan::gpu::scope(self.processor.clone(), || self.show_with_processor(ctx));
    }

    fn show_with_processor(&mut self, ctx: &egui::Context) {
        self.poll_job();
        self.poll_develop(ctx);
        self.frames += 1;
        if self.develop.is_some()
            && !self.allow_close
            && ctx.input(|i| i.viewport().close_requested())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.develop_close_requested = true;
        }
        if ctx.input(|i| i.viewport().close_requested())
            && !self.allow_close
            && self.develop.is_none()
            && self.sessions.iter().any(|s| s.history.dirty())
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            if let Some(job) = &self.job {
                job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            self.close_app = true;
        }
        if self.dialog.is_none()
            && self.develop.is_none()
            && self.job.is_none()
            && self.error.is_none()
            && self.close_tab.is_none()
            && !self.close_app
        {
            self.shortcuts(ctx);
            let dropped = ctx.input(|i| i.raw.dropped_files.clone());
            for file in dropped {
                if let Some(path) = file.path {
                    self.open_path(&path, true);
                }
            }
        }
        // Keep antialiased panel seams opaque while preserving the rounded window corners.
        ctx.layer_painter(egui::LayerId::background()).rect_filled(
            ctx.content_rect(),
            theme::window_corner_radius(ctx),
            theme::PANEL,
        );
        self.window_resize(ctx);
        if self.develop.is_some() {
            self.develop_workspace(ctx);
        } else {
            self.menus(ctx);
            self.tabs(ctx);
            self.tool_options(ctx);
            self.status_bar(ctx);
            self.tool_rail(ctx);
            self.layers_panel(ctx);
            self.canvas(ctx);
        }
        self.dialogs(ctx);
        if self.gesture.is_none()
            && self.effect.is_none()
            && self.text_edit.is_none()
            && self.job.is_none()
            && !ctx.input(|i| i.pointer.any_down())
            && let Some(session) = self.session_mut()
        {
            session.history.commit();
        }
        let title = self.session().map_or("Xuan".to_owned(), |s| {
            format!(
                "{}{} —  Xuan",
                s.title,
                if s.history.dirty() { " •" } else { "" }
            )
        });
        if title != self.window_title {
            self.window_title = title.clone();
            // Viewport commands request another repaint, even for an unchanged title.
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));
        }
        if self.screenshot.is_some()
            && !self.screenshot_requested
            && self.frames >= 5
            && ctx.input(|i| i.time) >= 0.5
            && self
                .develop
                .as_ref()
                .is_none_or(|d| d.ready_for_screenshot())
        {
            self.screenshot_requested = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(egui::UserData::default()));
        }
        for event in ctx.input(|i| i.events.clone()) {
            if let egui::Event::Screenshot { image, .. } = event
                && let Some(path) = self.screenshot.take()
            {
                let bytes: Vec<_> = image.pixels.iter().flat_map(|p| p.to_array()).collect();
                match image::save_buffer(
                    &path,
                    &bytes,
                    image.width() as u32,
                    image.height() as u32,
                    image::ColorType::Rgba8,
                ) {
                    Ok(()) => {
                        println!("Screenshot saved to {}", path.display());
                        self.allow_close = true;
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    Err(error) => self.error = Some(error.to_string()),
                }
            }
        }
        if self.screenshot.is_some() {
            ctx.request_repaint();
        }
    }
}
