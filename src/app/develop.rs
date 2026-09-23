use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result, ensure};
use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2, emath::GuiRounding, pos2, vec2};
use image::RgbaImage;
use uuid::Uuid;
use xuan::{
    document::{Document, Layer, Point},
    raw::{self, DecodedRaw, DevelopSettings, OverlayKind, RawAsset, WhiteBalance},
};

use super::develop_preview::{PreparedPreview, PreviewImage, PreviewTexture, PreviewWorker};
use super::{EditorApp, Session, theme, widgets};

const PREVIEW_INTERVAL: Duration = Duration::from_millis(33);
const PREVIEW_SETTLE: Duration = Duration::from_millis(150);

#[derive(Clone, Debug)]
pub(super) enum DevelopTarget {
    New,
    Insert(Uuid),
    Existing { document: Uuid, layer: Uuid },
}

#[derive(Clone, Copy)]
pub(super) enum DevelopClose {
    Tab,
    Document(Uuid),
    Window,
}

enum WorkerResult {
    Loaded {
        asset: RawAsset,
        full: Arc<DecodedRaw>,
        proxy: Arc<DecodedRaw>,
        before: Option<PreviewImage>,
        preview: Box<PreparedPreview>,
    },
    Preview {
        revision: u64,
        side: u32,
        interactive: bool,
        crop: [f32; 4],
        preview: Box<PreparedPreview>,
        before: Option<PreviewImage>,
    },
    Applied {
        settings: DevelopSettings,
        pixels: RgbaImage,
    },
    Exported(PathBuf),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Compare {
    Edited,
    Original,
    Split,
    SideBySide,
}

#[derive(Clone, Copy)]
enum CanvasDrag {
    Pan { origin: Pos2, pan: Vec2 },
    Split { pointer_offset: f32 },
}

pub(super) struct Develop {
    pub id: Uuid,
    pub opened: Instant,
    target: DevelopTarget,
    pub title: String,
    pub asset: Option<RawAsset>,
    full: Option<Arc<DecodedRaw>>,
    pub proxy: Option<Arc<DecodedRaw>>,
    pub settings: DevelopSettings,
    receiver: Option<Receiver<Result<WorkerResult, String>>>,
    cancel: Arc<AtomicBool>,
    pub applying: bool,
    exporting: Option<PathBuf>,
    navigated_history: bool,
    pub error: Option<String>,
    notice: Option<String>,
    revision: u64,
    rendered_revision: Option<u64>,
    last_change: Instant,
    last_preview: Option<Instant>,
    preview_job: Option<bool>,
    worker: Arc<Mutex<PreviewWorker>>,
    texture: Option<PreviewTexture>,
    before: Option<PreviewTexture>,
    before_side: u32,
    rendered_side: u32,
    #[cfg(test)]
    preview: Option<RgbaImage>,
    pub histogram: [[u32; 256]; 3],
    pub clipping: [f32; 2],
    warning: Option<PreviewTexture>,
    pub show_clipping: bool,
    pub compare: Compare,
    pub split: f32,
    pub full_preview: bool,
    preview_side: u32,
    /// Physical screen pixels per full-resolution image pixel.
    pub zoom: f32,
    pub pan: Vec2,
    canvas_drag: Option<CanvasDrag>,
    pub fit: bool,
    pub picker: bool,
    pub panel: usize,
    pub curve_channel: usize,
    pub hsl_band: usize,
    pub selected_overlay: Option<usize>,
    pub draw_overlay: bool,
    pub show_mask: bool,
    pub undo: Vec<DevelopSettings>,
    pub redo: Vec<DevelopSettings>,
    pending_undo: Option<DevelopSettings>,
    pub last_brush_point: Option<Point>,
}

impl Develop {
    fn loading(target: DevelopTarget, title: String, settings: DevelopSettings) -> Self {
        Self {
            id: Uuid::new_v4(),
            opened: Instant::now(),
            target,
            title,
            settings,
            asset: None,
            full: None,
            proxy: None,
            receiver: None,
            cancel: Arc::new(AtomicBool::new(false)),
            applying: false,
            exporting: None,
            navigated_history: false,
            error: None,
            notice: None,
            revision: 0,
            rendered_revision: None,
            last_change: Instant::now(),
            last_preview: None,
            preview_job: None,
            worker: Arc::default(),
            texture: None,
            before: None,
            before_side: 0,
            rendered_side: 0,
            #[cfg(test)]
            preview: None,
            histogram: [[0; 256]; 3],
            clipping: [0.0; 2],
            warning: None,
            show_clipping: false,
            compare: Compare::Edited,
            split: 0.5,
            full_preview: false,
            preview_side: 0,
            zoom: 1.0,
            pan: Vec2::ZERO,
            canvas_drag: None,
            fit: true,
            picker: false,
            panel: 0,
            curve_channel: 0,
            hsl_band: 0,
            selected_overlay: None,
            draw_overlay: false,
            show_mask: false,
            undo: Vec::new(),
            redo: Vec::new(),
            pending_undo: None,
            last_brush_point: None,
        }
    }

    pub fn ready_for_screenshot(&self) -> bool {
        self.texture.is_some()
            && self.receiver.is_none()
            && self.rendered_revision == Some(self.revision)
            && self.rendered_side == self.preview_side
            && (self.compare == Compare::Edited || self.before_side == self.preview_side)
            && (!self.show_clipping || self.warning.is_some())
    }

    pub fn ready(&self) -> bool {
        self.full.is_some() && !self.applying && self.exporting.is_none()
    }

    pub fn view_command(&mut self, command: &str) {
        match command {
            "fit" => self.fit = true,
            "actual" => {
                self.fit = false;
                self.zoom = 1.0;
                self.pan = Vec2::ZERO;
            }
            "zoom_in" | "zoom_out" => {
                self.fit = false;
                let previous_zoom = self.zoom;
                self.zoom =
                    (self.zoom * if command == "zoom_in" { 1.25 } else { 0.8 }).clamp(0.02, 16.0);
                if self.compare == Compare::SideBySide {
                    self.pan *= self.zoom / previous_zoom;
                }
            }
            _ => {}
        }
    }

    pub fn targets_document(&self, id: Uuid) -> bool {
        match self.target {
            DevelopTarget::New => false,
            DevelopTarget::Insert(document) | DevelopTarget::Existing { document, .. } => {
                document == id
            }
        }
    }

    pub fn changed(&mut self) {
        // Let a short interactive render finish, but abandon an obsolete
        // settled refinement before it holds up the next interactive frame.
        if self.preview_job == Some(false) {
            self.cancel.store(true, Ordering::Relaxed);
        }
        self.revision += 1;
        self.last_change = Instant::now();
        self.error = None;
        self.notice = None;
    }

    fn update_preview_resolution(&mut self, ctx: &egui::Context) {
        let (Some(full), Some(proxy)) = (&self.full, &self.proxy) else {
            return;
        };
        let full_side = full.camera.width().max(full.camera.height());
        let limit = (ctx.input(|i| i.max_texture_side) as u32).min(full_side);
        let required = if self.full_preview {
            full_side
        } else {
            (full_side as f32 * self.zoom.min(1.0)).ceil() as u32
        };
        let mut side = proxy.camera.width().max(proxy.camera.height()).min(limit);
        while side < required.min(limit) {
            side = (side * 3 / 2).max(side + 1).min(limit);
        }
        if self.preview_side != side {
            self.preview_side = side;
            // Zoom does not change the development settings or undo history.
            // Cancel a previous refinement and request the new level directly.
            if self.preview_job.is_some() {
                self.cancel.store(true, Ordering::Relaxed);
            }
            ctx.request_repaint();
        }
        if required > limit {
            self.notice = Some("This image exceeds the GPU's full-resolution preview limit. Develop and TIFF export still use every source pixel.".into());
        }
    }

    fn preview_target(&self) -> (u32, bool) {
        let interactive = self.last_change.elapsed() < PREVIEW_SETTLE;
        let side = if interactive && !self.full_preview {
            self.proxy.as_ref().map_or(self.preview_side, |raw| {
                raw.camera
                    .width()
                    .max(raw.camera.height())
                    .min(self.preview_side)
            })
        } else {
            self.preview_side
        };
        (side, interactive)
    }

    fn needs_preview(&self, side: u32) -> bool {
        self.rendered_revision != Some(self.revision)
            || self.rendered_side != side
            || (self.show_clipping && self.warning.is_none())
            || (self.compare != Compare::Edited && self.before_side != side)
    }

    fn spawn(
        &mut self,
        ctx: &egui::Context,
        operation: impl FnOnce(&AtomicBool) -> Result<WorkerResult> + Send + 'static,
    ) {
        let (send, receive) = mpsc::channel();
        let context = ctx.clone();
        self.cancel = Arc::new(AtomicBool::new(false));
        self.preview_job = None;
        let cancel = self.cancel.clone();
        xuan::gpu::spawn(move || {
            let _cancel = xuan::gpu::cancellation(cancel.clone());
            let result =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| operation(&cancel)))
                    .unwrap_or_else(|_| Err(anyhow::anyhow!("RAW processing failed unexpectedly")))
                    .map_err(|e| format!("{e:#}"));
            let _ = send.send(result);
            context.request_repaint();
        });
        self.receiver = Some(receive);
    }

    fn set_preview(
        &mut self,
        ctx: &egui::Context,
        state: Option<&eframe::egui_wgpu::RenderState>,
        preview: PreparedPreview,
    ) {
        self.histogram = preview.histogram;
        self.clipping = preview.clipping;
        self.texture = Some(preview.image.register(ctx, state));
        self.warning = preview.warnings.map(|image| image.register(ctx, state));
        #[cfg(test)]
        {
            self.preview = preview.pixels;
        }
    }

    pub fn undo(&mut self, redo: bool) {
        self.navigated_history = true;
        if self.preview_job.is_some() {
            self.cancel.store(true, Ordering::Relaxed);
        }
        self.finish_undo();
        let state = if redo {
            self.redo.pop()
        } else {
            self.undo.pop()
        };
        if let Some(settings) = state {
            let previous = std::mem::replace(&mut self.settings, settings);
            if redo {
                self.undo.push(previous);
            } else {
                self.redo.push(previous);
            }
            self.selected_overlay = None;
            self.changed();
        }
    }

    fn finish_undo(&mut self) {
        if let Some(previous) = self.pending_undo.take() {
            self.undo.push(previous);
            if self.undo.len() > 64 {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }
}

#[cfg(test)]
fn texture(ctx: &egui::Context, name: &str, pixels: &RgbaImage) -> PreviewTexture {
    PreviewTexture::Cpu(ctx.load_texture(
        name,
        egui::ColorImage::from_rgba_unmultiplied(
            [pixels.width() as usize, pixels.height() as usize],
            pixels.as_raw(),
        ),
        egui::TextureOptions::LINEAR,
    ))
}

fn crop_rect(raw: &DecodedRaw, settings: &DevelopSettings) -> Rect {
    let size = vec2(raw.camera.width() as f32, raw.camera.height() as f32);
    let [left, top, right, bottom] = settings.crop;
    Rect::from_min_max(
        pos2((left * size.x).floor(), (top * size.y).floor()),
        pos2(
            (right * size.x).ceil().min(size.x),
            (bottom * size.y).ceil().min(size.y),
        ),
    )
}

impl EditorApp {
    pub(super) fn suspend_develop(&mut self) {
        if let Some(mut develop) = self.develop.take() {
            develop.finish_undo();
            develop.last_brush_point = None;
            if develop.preview_job.is_some() {
                develop.cancel.store(true, Ordering::Relaxed);
            }
            // Inactive tabs retain their displayed images and settings, without
            // tying up full-size compute buffers. Release them off the UI thread.
            let worker = develop.worker.clone();
            xuan::gpu::spawn(move || {
                worker
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .release_buffers()
            });
            self.inactive_develop.push(develop);
        }
    }

    pub(super) fn activate_develop(&mut self, id: Uuid) {
        if let Some(index) = self.inactive_develop.iter().position(|d| d.id == id) {
            self.cancel_gesture();
            let develop = self.inactive_develop.remove(index);
            self.suspend_develop();
            self.develop = Some(develop);
        }
    }

    pub(super) fn request_develop_close(&mut self, target: DevelopClose) {
        if self.develop.is_none()
            && let Some(id) = self.inactive_develop.first().map(|d| d.id)
        {
            self.activate_develop(id);
        }
        if self.develop.is_some() {
            self.develop_close_requested = Some(target);
        }
    }

    pub(super) fn request_project_close(&mut self, index: usize) {
        let Some(session) = self.sessions.get(index) else {
            return;
        };
        let document = session.document.id;
        let pending = self
            .develop
            .iter()
            .chain(&self.inactive_develop)
            .find(|d| d.targets_document(document))
            .map(|d| d.id);
        if let Some(id) = pending {
            self.activate_develop(id);
            self.request_develop_close(DevelopClose::Document(document));
        } else {
            self.close_tab = Some(index);
        }
    }

    pub(super) fn queue_raw(&mut self, path: &Path, as_layer: bool) {
        let target = if as_layer {
            self.session()
                .map_or(DevelopTarget::New, |s| DevelopTarget::Insert(s.document.id))
        } else {
            DevelopTarget::New
        };
        self.raw_queue.push_back((path.to_owned(), target));
        let ctx = self.context.clone();
        self.start_next_raw(&ctx);
        self.poll_develop(&ctx);
    }

    pub(super) fn start_develop_layer(&mut self, id: Uuid) {
        if self.develop.is_some() || self.job.is_some() || self.dialog.is_some() {
            return;
        }
        self.cancel_gesture();
        let Some(session) = self.session() else {
            return;
        };
        if let Some(pending) = self.inactive_develop.iter().find(|d| {
            matches!(d.target, DevelopTarget::Existing { document, layer } if document == session.document.id && layer == id)
        }) {
            let pending = pending.id;
            self.activate_develop(pending);
            return;
        }
        let Some(layer) = session.document.layers.iter().find(|l| l.id == id) else {
            return;
        };
        if layer.locked {
            self.error = Some("Unlock the RAW layer before developing it".into());
            return;
        }
        let Some(asset) = layer.raw.clone() else {
            return;
        };
        let mut develop = Develop::loading(
            DevelopTarget::Existing {
                document: session.document.id,
                layer: id,
            },
            asset.filename.clone(),
            asset.settings.clone(),
        );
        let worker = develop.worker.clone();
        let native = self.gpu_state.is_some();
        develop.spawn(&self.context, move |cancel| {
            let decoded = raw::decode(&asset.bytes)?;
            loaded(
                asset,
                decoded,
                &mut worker.lock().unwrap_or_else(|p| p.into_inner()),
                native,
                cancel,
            )
        });
        self.develop = Some(develop);
        self.mask_target = false;
    }

    fn start_next_raw(&mut self, ctx: &egui::Context) {
        if self.develop.is_none()
            && self.job.is_none()
            && self.dialog.is_none()
            && self.error.is_none()
            && !self.close_app
            && let Some((path, target)) = self.raw_queue.pop_front()
        {
            self.cancel_gesture();
            let mut develop = Develop::loading(
                target,
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into(),
                DevelopSettings::default(),
            );
            let worker = develop.worker.clone();
            let native = self.gpu_state.is_some();
            develop.spawn(ctx, move |cancel| {
                let (asset, decoded) = raw::open(&path)?;
                loaded(
                    asset,
                    decoded,
                    &mut worker.lock().unwrap_or_else(|p| p.into_inner()),
                    native,
                    cancel,
                )
            });
            self.develop = Some(develop);
        }
    }

    pub(super) fn poll_develop(&mut self, ctx: &egui::Context) {
        // Changing document tabs must not advance the import queue or steal focus.
        if self.inactive_develop.is_empty() {
            self.start_next_raw(ctx);
        }
        let Some(mut develop) = self.develop.take() else {
            return;
        };
        let result = develop
            .receiver
            .as_ref()
            .and_then(|rx| match rx.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("The RAW worker stopped unexpectedly".into()))
                }
            });
        if let Some(result) = result {
            develop.receiver = None;
            develop.preview_job = None;
            let result = if develop.cancel.load(Ordering::Relaxed) {
                None
            } else {
                Some(result)
            };
            if let Some(result) = result {
                match result {
                    Ok(WorkerResult::Loaded {
                        asset,
                        full,
                        proxy,
                        before,
                        preview,
                    }) => {
                        develop.settings = asset.settings.clone();
                        develop.asset = Some(asset);
                        develop.full = Some(full);
                        let side = proxy.camera.width().max(proxy.camera.height());
                        develop.proxy = Some(proxy);
                        develop.preview_side = side;
                        develop.rendered_side = side;
                        develop.before_side = if before.is_some() { side } else { 0 };
                        develop.before =
                            before.map(|image| image.register(ctx, self.gpu_state.as_ref()));
                        develop.set_preview(ctx, self.gpu_state.as_ref(), *preview);
                        develop.rendered_revision = Some(develop.revision);
                    }
                    Ok(WorkerResult::Preview {
                        revision,
                        side,
                        interactive,
                        crop,
                        preview,
                        before,
                    }) => {
                        // Show monotonically newer interactive snapshots during a
                        // drag. Crops must match the current canvas geometry; final
                        // refinements must match the exact latest revision.
                        let current = revision == develop.revision;
                        let progressing = interactive
                            && develop.last_change.elapsed() < PREVIEW_SETTLE
                            && develop
                                .rendered_revision
                                .is_none_or(|previous| revision > previous);
                        if (current || progressing) && crop == develop.settings.crop {
                            if let Some(before) = before {
                                develop.before =
                                    Some(before.register(ctx, self.gpu_state.as_ref()));
                                develop.before_side = side;
                            }
                            develop.set_preview(ctx, self.gpu_state.as_ref(), *preview);
                            develop.rendered_revision = Some(revision);
                            develop.rendered_side = side;
                        }
                    }
                    Ok(WorkerResult::Applied { settings, pixels }) => {
                        let mut asset = develop.asset.take().unwrap();
                        asset.settings = settings;
                        if let Err(error) =
                            self.apply_developed(&develop.target, asset.clone(), pixels)
                        {
                            develop.asset = Some(asset);
                            develop.error = Some(format!("{error:#}"));
                            develop.applying = false;
                        } else {
                            self.status =
                                "RAW developed · Double-click the RAW layer to edit it again"
                                    .into();
                            ctx.request_repaint();
                            return;
                        }
                    }
                    Ok(WorkerResult::Exported(path)) => {
                        develop.exporting = None;
                        develop.notice = Some(format!("Saved 16-bit TIFF · {}", path.display()));
                    }
                    Err(error) => {
                        develop.error = Some(error);
                        develop.applying = false;
                        develop.exporting = None;
                    }
                }
            }
        }
        if develop.receiver.is_none()
            && develop.error.is_none()
            && let Some(full) = develop.full.clone()
        {
            if let Some(path) = develop.exporting.clone() {
                let settings = develop.settings.clone();
                develop.spawn(ctx, move |cancel| {
                    ensure!(
                        path.extension().and_then(|v| v.to_str()).is_some_and(|v| v
                            .eq_ignore_ascii_case("tif")
                            || v.eq_ignore_ascii_case("tiff")),
                        "Save the 16-bit image with a .tif or .tiff extension"
                    );
                    let pixels = raw::render_16(&full, &settings, cancel)?;
                    let parent = path.parent().unwrap_or(Path::new("."));
                    let mut file = tempfile::NamedTempFile::new_in(parent)?;
                    use image::ImageEncoder;
                    let mut encoder = image::codecs::tiff::TiffEncoder::new(file.as_file_mut());
                    encoder
                        .set_icc_profile(include_bytes!("../../assets/color/sRGB.icc").to_vec())?;
                    encoder.write_image(
                        bytemuck::cast_slice(pixels.as_raw()),
                        pixels.width(),
                        pixels.height(),
                        image::ExtendedColorType::Rgba16,
                    )?;
                    ensure!(!cancel.load(Ordering::Relaxed), "Export cancelled");
                    file.as_file().sync_all()?;
                    file.persist(&path)?;
                    Ok(WorkerResult::Exported(path))
                });
            } else if develop.applying {
                let settings = develop.settings.clone();
                develop.spawn(ctx, move |cancel| {
                    let pixels = raw::render(&full, &settings, cancel)?;
                    Ok(WorkerResult::Applied { settings, pixels })
                });
            } else {
                let (side, interactive) = develop.preview_target();
                if develop.needs_preview(side)
                    && develop
                        .last_preview
                        .is_none_or(|started| started.elapsed() >= PREVIEW_INTERVAL)
                {
                    let proxy = develop.proxy.clone().unwrap();
                    let settings = develop.settings.clone();
                    let revision = develop.revision;
                    let compare = develop.compare != Compare::Edited;
                    let warnings = develop.show_clipping;
                    let native = self.gpu_state.is_some();
                    let worker = develop.worker.clone();
                    develop.spawn(ctx, move |cancel| {
                        let mut worker = worker.lock().unwrap_or_else(|p| p.into_inner());
                        let input = worker.input(&full, &proxy, side, cancel)?;
                        ensure!(!cancel.load(Ordering::Relaxed), "RAW preview cancelled");
                        let before = compare
                            .then(|| worker.original(&input, native, cancel))
                            .transpose()?;
                        let preview = worker.render(&input, &settings, warnings, native, cancel)?;
                        Ok(WorkerResult::Preview {
                            revision,
                            side,
                            interactive,
                            crop: settings.crop,
                            preview: Box::new(preview),
                            before,
                        })
                    });
                    develop.preview_job = Some(interactive);
                    develop.last_preview = Some(Instant::now());
                }
            }
        }
        if develop.receiver.is_some() || develop.needs_preview(develop.preview_side) {
            ctx.request_repaint_after(PREVIEW_INTERVAL);
        }

        self.develop = Some(develop);
    }

    fn apply_developed(
        &mut self,
        target: &DevelopTarget,
        asset: RawAsset,
        pixels: RgbaImage,
    ) -> Result<()> {
        match target {
            DevelopTarget::New => {
                let mut document = Document::new(pixels.width(), pixels.height())?;
                let title = PathBuf::from(&asset.filename)
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                let mut layer = Layer::image(&title, pixels);
                layer.raw = Some(asset);
                document.layers = vec![layer];
                document.select(document.layers[0].id, false);
                document.validate()?;
                let mut session = Session::new(document, title, None);
                session.history.mark_modified();
                self.sessions.push(session);
                self.current = self.sessions.len() - 1;
            }
            DevelopTarget::Insert(id) | DevelopTarget::Existing { document: id, .. } => {
                let index = self
                    .sessions
                    .iter()
                    .position(|s| s.document.id == *id)
                    .context("The target project is no longer open")?;
                let session = &mut self.sessions[index];
                let mut document = session.document.clone();
                match target {
                    DevelopTarget::Insert(_) => {
                        let mut layer = Layer::image(
                            Path::new(&asset.filename)
                                .file_stem()
                                .unwrap_or_default()
                                .to_string_lossy(),
                            pixels,
                        );
                        layer.transform.x = (document.width as f32 - layer.transform.width) * 0.5;
                        layer.transform.y = (document.height as f32 - layer.transform.height) * 0.5;
                        layer.raw = Some(asset);
                        document.insert(layer);
                    }
                    DevelopTarget::Existing { layer, .. } => {
                        let target = document
                            .layers
                            .iter_mut()
                            .find(|l| l.id == *layer)
                            .context("The RAW layer is no longer available")?;
                        raw::update_layer(target, asset, pixels)?;
                    }
                    DevelopTarget::New => unreachable!(),
                }
                document.validate()?;
                session.history.begin("Develop RAW", &session.document);
                session.document = document;
                session.history.commit();
                session.invalidate();
                self.current = index;
            }
        }
        self.tool = super::Tool::Move;
        self.mask_target = false;
        Ok(())
    }

    pub(super) fn cancel_develop(&mut self) {
        if let Some(develop) = self.develop.take() {
            develop.cancel.store(true, Ordering::Relaxed);
        }
        self.develop_close_requested = None;
        self.status = "RAW development cancelled".into();
        self.context.request_repaint();
    }

    pub(super) fn develop_workspace(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.develop.take() else {
            return;
        };
        d.navigated_history = false;
        let previous = d.settings.clone();
        let mut apply = false;
        let mut export = false;
        let mut cancel = false;
        let interactive =
            d.ready() && self.develop_close_requested.is_none() && self.dialog.is_none();
        egui::TopBottomPanel::top("develop_toolbar")
            .exact_height(44.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::symmetric(12, 8)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.add_enabled_ui(interactive, |ui| {
                        apply = widgets::primary_button(ui, "Develop").clicked();
                    });
                    ui.add_enabled_ui(
                        self.develop_close_requested.is_none() && self.dialog.is_none(),
                        |ui| {
                            cancel = widgets::button(ui, "Cancel").clicked();
                        },
                    );
                    ui.add_enabled_ui(interactive, |ui| {
                        export = widgets::button(ui, "16-bit TIFF…").clicked();
                    });
                    ui.separator();
                    ui.add_enabled_ui(interactive, |ui| {
                        widgets::segmented(
                            ui,
                            &mut d.compare,
                            &[
                                (Compare::Edited, "Edited"),
                                (Compare::Original, "Original"),
                                (Compare::Split, "Split"),
                                (Compare::SideBySide, "Side by side"),
                            ],
                        );
                        widgets::checkbox(ui, &mut d.show_clipping, "Clipping");
                    });
                });
            });
        super::chrome::status_bar(ctx, "develop_status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if d.receiver.is_some() {
                    ui.spinner();
                }
                ui.label(if d.exporting.is_some() {
                    "Exporting 16-bit TIFF…"
                } else if d.applying {
                    "Developing full-resolution image…"
                } else if d.full.is_none() && d.error.is_none() {
                    "Decoding RAW sensor data…"
                } else if d.receiver.is_some() || d.needs_preview(d.preview_side) {
                    "Updating preview…"
                } else if d.picker {
                    "Click a neutral gray area to set white balance"
                } else if d.draw_overlay {
                    "Drag on the image to place the selected mask"
                } else if let Some(notice) = &d.notice {
                    notice
                } else {
                    "RAW embedded · 32-bit float processing · sRGB photo layer"
                });
                if let Some(asset) = &d.asset {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(format!(
                            "{} × {} · {}",
                            asset.metadata.width,
                            asset.metadata.height,
                            if d.full
                                .as_ref()
                                .zip(d.texture.as_ref())
                                .is_some_and(|(full, texture)| texture.size_vec2()
                                    == crop_rect(full, &d.settings).size())
                            {
                                "Full resolution"
                            } else {
                                "Preview"
                            }
                        ));
                    });
                }
            });
        });
        egui::SidePanel::right("develop_controls")
            .default_width(330.0)
            .width_range(330.0..=420.0)
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(12))
            .show(ctx, |ui| {
                ui.add_enabled_ui(interactive, |ui| {
                    ui.push_id(d.id, |ui| {
                        super::develop_controls::controls(ui, &mut d);
                    });
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::CANVAS))
            .show(ctx, |ui| {
                if let Some(error) = &d.error {
                    ui.colored_label(Color32::from_rgb(255, 140, 140), error);
                    if d.full.is_some() && widgets::button(ui, "Retry preview").clicked() {
                        d.changed();
                    }
                }
                if let Some(texture) = d.texture.clone() {
                    draw_canvas(ui, &mut d, texture, interactive);
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label(if d.error.is_some() {
                            "Unable to develop this RAW file"
                        } else {
                            "Opening RAW…"
                        });
                    });
                }
            });
        if d.settings != previous && !d.navigated_history {
            d.pending_undo.get_or_insert(previous);
            d.changed();
        }
        if !ctx.input(|i| i.pointer.any_down()) {
            d.finish_undo();
        }
        d.update_preview_resolution(ctx);
        if apply && d.settings.validate().is_ok() {
            d.finish_undo();
            if d.preview_job.is_some() {
                d.cancel.store(true, Ordering::Relaxed);
            }
            d.applying = true;
            d.error = None;
            ctx.request_repaint();
        }
        if export
            && let Some(mut path) = rfd::FileDialog::new()
                .add_filter("16-bit TIFF", &["tif", "tiff"])
                .set_file_name(format!(
                    "{}.tif",
                    Path::new(&d.title)
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                ))
                .save_file()
        {
            if path.extension().is_none() {
                path.set_extension("tif");
            }
            if d.preview_job.is_some() {
                d.cancel.store(true, Ordering::Relaxed);
            }
            d.exporting = Some(path);
            d.error = None;
            ctx.request_repaint();
        }
        self.develop = Some(d);
        if cancel {
            self.cancel_develop();
        }
        if let Some(target) = self.develop_close_requested {
            let mut discard = false;
            let mut keep = false;
            widgets::Window::new("Finish developing?").show(ctx, |ui| {
                ui.label(if matches!(target, DevelopClose::Window) && !self.inactive_develop.is_empty() {
                    "Develop the images to keep your RAW adjustments in projects, or discard all open Develop sessions."
                } else {
                    "Develop the image to keep your RAW adjustments in a project, or discard this Develop session."
                });
                ui.horizontal(|ui| {
                    keep = widgets::primary_button(ui, "Keep developing").clicked();
                    discard = widgets::button(ui, "Discard and close").clicked();
                });
            });
            if keep {
                self.develop_close_requested = None;
            }
            if discard {
                self.cancel_develop();
                match target {
                    DevelopClose::Tab => {}
                    DevelopClose::Document(id) => {
                        if let Some(index) = self.sessions.iter().position(|s| s.document.id == id)
                        {
                            self.request_project_close(index);
                        }
                    }
                    DevelopClose::Window => {
                        self.raw_queue.clear();
                        for develop in self.inactive_develop.drain(..) {
                            develop.cancel.store(true, Ordering::Relaxed);
                        }
                        if self.sessions.iter().any(|s| s.history.dirty()) {
                            self.close_app = true;
                        } else {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                }
            }
        }
    }
}

fn loaded(
    asset: RawAsset,
    decoded: DecodedRaw,
    worker: &mut PreviewWorker,
    native: bool,
    cancel: &AtomicBool,
) -> Result<WorkerResult> {
    ensure!(!cancel.load(Ordering::Relaxed), "Cancelled");
    let full = Arc::new(decoded);
    let proxy = Arc::new(full.preview_cancellable(1600, cancel)?);
    let preview = worker.render(&proxy, &asset.settings, false, native, cancel)?;
    let before = if asset.settings == DevelopSettings::default() {
        let before = preview.image.clone();
        worker.remember_original(
            proxy.camera.width().max(proxy.camera.height()),
            before.clone(),
        );
        Some(before)
    } else {
        None
    };
    Ok(WorkerResult::Loaded {
        asset,
        full,
        proxy,
        before,
        preview: Box::new(preview),
    })
}

fn draw_canvas(ui: &mut egui::Ui, d: &mut Develop, texture: PreviewTexture, interactive: bool) {
    let (viewport, response) = ui.allocate_exact_size(ui.available_size(), Sense::click_and_drag());
    let before = d.before.as_ref().unwrap_or(&texture);
    let full = d.full.as_ref().unwrap();
    let source = crop_rect(full, &d.settings);
    // Keep zoom and pan independent of the texture while a sharper render arrives.
    let image_size = source.size() / ui.pixels_per_point();
    let side_by_side = d.compare == Compare::SideBySide;
    let available = vec2(
        viewport.width() / if side_by_side { 2.0 } else { 1.0 },
        viewport.height(),
    );
    if d.fit {
        d.zoom = ((available.x - 36.0) / image_size.x)
            .min((available.y - 36.0) / image_size.y)
            .max(0.01);
        d.pan = Vec2::ZERO;
    }
    let center = viewport.center() + vec2(if side_by_side { available.x * 0.5 } else { 0.0 }, 0.0);
    let anchor = if side_by_side {
        super::canvas::ZoomAnchor::Center
    } else {
        super::canvas::ZoomAnchor::Pointer
    };
    if interactive
        && super::canvas::scroll_canvas(
            ui,
            &response,
            center,
            anchor,
            &mut d.zoom,
            &mut d.pan,
            0.02..=16.0,
        )
    {
        d.fit = false;
    }
    let mut rect = Rect::from_center_size(center + d.pan, image_size * d.zoom);
    if d.zoom == 1.0 {
        // At 100%, align texels with physical pixels to avoid interpolation blur.
        rect = rect.translate(rect.min.round_to_pixels(ui.pixels_per_point()) - rect.min);
    }
    let painter = ui.painter().with_clip_rect(viewport);
    let edited_viewport = if side_by_side {
        Rect::from_min_max(pos2(viewport.center().x, viewport.top()), viewport.max)
    } else {
        viewport
    };
    painter
        .with_clip_rect(edited_viewport)
        .rect_filled(rect, 0.0, Color32::from_gray(50));
    let uv = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
    let original_uv = Rect::from_min_max(
        pos2(
            source.left() / full.camera.width() as f32,
            source.top() / full.camera.height() as f32,
        ),
        pos2(
            source.right() / full.camera.width() as f32,
            source.bottom() / full.camera.height() as f32,
        ),
    );
    let shown = if d.show_clipping {
        d.warning.as_ref().unwrap_or(&texture)
    } else {
        &texture
    };
    match d.compare {
        Compare::Original => {
            painter.image(before.id(), rect, original_uv, Color32::WHITE);
        }
        Compare::Edited => {
            painter.image(shown.id(), rect, uv, Color32::WHITE);
        }
        Compare::SideBySide => {
            let mut original_rect = rect.translate(vec2(-available.x, 0.0));
            if d.zoom == 1.0 {
                original_rect = original_rect.translate(
                    original_rect.min.round_to_pixels(ui.pixels_per_point()) - original_rect.min,
                );
            }
            let original_viewport =
                Rect::from_min_max(viewport.min, pos2(viewport.center().x, viewport.bottom()));
            painter.with_clip_rect(original_viewport).image(
                before.id(),
                original_rect,
                original_uv,
                Color32::WHITE,
            );
            painter
                .with_clip_rect(edited_viewport)
                .image(shown.id(), rect, uv, Color32::WHITE);
        }
        Compare::Split => {
            painter.image(shown.id(), rect, uv, Color32::WHITE);
            let x = rect.left() + rect.width() * d.split;
            let clip = Rect::from_min_max(rect.min, pos2(x, rect.bottom())).intersect(viewport);
            painter
                .with_clip_rect(clip)
                .image(before.id(), rect, original_uv, Color32::WHITE);
            painter.line_segment(
                [pos2(x, rect.top()), pos2(x, rect.bottom())],
                Stroke::new(1.5_f32, Color32::WHITE),
            );
        }
    }
    if !interactive {
        d.canvas_drag = None;
        return;
    }
    let force_pan = ui.input(|i| {
        i.key_down(egui::Key::Space)
            || i.pointer.button_down(egui::PointerButton::Middle)
            || (d.compare == Compare::Split && i.modifiers.alt)
    });
    let split_x = rect.left() + rect.width() * d.split;
    let over_divider = |point: Pos2| {
        d.compare == Compare::Split
            && rect.intersect(viewport).contains(point)
            && (point.x - split_x).abs() <= 8.0
    };
    if response.drag_started()
        && let Some(origin) = ui.input(|i| i.pointer.press_origin())
    {
        // Keep the gesture chosen at its origin, even after leaving the divider.
        d.canvas_drag = if force_pan {
            Some(CanvasDrag::Pan { origin, pan: d.pan })
        } else if !d.picker && !d.draw_overlay {
            Some(if over_divider(origin) {
                CanvasDrag::Split {
                    pointer_offset: origin.x - split_x,
                }
            } else {
                CanvasDrag::Pan { origin, pan: d.pan }
            })
        } else {
            None
        };
    }
    if response.hovered() || response.dragged() {
        let cursor = match d.canvas_drag {
            Some(CanvasDrag::Pan { .. }) => egui::CursorIcon::Grabbing,
            Some(CanvasDrag::Split { .. }) => egui::CursorIcon::ResizeHorizontal,
            None if force_pan => egui::CursorIcon::Grab,
            None if d.picker || d.draw_overlay => egui::CursorIcon::Crosshair,
            None if response.hover_pos().is_some_and(over_divider) => {
                egui::CursorIcon::ResizeHorizontal
            }
            None => egui::CursorIcon::Grab,
        };
        ui.ctx().set_cursor_icon(cursor);
    }
    let point = response
        .interact_pointer_pos()
        .filter(|p| rect.contains(*p) && edited_viewport.contains(*p))
        .map(|p| {
            Point::new(
                d.settings.crop[0]
                    + (p.x - rect.left()) / rect.width()
                        * (d.settings.crop[2] - d.settings.crop[0]),
                d.settings.crop[1]
                    + (p.y - rect.top()) / rect.height()
                        * (d.settings.crop[3] - d.settings.crop[1]),
            )
        });
    let other_brush_points: usize = d
        .settings
        .overlays
        .iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != d.selected_overlay)
        .map(|(_, overlay)| overlay.points.len())
        .sum();
    let brush_limit = 8192_usize.saturating_sub(other_brush_points);
    if let Some(drag) = d.canvas_drag {
        if let Some(pointer) = response.interact_pointer_pos() {
            match drag {
                CanvasDrag::Pan { origin, pan } => {
                    d.fit = false;
                    d.pan = pan + (pointer - origin);
                }
                CanvasDrag::Split { pointer_offset } => {
                    d.split =
                        ((pointer.x - pointer_offset - rect.left()) / rect.width()).clamp(0.0, 1.0);
                }
            }
        }
        d.last_brush_point = None;
    } else if !force_pan
        && d.picker
        && response.clicked()
        && let (Some(point), Some(raw)) = (point, &d.proxy)
    {
        let source = raw::source_point(
            point,
            &d.settings,
            raw.camera.width() as f32 / raw.camera.height() as f32,
        );
        d.settings.custom_wb = raw::sample_white_balance(raw, source);
        d.settings.white_balance = WhiteBalance::Custom;
        d.settings.tint = 0.0;
        d.picker = false;
    } else if !force_pan
        && d.draw_overlay
        && let Some(overlay) = d
            .selected_overlay
            .and_then(|i| d.settings.overlays.get_mut(i))
    {
        if let Some(point) = point {
            if response.drag_started() || response.clicked() {
                if overlay.kind == OverlayKind::Brush {
                    d.last_brush_point = None;
                } else {
                    overlay.start = point;
                    overlay.end = point;
                }
            }
            if response.dragged() || response.clicked() {
                if overlay.kind == OverlayKind::Brush {
                    if overlay.points.len() < brush_limit {
                        if let Some(previous) = d.last_brush_point {
                            let distance = previous.distance(point);
                            let steps = (distance / (overlay.radius * 0.3)).ceil().clamp(1.0, 256.0)
                                as usize;
                            for i in 1..=steps {
                                if overlay.points.len() >= brush_limit {
                                    break;
                                }
                                let t = i as f32 / steps as f32;
                                overlay.points.push(Point::new(
                                    previous.x + (point.x - previous.x) * t,
                                    previous.y + (point.y - previous.y) * t,
                                ));
                            }
                        } else {
                            overlay.points.push(point);
                        }
                        d.last_brush_point = Some(point);
                    }
                } else {
                    overlay.end = point;
                }
            }
        }
        if response.drag_stopped() {
            d.last_brush_point = None;
        }
    }
    if response.drag_stopped() || !ui.input(|i| i.pointer.any_down()) {
        d.canvas_drag = None;
    }
    if d.show_mask
        && let Some(overlay) = d.selected_overlay.and_then(|i| d.settings.overlays.get(i))
    {
        let painter = painter.with_clip_rect(edited_viewport);
        let to_screen = |p: Point| {
            pos2(
                rect.left()
                    + (p.x - d.settings.crop[0]) / (d.settings.crop[2] - d.settings.crop[0])
                        * rect.width(),
                rect.top()
                    + (p.y - d.settings.crop[1]) / (d.settings.crop[3] - d.settings.crop[1])
                        * rect.height(),
            )
        };
        let stroke = Stroke::new(1.5_f32, Color32::from_rgb(255, 185, 75));
        if overlay.kind == OverlayKind::Brush {
            for point in &overlay.points {
                painter.circle_stroke(
                    to_screen(*point),
                    overlay.radius * rect.height() / (d.settings.crop[3] - d.settings.crop[1]),
                    stroke,
                );
            }
        } else {
            let a = to_screen(overlay.start);
            let b = to_screen(overlay.end);
            painter.line_segment([a, b], stroke);
            painter.circle_filled(a, 4.0, stroke.color);
            painter.circle_stroke(b, 5.0, stroke);
            if overlay.kind == OverlayKind::Radial {
                painter.add(egui::epaint::EllipseShape::stroke(a, (b - a).abs(), stroke));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (RawAsset, Arc<DecodedRaw>) {
        let raw = Arc::new(DecodedRaw {
            camera: image::Rgb32FImage::from_pixel(64, 48, image::Rgb([0.18; 3])),
            as_shot: [1.0; 3],
            camera_to_rgb: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            xyz_to_camera: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            metadata: raw::RawMetadata {
                width: 64,
                height: 48,
                ..Default::default()
            },
        });
        let asset = RawAsset {
            filename: "fixture.NEF".into(),
            metadata: raw.metadata.clone(),
            settings: DevelopSettings::default(),
            bytes: Arc::new(vec![1, 2, 3]),
        };
        (asset, raw)
    }

    fn ready(ctx: &egui::Context) -> Develop {
        ready_with_resolution(ctx, [64, 48], 64)
    }

    fn ready_with_resolution(ctx: &egui::Context, size: [u32; 2], proxy_side: u32) -> Develop {
        let (mut asset, mut raw) = fixture();
        let full = Arc::get_mut(&mut raw).unwrap();
        full.camera = image::Rgb32FImage::from_fn(size[0], size[1], |x, y| {
            image::Rgb([if (x + y) % 2 == 0 { 0.1 } else { 0.8 }; 3])
        });
        full.metadata.width = size[0];
        full.metadata.height = size[1];
        asset.metadata = full.metadata.clone();
        let proxy = Arc::new(raw.preview(proxy_side));
        let mut d = Develop::loading(
            DevelopTarget::New,
            asset.filename.clone(),
            asset.settings.clone(),
        );
        let pixels = raw::render(&proxy, &asset.settings, &AtomicBool::new(false)).unwrap();
        d.asset = Some(asset);
        d.full = Some(raw);
        d.proxy = Some(proxy);
        d.preview_side = proxy_side;
        d.rendered_side = proxy_side;
        d.before_side = proxy_side;
        d.set_preview(
            ctx,
            None,
            PreparedPreview::cpu(pixels.clone(), false, &AtomicBool::new(false)).unwrap(),
        );
        d.before = Some(texture(ctx, "before", &pixels));
        d.rendered_revision = Some(0);
        d
    }

    fn frame(
        ctx: &egui::Context,
        app: &mut EditorApp,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 860.0))),
                events,
                ..Default::default()
            },
            |ctx| app.show(ctx),
        )
    }

    fn image_rect(output: &egui::FullOutput, texture: egui::TextureId) -> Rect {
        output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Mesh(mesh) if mesh.texture_id == texture => Some(mesh.calc_bounds()),
                _ => None,
            })
            .expect("Develop preview is visible")
    }

    fn canvas_frame(
        ctx: &egui::Context,
        d: &mut Develop,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        canvas_input(ctx, d, events, egui::Modifiers::NONE)
    }

    fn canvas_input(
        ctx: &egui::Context,
        d: &mut Develop,
        events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
    ) -> egui::FullOutput {
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(160.0, 144.0))),
                events,
                modifiers,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    draw_canvas(ui, d, d.texture.clone().unwrap(), true);
                });
                d.update_preview_resolution(ctx);
            },
        )
    }

    #[test]
    fn split_view_pans_without_moving_the_divider_or_editing_masks() {
        for (button, space, modifiers, on_divider, mask) in [
            (
                egui::PointerButton::Primary,
                false,
                egui::Modifiers::NONE,
                false,
                false,
            ),
            (
                egui::PointerButton::Primary,
                true,
                egui::Modifiers::NONE,
                true,
                false,
            ),
            (
                egui::PointerButton::Middle,
                false,
                egui::Modifiers::NONE,
                true,
                false,
            ),
            (
                egui::PointerButton::Primary,
                false,
                egui::Modifiers::ALT,
                true,
                false,
            ),
            (
                egui::PointerButton::Primary,
                true,
                egui::Modifiers::NONE,
                true,
                true,
            ),
            (
                egui::PointerButton::Middle,
                false,
                egui::Modifiers::NONE,
                true,
                true,
            ),
        ] {
            let ctx = egui::Context::default();
            let mut d = ready(&ctx);
            d.compare = Compare::Split;
            if mask {
                d.settings.overlays.push(raw::Overlay::default());
                d.selected_overlay = Some(0);
                d.draw_overlay = true;
            }
            let settings = d.settings.clone();
            let texture = d.texture.as_ref().unwrap().id();
            let rect = image_rect(&canvas_frame(&ctx, &mut d, vec![]), texture);
            let start = rect.center() - vec2(if on_divider { 0.0 } else { 28.0 }, 0.0);
            let mut events = vec![egui::Event::PointerMoved(start)];
            if space {
                events.push(egui::Event::Key {
                    key: egui::Key::Space,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers,
                });
            }
            canvas_input(&ctx, &mut d, events, modifiers);
            canvas_input(
                &ctx,
                &mut d,
                vec![egui::Event::PointerButton {
                    pos: start,
                    button,
                    pressed: true,
                    modifiers,
                }],
                modifiers,
            );
            for delta in [vec2(15.0, 5.0), vec2(35.0, 10.0)] {
                canvas_input(
                    &ctx,
                    &mut d,
                    vec![egui::Event::PointerMoved(start + delta)],
                    modifiers,
                );
                assert_eq!(d.pan, delta, "The image should follow the pan gesture");
                assert_eq!(d.split, 0.5);
                assert!(!d.fit);
                assert_eq!(d.settings, settings);
            }
            canvas_input(
                &ctx,
                &mut d,
                vec![egui::Event::PointerButton {
                    pos: start + vec2(35.0, 10.0),
                    button,
                    pressed: false,
                    modifiers,
                }],
                modifiers,
            );
            assert!(d.undo.is_empty());
        }
    }

    #[test]
    fn split_divider_drag_stays_attached_without_snapping_to_the_pointer() {
        let ctx = egui::Context::default();
        let mut d = ready(&ctx);
        d.compare = Compare::Split;
        let texture = d.texture.as_ref().unwrap().id();
        let rect = image_rect(&canvas_frame(&ctx, &mut d, vec![]), texture);
        let start = rect.center() + vec2(4.0, 0.0);
        canvas_frame(&ctx, &mut d, vec![egui::Event::PointerMoved(start)]);
        canvas_frame(
            &ctx,
            &mut d,
            vec![egui::Event::PointerButton {
                pos: start,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        for delta in [20.0, 35.0] {
            canvas_frame(
                &ctx,
                &mut d,
                vec![egui::Event::PointerMoved(start + vec2(delta, 0.0))],
            );
            assert!((d.split - (0.5 + delta / rect.width())).abs() < 0.001);
            assert_eq!(d.pan, Vec2::ZERO);
            assert!(d.fit);
        }
    }

    #[test]
    fn preview_resolution_tracks_zoom_and_display_density() {
        let ctx = egui::Context::default();
        let mut d = ready_with_resolution(&ctx, [256, 192], 128);
        canvas_frame(&ctx, &mut d, vec![]);
        assert!(d.preview_side == 128);

        ctx.set_pixels_per_point(2.0);
        canvas_frame(&ctx, &mut d, vec![]);
        canvas_frame(&ctx, &mut d, vec![]);
        assert!(
            d.preview_side > 128,
            "Fit must cover physical display pixels"
        );
        assert!(
            !d.full_preview,
            "Automatic detail must not enable the override"
        );

        ctx.set_pixels_per_point(1.0);
        canvas_frame(&ctx, &mut d, vec![]);
        canvas_frame(&ctx, &mut d, vec![]);
        assert!(d.preview_side == 128);
        d.view_command("zoom_in");
        canvas_frame(&ctx, &mut d, vec![]);
        assert!(d.preview_side > 128, "Menu zoom must load full detail");

        d.view_command("fit");
        canvas_frame(&ctx, &mut d, vec![]);
        assert!(d.preview_side == 128);
        d.full_preview = true;
        canvas_frame(&ctx, &mut d, vec![]);
        assert!(d.preview_side > 128, "The override must also apply to Fit");
        assert!(d.undo.is_empty());
    }

    #[test]
    fn wheel_zoom_refines_both_comparisons_without_moving_the_image() {
        let ctx = egui::Context::default();
        let mut d = ready_with_resolution(&ctx, [256, 192], 128);
        let proxy_texture = d.texture.as_ref().unwrap().id();
        let rect = image_rect(&canvas_frame(&ctx, &mut d, vec![]), proxy_texture);
        let pointer = rect.lerp_inside(vec2(0.6, 0.4));
        let point = (pointer - rect.min) / rect.size();
        canvas_frame(&ctx, &mut d, vec![egui::Event::PointerMoved(pointer)]);
        canvas_frame(
            &ctx,
            &mut d,
            vec![egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, 200.0),
                modifiers: egui::Modifiers::NONE,
            }],
        );
        for _ in 0..30 {
            canvas_frame(&ctx, &mut d, vec![]);
        }
        assert!(d.preview_side > 128);
        let rect = image_rect(&canvas_frame(&ctx, &mut d, vec![]), proxy_texture);
        assert!(((pointer - rect.min) / rect.size() - point).length() < 0.001);
        let (zoom, pan) = (d.zoom, d.pan);

        // Exercise the real worker, including a matching-resolution Original.
        d.compare = Compare::Split;
        d.last_change = Instant::now() - Duration::from_secs(1);
        let mut app = EditorApp::with_context(&ctx, vec![], false, None);
        app.develop = Some(d);
        app.poll_develop(&ctx);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !app.develop.as_ref().unwrap().ready_for_screenshot() {
            assert!(Instant::now() < deadline, "Preview worker did not finish");
            std::thread::sleep(Duration::from_millis(5));
            app.poll_develop(&ctx);
        }
        let d = app.develop.as_mut().unwrap();
        let input = d.full.as_ref().unwrap().preview(d.preview_side);
        let expected = raw::render(&input, &d.settings, &AtomicBool::new(false)).unwrap();
        assert_eq!(d.preview.as_ref().unwrap(), &expected);
        assert_eq!(
            d.before.as_ref().unwrap().size(),
            [
                input.camera.width() as usize,
                input.camera.height() as usize
            ]
        );
        let full_texture = d.texture.as_ref().unwrap().id();
        let refined = image_rect(&canvas_frame(&ctx, d, vec![]), full_texture);
        assert_eq!(refined, rect);
        assert_eq!((d.zoom, d.pan), (zoom, pan));
        d.compare = Compare::Original;
        let before_texture = d.before.as_ref().unwrap().id();
        assert_eq!(
            image_rect(&canvas_frame(&ctx, d, vec![]), before_texture),
            rect
        );
    }

    #[test]
    fn actual_pixels_uses_cropped_source_size_at_each_display_scale() {
        for scale in [1.0, 1.5, 2.0] {
            let ctx = egui::Context::default();
            ctx.set_pixels_per_point(scale);
            let mut d = ready_with_resolution(&ctx, [256, 192], 128);
            d.settings.crop = [0.251, 0.251, 0.749, 0.749];
            d.view_command("actual");
            let texture = d.texture.as_ref().unwrap().id();
            let rect = image_rect(&canvas_frame(&ctx, &mut d, vec![]), texture);
            assert!((rect.size() * scale - vec2(128.0, 96.0)).length() < 0.001);
            let origin = rect.min.to_vec2() * scale;
            assert!((origin - origin.round()).length() < 0.001);
            assert!(d.preview_side > 128);
            d.compare = Compare::SideBySide;
            let original_texture = d.before.as_ref().unwrap().id();
            let output = canvas_frame(&ctx, &mut d, vec![]);
            let original = image_rect(&output, original_texture);
            let origin = original.min.to_vec2() * scale;
            assert!((origin - origin.round()).length() < 0.001);
            let mesh = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Mesh(mesh) if mesh.texture_id == original_texture => Some(mesh),
                    _ => None,
                })
                .unwrap();
            for vertex in &mesh.vertices {
                assert!([0.25, 0.75].contains(&vertex.uv.x));
                assert!([0.25, 0.75].contains(&vertex.uv.y));
            }
        }
    }

    #[test]
    fn limited_preview_preserves_zoom_and_pan_without_repeated_work() {
        let ctx = egui::Context::default();
        let mut d = ready_with_resolution(&ctx, [256, 192], 128);
        d.view_command("actual");
        d.pan = vec2(13.0, -7.0);
        ctx.input_mut(|i| i.max_texture_side = 128);
        d.update_preview_resolution(&ctx);
        d.update_preview_resolution(&ctx);
        assert!(d.preview_side == 128);
        assert_eq!(d.revision, 0);
        assert!(!d.fit);
        assert_eq!(d.zoom, 1.0);
        assert_eq!(d.pan, vec2(13.0, -7.0));
        assert!(d.notice.as_ref().unwrap().contains("preview limit"));
    }

    fn click(ctx: &egui::Context, app: &mut EditorApp, pos: Pos2) {
        frame(ctx, app, vec![egui::Event::PointerMoved(pos)]);
        for pressed in [true, false] {
            frame(
                ctx,
                app,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                }],
            );
        }
    }

    fn click_text(ctx: &egui::Context, app: &mut EditorApp, label: &str) {
        // Floating windows use their first frame to measure their contents.
        frame(ctx, app, vec![]);
        let output = frame(ctx, app, vec![]);
        let pos = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text() == label => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("Missing UI label: {label}"));
        click(ctx, app, pos);
    }

    #[test]
    fn shared_tabs_preserve_raw_sessions_and_route_menu_commands() {
        let ctx = egui::Context::default();
        let mut app = EditorApp::with_context(&ctx, vec![], false, None);
        app.dimensions = [32, 24];
        app.new_document();
        app.sessions[0].title = "Photo".into();
        let mut d = ready(&ctx);
        d.undo.push(d.settings.clone());
        d.settings.exposure = 1.25;
        d.fit = false;
        d.zoom = 2.0;
        d.pan = vec2(30.0, -15.0);
        d.panel = 1;
        let id = d.id;
        app.develop = Some(d);
        // A queued RAW must not steal focus when we switch to a document.
        app.raw_queue
            .push_back((PathBuf::from("queued.NEF"), DevelopTarget::New));
        click_text(&ctx, &mut app, "Photo");
        assert!(app.develop.is_none());
        assert_eq!(app.inactive_develop.len(), 1);
        app.command("fill_fg");
        frame(&ctx, &mut app, vec![]);
        assert!(app.develop.is_none());
        assert_eq!(app.raw_queue.len(), 1);
        let history = app.sessions[0].history.names().count();
        assert_eq!(history, 1);

        click_text(&ctx, &mut app, "fixture.NEF · RAW");
        let d = app.develop.as_ref().unwrap();
        assert_eq!(d.id, id);
        assert_eq!(d.settings.exposure, 1.25);
        assert_eq!(d.zoom, 2.0);
        assert_eq!(d.pan, vec2(30.0, -15.0));
        assert_eq!(d.panel, 1);
        click_text(&ctx, &mut app, "Edit");
        click_text(&ctx, &mut app, "Undo RAW adjustment");
        assert_eq!(app.develop.as_ref().unwrap().settings.exposure, 0.0);
        assert_eq!(app.sessions[0].history.names().count(), history);
        app.command("redo");
        assert_eq!(app.develop.as_ref().unwrap().settings.exposure, 1.25);
        click_text(&ctx, &mut app, "100%");
        assert_eq!(app.develop.as_ref().unwrap().zoom, 1.0);
        assert!(!app.develop.as_ref().unwrap().full_preview);
        click_text(&ctx, &mut app, "Fit");
        assert!(app.develop.as_ref().unwrap().fit);

        app.command("new");
        assert!(app.develop.is_none());
        assert!(matches!(app.dialog, Some(super::super::Dialog::New)));
        app.dialog = None;
        let mut second = ready(&ctx);
        second.title = "second.NEF".into();
        second.settings.exposure = -0.5;
        let second_id = second.id;
        app.develop = Some(second);
        click_text(&ctx, &mut app, "fixture.NEF · RAW");
        assert_eq!(app.develop.as_ref().unwrap().id, id);
        assert_eq!(app.develop.as_ref().unwrap().settings.exposure, 1.25);
        click_text(&ctx, &mut app, "second.NEF · RAW");
        assert_eq!(app.develop.as_ref().unwrap().id, second_id);
        assert_eq!(app.develop.as_ref().unwrap().settings.exposure, -0.5);
    }

    #[test]
    fn shared_window_and_project_close_preserve_pending_raw_until_confirmed() {
        let ctx = egui::Context::default();
        let mut app = EditorApp::with_context(&ctx, vec![], false, None);
        app.dimensions = [32, 24];
        app.new_document();
        app.sessions[0].title = "Photo".into();
        let document = app.sessions[0].document.id;
        let layer = app.sessions[0].document.layers[0].id;
        let mut d = ready(&ctx);
        d.target = DevelopTarget::Existing { document, layer };
        d.settings.exposure = 1.25;
        let id = d.id;
        let cancelled = d.cancel.clone();
        app.develop = Some(d);
        frame(
            &ctx,
            &mut app,
            vec![egui::Event::PointerMoved(pos2(61.0, 16.0))],
        );
        assert!(
            app.develop_close_requested.is_none(),
            "Hovering window controls must not close Develop"
        );
        click_text(&ctx, &mut app, "Photo");
        app.command("close");
        assert_eq!(app.develop.as_ref().unwrap().id, id);
        assert!(
            matches!(app.develop_close_requested, Some(DevelopClose::Document(target)) if target == document)
        );
        assert_eq!(app.sessions.len(), 1);
        assert!(!cancelled.load(Ordering::Relaxed));
        click_text(&ctx, &mut app, "Keep developing");
        assert!(app.develop_close_requested.is_none());
        assert_eq!(app.develop.as_ref().unwrap().settings.exposure, 1.25);

        click_text(&ctx, &mut app, "Photo");
        click(&ctx, &mut app, pos2(21.0, 16.0));
        assert!(matches!(
            app.develop_close_requested,
            Some(DevelopClose::Window)
        ));
        assert_eq!(app.develop.as_ref().unwrap().id, id);
        assert!(!cancelled.load(Ordering::Relaxed));
        click_text(&ctx, &mut app, "Keep developing");
        app.command("close");
        assert!(matches!(
            app.develop_close_requested,
            Some(DevelopClose::Tab)
        ));
        click_text(&ctx, &mut app, "Discard and close");
        assert!(cancelled.load(Ordering::Relaxed));
        assert!(app.develop.is_none());
        assert_eq!(app.sessions.len(), 1);
    }

    #[test]
    fn develop_canvas_wheel_pans_horizontally_without_zooming() {
        for fit in [true, false] {
            for (delta, modifiers) in [
                (vec2(-1.0, 0.0), egui::Modifiers::NONE),
                (vec2(1.0, 0.0), egui::Modifiers::NONE),
                (vec2(0.0, -1.0), egui::Modifiers::SHIFT),
            ] {
                let ctx = egui::Context::default();
                let mut app = EditorApp::with_context(&ctx, vec![], false, None);
                let mut d = ready(&ctx);
                d.fit = fit;
                d.zoom = 0.01;
                let texture = d.texture.as_ref().unwrap().id();
                app.develop = Some(d);
                let rect = image_rect(&frame(&ctx, &mut app, vec![]), texture);
                let pointer = rect.center();
                frame(&ctx, &mut app, vec![egui::Event::PointerMoved(pointer)]);
                let d = app.develop.as_ref().unwrap();
                let (zoom, pan) = (d.zoom, d.pan);
                frame(
                    &ctx,
                    &mut app,
                    vec![egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Line,
                        delta,
                        modifiers,
                    }],
                );
                for _ in 0..30 {
                    frame(&ctx, &mut app, vec![]);
                }
                let d = app.develop.as_ref().unwrap();
                assert_eq!(d.zoom, zoom);
                assert_eq!(d.pan.y, pan.y);
                assert_eq!((d.pan.x - pan.x).signum(), (delta.x + delta.y).signum());
                assert!(!d.fit);
                assert!(d.undo.is_empty());
            }
        }
    }

    #[test]
    fn develop_canvas_wheel_zoom_keeps_pointer_anchored_in_single_image_views() {
        for (compare, original) in [
            (Compare::Edited, false),
            (Compare::Original, true),
            (Compare::Split, false),
        ] {
            for (fit, zoom, delta) in [
                (true, 1.0, 1.0),
                (false, 2.0, 1.0),
                (false, 2.0, -1.0),
                (false, 16.0, 1.0),
                (false, 0.02, -1.0),
            ] {
                let ctx = egui::Context::default();
                let mut app = EditorApp::with_context(&ctx, vec![], false, None);
                let mut d = ready(&ctx);
                d.compare = compare;
                d.fit = fit;
                d.zoom = zoom;
                d.pan = vec2(30.0, -15.0);
                let texture = if original { &d.before } else { &d.texture }
                    .as_ref()
                    .unwrap()
                    .id();
                app.develop = Some(d);
                let rect = image_rect(&frame(&ctx, &mut app, vec![]), texture);
                let pointer = rect.lerp_inside(vec2(0.6, 0.4));
                let point = (pointer - rect.min) / rect.size();
                frame(&ctx, &mut app, vec![egui::Event::PointerMoved(pointer)]);
                let old_zoom = app.develop.as_ref().unwrap().zoom;
                frame(
                    &ctx,
                    &mut app,
                    vec![egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Line,
                        delta: vec2(0.0, delta),
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
                for _ in 0..30 {
                    let rect = image_rect(&frame(&ctx, &mut app, vec![]), texture);
                    let after = (pointer - rect.min) / rect.size();
                    assert!(
                        (after - point).length() < 0.001,
                        "Zoom moved the image under the pointer"
                    );
                }
                let d = app.develop.as_ref().unwrap();
                if delta > 0.0 && old_zoom < 16.0 {
                    assert!(d.zoom > old_zoom);
                } else if delta < 0.0 && old_zoom > 0.02 {
                    assert!(d.zoom < old_zoom);
                } else {
                    assert_eq!(d.zoom, old_zoom);
                }
                assert!(!d.fit);
                assert!(d.undo.is_empty());
            }
        }
    }

    #[test]
    fn side_by_side_zoom_keeps_both_pane_centers_anchored() {
        for (fit, zoom, delta) in [
            (true, 1.0, 1.0),
            (false, 2.0, 1.0),
            (false, 2.0, -1.0),
            (false, 16.0, 1.0),
            (false, 0.02, -1.0),
        ] {
            for original in [true, false] {
                let ctx = egui::Context::default();
                let mut d = ready(&ctx);
                d.compare = Compare::SideBySide;
                d.fit = fit;
                d.zoom = zoom;
                d.pan = vec2(12.0, -8.0);
                let textures = [
                    d.before.as_ref().unwrap().id(),
                    d.texture.as_ref().unwrap().id(),
                ];
                let output = canvas_frame(&ctx, &mut d, vec![]);
                let viewports = textures.map(|texture| {
                    output
                        .shapes
                        .iter()
                        .find_map(|shape| match &shape.shape {
                            egui::Shape::Mesh(mesh) if mesh.texture_id == texture => {
                                Some(shape.clip_rect)
                            }
                            _ => None,
                        })
                        .unwrap()
                });
                let points = std::array::from_fn::<_, 2, _>(|i| {
                    let rect = image_rect(&output, textures[i]);
                    (viewports[i].center() - rect.min) / rect.size()
                });
                let pointer = if original {
                    viewports[0].lerp_inside(vec2(0.2, 0.3))
                } else {
                    viewports[1].lerp_inside(vec2(0.8, 0.7))
                };
                canvas_frame(&ctx, &mut d, vec![egui::Event::PointerMoved(pointer)]);
                let previous_zoom = d.zoom;
                canvas_frame(
                    &ctx,
                    &mut d,
                    vec![egui::Event::MouseWheel {
                        unit: egui::MouseWheelUnit::Line,
                        delta: vec2(0.0, delta),
                        modifiers: egui::Modifiers::NONE,
                    }],
                );
                let check_centers = |output: &egui::FullOutput| {
                    for i in 0..2 {
                        let rect = image_rect(output, textures[i]);
                        let after = (viewports[i].center() - rect.min) / rect.size();
                        assert!(
                            (after - points[i]).length() < 0.001,
                            "Zoom must keep the same image point at each pane center"
                        );
                    }
                };
                for _ in 0..30 {
                    check_centers(&canvas_frame(&ctx, &mut d, vec![]));
                }
                if previous_zoom > 0.02 && previous_zoom < 16.0 {
                    assert_eq!((d.zoom - previous_zoom).signum(), delta.signum());
                } else {
                    assert_eq!(d.zoom, previous_zoom);
                }
                for command in ["zoom_in", "zoom_out"] {
                    d.view_command(command);
                    check_centers(&canvas_frame(&ctx, &mut d, vec![]));
                }
                assert!(!d.fit);
                assert!(d.undo.is_empty());
            }
        }
    }

    #[test]
    fn develop_workspace_renders_all_panels_and_undo_retains_redo() {
        let ctx = egui::Context::default();
        let mut app = EditorApp::with_context(&ctx, vec![], false, None);
        app.develop = Some(ready(&ctx));
        let d = app.develop.as_mut().unwrap();
        d.settings.brightness = 12.34;
        d.settings.rotation = 1.25;
        // Presets can contain valid brush radii outside the control's edit range.
        d.settings.overlays = [0.002, 0.75]
            .map(|radius| raw::Overlay {
                kind: OverlayKind::Brush,
                radius,
                ..Default::default()
            })
            .to_vec();
        d.selected_overlay = Some(0);
        let settings = d.settings.clone();
        settings.validate().unwrap();
        for panel in 0..6 {
            app.develop.as_mut().unwrap().panel = panel;
            frame(&ctx, &mut app, vec![]);
        }
        let d = app.develop.as_mut().unwrap();
        d.panel = 4;
        d.selected_overlay = Some(1);
        frame(&ctx, &mut app, vec![]);
        let d = app.develop.as_mut().unwrap();
        assert_eq!(d.settings, settings);
        assert!(d.undo.is_empty(), "Viewing controls must not create edits");
        d.undo.push(d.settings.clone());
        d.settings.exposure = 1.0;
        let event = egui::Event::Key {
            key: egui::Key::Z,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::CTRL,
        };
        frame(&ctx, &mut app, vec![event]);
        let d = app.develop.as_ref().unwrap();
        assert_eq!(d.settings.exposure, 0.0);
        assert_eq!(d.redo.len(), 1);
        assert!(d.undo.is_empty());
        app.develop.as_mut().unwrap().undo(true);
        assert_eq!(app.develop.as_ref().unwrap().settings.exposure, 1.0);
    }

    #[test]
    fn raw_apply_targets_its_own_document_and_undo_restores_all_assets() {
        let ctx = egui::Context::default();
        let mut app = EditorApp::with_context(&ctx, vec![], false, None);
        let (asset, raw) = fixture();
        let pixels = raw::render(&raw, &asset.settings, &AtomicBool::new(false)).unwrap();
        app.apply_developed(&DevelopTarget::New, asset.clone(), pixels.clone())
            .unwrap();
        assert!(app.session().unwrap().history.dirty());
        let doc = &mut app.session_mut().unwrap().document;
        doc.layers[0].transform.x = 14.0;
        doc.layers[0].mask = Some(xuan::document::Mask::white());
        doc.layers[0].opacity = 0.7;
        let target = DevelopTarget::Existing {
            document: doc.id,
            layer: doc.layers[0].id,
        };
        let before = doc.clone();
        app.dimensions = [50, 50];
        app.new_document();
        let other_id = app.session().unwrap().document.id;
        let mut asset = asset;
        asset.settings.exposure = 1.0;
        let updated = raw::render(&raw, &asset.settings, &AtomicBool::new(false)).unwrap();
        app.apply_developed(&target, asset, updated).unwrap();
        let session = app.session_mut().unwrap();
        assert_eq!(session.document.id, before.id);
        assert_eq!(
            session.document.layers[0].transform,
            before.layers[0].transform
        );
        assert_eq!(session.document.layers[0].opacity, 0.7);
        assert!(session.document.layers[0].mask.is_some());
        assert!(session.history.undo(&mut session.document));
        assert_eq!(session.document.layers[0].pixels, before.layers[0].pixels);
        assert_eq!(
            session.document.layers[0]
                .raw
                .as_ref()
                .unwrap()
                .settings
                .exposure,
            0.0
        );
        assert_eq!(app.sessions[1].document.id, other_id);
        assert!(app.sessions[1].document.layers[0].raw.is_none());
    }

    #[test]
    fn cancel_ignores_late_worker_results_and_stale_previews() {
        let ctx = egui::Context::default();
        let mut app = EditorApp::with_context(&ctx, vec![], false, None);
        let mut d = ready(&ctx);
        let before = d.preview.clone();
        let original = d.before.as_ref().unwrap().id();
        let (tx, rx) = mpsc::channel();
        d.receiver = Some(rx);
        d.changed();
        tx.send(Ok(WorkerResult::Preview {
            revision: 0,
            side: 1,
            interactive: false,
            crop: d.settings.crop,
            preview: Box::new(
                PreparedPreview::cpu(RgbaImage::new(1, 1), false, &AtomicBool::new(false)).unwrap(),
            ),
            before: Some(PreviewImage::Cpu(Arc::new(egui::ColorImage::filled(
                [1, 1],
                Color32::BLACK,
            )))),
        }))
        .unwrap();
        app.develop = Some(d);
        app.poll_develop(&ctx);
        assert_eq!(app.develop.as_ref().unwrap().preview, before);
        assert_eq!(
            app.develop.as_ref().unwrap().before.as_ref().unwrap().id(),
            original
        );
        let mut d = app.develop.take().unwrap();
        let cancel = d.cancel.clone();
        let (tx, rx) = mpsc::channel();
        d.receiver = Some(rx);
        app.develop = Some(d);
        app.cancel_develop();
        assert!(cancel.load(Ordering::Relaxed));
        assert!(
            tx.send(Ok(WorkerResult::Applied {
                settings: DevelopSettings::default(),
                pixels: RgbaImage::new(1, 1)
            }))
            .is_err()
        );
        app.poll_develop(&ctx);
        assert!(app.sessions.is_empty());
        assert!(app.develop.is_none());
    }

    #[test]
    fn cancel_and_invalid_commit_leave_existing_document_untouched() {
        let ctx = egui::Context::default();
        let mut app = EditorApp::with_context(&ctx, vec![], false, None);
        let (asset, raw) = fixture();
        let pixels = raw::render(&raw, &asset.settings, &AtomicBool::new(false)).unwrap();
        app.apply_developed(&DevelopTarget::New, asset.clone(), pixels.clone())
            .unwrap();
        let before = app.session().unwrap().document.clone();
        app.start_develop_layer(before.layers[0].id);
        app.cancel_develop();
        assert_eq!(
            app.session().unwrap().document.layers[0].pixels,
            before.layers[0].pixels
        );
        app.session_mut().unwrap().document.layers[0].locked = true;
        let target = DevelopTarget::Existing {
            document: before.id,
            layer: before.layers[0].id,
        };
        assert!(app.apply_developed(&target, asset, pixels).is_err());
        assert_eq!(
            app.session().unwrap().document.layers[0].pixels,
            before.layers[0].pixels
        );
    }

    #[test]
    fn develop_exports_true_sixteen_bit_tiff_with_srgb_profile() {
        use image::ImageDecoder;
        let ctx = egui::Context::default();
        let mut app = EditorApp::with_context(&ctx, vec![], false, None);
        let mut d = ready(&ctx);
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("developed.tif");
        d.exporting = Some(path.clone());
        app.develop = Some(d);
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            app.poll_develop(&ctx);
            let d = app.develop.as_ref().unwrap();
            assert!(d.error.is_none(), "{:?}", d.error);
            if d.exporting.is_none() {
                break;
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(app.sessions.is_empty());
        assert!(app.develop.as_ref().unwrap().notice.is_some());
        let mut decoder = image::ImageReader::open(&path)
            .unwrap()
            .into_decoder()
            .unwrap();
        assert_eq!(decoder.color_type(), image::ColorType::Rgba16);
        assert_eq!(
            decoder.icc_profile().unwrap().unwrap(),
            include_bytes!("../../assets/color/sRGB.icc")
        );
        let pixels = image::open(&path).unwrap().to_rgba16();
        assert_ne!(pixels.get_pixel(0, 0)[0] % 257, 0);
        assert_eq!(pixels.get_pixel(0, 0)[3], 65_535);
    }

    #[test]
    fn raw_file_imports_enter_develop_and_preserve_the_layer_destination() {
        use super::super::clipboard::ClipboardContent;

        let ctx = egui::Context::default();
        let dir = tempfile::tempdir().unwrap();
        for filename in [
            "photo.NEF",
            "photo.nrw",
            "photo.cr2",
            "photo.CR3",
            "photo.CrW",
        ] {
            // Invalid bytes still enter Develop; decoding errors belong to its worker.
            let path = dir.path().join(filename);
            std::fs::write(&path, b"broken camera file").unwrap();
            for import in ["open", "layer", "paste"] {
                let mut app = EditorApp::with_context(&ctx, vec![], false, None);
                app.dimensions = [64, 48];
                app.new_document();
                let document = app.session().unwrap().document.id;
                let layer_count = app.session().unwrap().document.layers.len();
                if import == "paste" {
                    app.paste_content(ClipboardContent::Files(vec![path.clone()]));
                } else {
                    app.open_path(&path, import == "layer");
                }
                assert!(app.error.is_none(), "{filename}: {:?}", app.error);
                let develop = app.develop.as_ref().expect("RAW import opens Develop");
                assert_eq!(develop.title, filename);
                match develop.target {
                    DevelopTarget::New => assert_eq!(import, "open"),
                    DevelopTarget::Insert(id) => {
                        assert_ne!(import, "open");
                        assert_eq!(id, document);
                    }
                    DevelopTarget::Existing { .. } => panic!("Expected a new RAW session"),
                }
                assert_eq!(app.sessions.len(), 1);
                assert_eq!(app.session().unwrap().document.layers.len(), layer_count);
                app.cancel_develop();
            }
        }
    }

    #[test]
    #[ignore = "Set XUAN_TEST_RAW to a local camera file"]
    fn sample_raw_opens_develop_commits_and_reopens() {
        let ctx = egui::Context::default();
        let mut app = EditorApp::with_context(&ctx, vec![], false, None);
        let path = PathBuf::from(
            std::env::var_os("XUAN_TEST_RAW")
                .or_else(|| std::env::var_os("XUAN_TEST_NEF"))
                .expect("Set XUAN_TEST_RAW"),
        );
        app.open_path(&path, false);
        assert!(app.sessions.is_empty());
        assert!(app.develop.is_some());
        let wait = |app: &mut EditorApp, applying: bool| {
            let deadline = Instant::now() + Duration::from_secs(60);
            loop {
                app.poll_develop(&ctx);
                if applying && app.develop.is_none() {
                    break;
                }
                if !applying
                    && app
                        .develop
                        .as_ref()
                        .is_some_and(|d| d.ready_for_screenshot())
                {
                    break;
                }
                if let Some(error) = app.develop.as_ref().and_then(|d| d.error.as_ref()) {
                    panic!("{error}");
                }
                assert!(Instant::now() < deadline, "RAW operation timed out");
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        wait(&mut app, false);
        app.develop.as_mut().unwrap().settings.exposure = 0.5;
        app.develop.as_mut().unwrap().applying = true;
        wait(&mut app, true);
        let layer = app.session().unwrap().document.active().unwrap();
        assert_eq!(
            layer.pixels.as_ref().unwrap().dimensions(),
            (
                layer.raw.as_ref().unwrap().metadata.width,
                layer.raw.as_ref().unwrap().metadata.height
            )
        );
        assert_eq!(layer.raw.as_ref().unwrap().settings.exposure, 0.5);
        let id = layer.id;
        app.start_develop_layer(id);
        wait(&mut app, false);
        assert_eq!(app.develop.as_ref().unwrap().settings.exposure, 0.5);
        app.develop.as_mut().unwrap().settings.exposure = 2.0;
        app.cancel_develop();
        assert_eq!(
            app.session()
                .unwrap()
                .document
                .active()
                .unwrap()
                .raw
                .as_ref()
                .unwrap()
                .settings
                .exposure,
            0.5
        );
    }
    #[test]
    fn intermediate_preview_levels_cover_display_pixels_without_forcing_full_size() {
        let ctx = egui::Context::default();
        let mut d = ready_with_resolution(&ctx, [960, 640], 160);
        d.fit = false;
        d.zoom = 0.35;
        d.update_preview_resolution(&ctx);
        assert_eq!(d.preview_side, 360);
        assert_eq!(d.revision, 0, "view changes do not edit the RAW settings");
        d.view_command("actual");
        d.update_preview_resolution(&ctx);
        assert_eq!(d.preview_side, 960);
        assert!(
            !d.full_preview,
            "100% does not permanently force full-resolution Fit previews"
        );
        d.zoom = 0.1;
        d.update_preview_resolution(&ctx);
        assert_eq!(d.preview_side, 160);
        d.full_preview = true;
        d.update_preview_resolution(&ctx);
        assert_eq!(d.preview_side, 960);
    }

    #[test]
    fn preview_updates_start_during_edits_and_refine_after_they_settle() {
        let ctx = egui::Context::default();
        let mut app = EditorApp::with_context(&ctx, vec![], false, None);
        let mut d = ready_with_resolution(&ctx, [256, 192], 128);
        d.preview_side = 256;
        d.settings.exposure = 0.5;
        d.changed();
        app.develop = Some(d);
        app.poll_develop(&ctx);
        assert_eq!(
            app.develop.as_ref().unwrap().preview_job,
            Some(true),
            "start immediately instead of waiting for a pause"
        );
        let wait = |app: &mut EditorApp| {
            let deadline = Instant::now() + Duration::from_secs(10);
            while app.develop.as_ref().unwrap().receiver.is_some() {
                assert!(Instant::now() < deadline);
                std::thread::sleep(Duration::from_millis(2));
                app.poll_develop(&ctx);
                assert!(app.develop.as_ref().unwrap().error.is_none());
            }
        };
        wait(&mut app);
        let d = app.develop.as_mut().unwrap();
        assert_eq!(d.rendered_revision, Some(d.revision));
        assert_eq!(d.rendered_side, 128);
        assert!(d.warning.is_none());
        d.last_change = Instant::now() - PREVIEW_SETTLE;
        d.last_preview = None;
        app.poll_develop(&ctx);
        wait(&mut app);
        let d = app.develop.as_mut().unwrap();
        assert_eq!(d.rendered_side, 256);
        assert_eq!(
            d.before_side, 128,
            "Edited mode does not render an unused full-size Original"
        );
        d.compare = Compare::Split;
        d.show_clipping = true;
        d.last_preview = None;
        app.poll_develop(&ctx);
        wait(&mut app);
        let d = app.develop.as_ref().unwrap();
        assert_eq!(d.before_side, 256);
        assert!(d.warning.is_some());
        assert!(d.ready_for_screenshot());
    }

    #[test]
    fn interactive_results_advance_without_accepting_cancelled_refinements() {
        let ctx = egui::Context::default();
        let mut app = EditorApp::with_context(&ctx, vec![], false, None);
        let mut d = ready(&ctx);
        d.changed();
        d.changed();
        d.last_preview = Some(Instant::now());
        let (tx, rx) = mpsc::channel();
        d.receiver = Some(rx);
        let pixels = RgbaImage::from_pixel(64, 48, image::Rgba([90, 80, 70, 255]));
        tx.send(Ok(WorkerResult::Preview {
            revision: 1,
            side: 64,
            interactive: true,
            crop: d.settings.crop,
            preview: Box::new(
                PreparedPreview::cpu(pixels.clone(), false, &AtomicBool::new(false)).unwrap(),
            ),
            before: None,
        }))
        .unwrap();
        app.develop = Some(d);
        app.poll_develop(&ctx);
        let d = app.develop.as_mut().unwrap();
        assert_eq!(
            d.rendered_revision,
            Some(1),
            "continuous edits can show intermediate progress"
        );
        assert_eq!(d.preview.as_ref(), Some(&pixels));
        d.preview_job = Some(false);
        let (tx, rx) = mpsc::channel();
        d.receiver = Some(rx);
        d.changed();
        assert!(
            d.cancel.load(Ordering::Relaxed),
            "edits cancel an obsolete full-detail refinement"
        );
        tx.send(Err("RAW preview cancelled".into())).unwrap();
        app.poll_develop(&ctx);
        let d = app.develop.as_ref().unwrap();
        assert!(d.error.is_none());
        assert_eq!(d.preview.as_ref(), Some(&pixels));
    }
}
