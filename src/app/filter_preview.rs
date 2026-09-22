use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver},
};

use xuan::{document::Document, effects::Filter};

use super::{EditorApp, EffectEdit};

#[derive(Default)]
pub(super) struct FilterPreview {
    job: Option<FilterJob>,
    ready: Option<Filter>,
    pub applying: bool,
}

struct FilterJob {
    filter: Filter,
    receive: Receiver<Result<Document, String>>,
    cancel: Arc<AtomicBool>,
}

impl Drop for FilterJob {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

impl FilterPreview {
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
}

impl EditorApp {
    /// Keep one worker in flight and only publish results for the current settings.
    /// Returns true when Apply or an error has closed the effect dialog.
    pub(super) fn update_filter_preview(
        &mut self,
        edit: &mut EffectEdit,
        changed: bool,
        apply: bool,
    ) -> bool {
        let filter = edit.filter.as_ref().unwrap();
        let preview = &mut edit.filter_preview;
        preview.applying |= apply;
        let wanted = edit.preview || preview.applying;
        if wanted
            && !preview.applying
            && !self.editing_mask()
            && let Filter::MotionBlur { distance, angle } = filter
            && xuan::gpu::can_preview_motion_blur(&edit.original)
            && let Some(session) = self.session_mut().filter(|session| session.gpu.is_some())
        {
            // Slider edits only change GPU uniforms. Full-resolution pixels are
            // produced once, in the worker, when Apply is pressed.
            preview.job = None;
            preview.ready = None;
            let settings = Some([*distance, *angle]);
            if session.motion_blur_preview != settings || edit.refresh {
                session.document = edit.original.clone();
                session.motion_blur_preview = settings;
                session.invalidate();
                self.context.request_repaint();
            }
            edit.refresh = false;
            return false;
        }
        if changed || edit.refresh {
            preview.ready = None;
            if !wanted && let Some(session) = self.session_mut() {
                session.document = edit.original.clone();
                session.motion_blur_preview = None;
                session.invalidate();
            }
            edit.refresh = false;
        }

        if let Some(job) = &preview.job {
            if !wanted || job.filter != *filter {
                job.cancel.store(true, Ordering::Relaxed);
            }
            let result = match job.receive.try_recv() {
                Ok(result) => Some(result),
                Err(mpsc::TryRecvError::Empty) => None,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Some(Err("The filter worker stopped unexpectedly".into()))
                }
            };
            if let Some(result) = result {
                let job = preview.job.take().unwrap();
                if !job.cancel.load(Ordering::Relaxed) {
                    match result {
                        Ok(document) => {
                            if let Some(session) = self.session_mut() {
                                session.document = document;
                                session.motion_blur_preview = None;
                                session.invalidate();
                            }
                            preview.ready = Some(job.filter.clone());
                            self.context.request_repaint();
                        }
                        Err(error) => {
                            if let Some(session) = self.session_mut() {
                                session.history.cancel(&mut session.document);
                                session.motion_blur_preview = None;
                                session.invalidate();
                            }
                            self.error = Some(error);
                            self.dialog = None;
                            return true;
                        }
                    }
                }
            }
        }

        if preview.ready.as_ref() == Some(filter) && preview.applying {
            if let Some(session) = self.session_mut() {
                session.history.commit();
            }
            self.dialog = None;
            return true;
        }

        if wanted && preview.job.is_none() && preview.ready.as_ref() != Some(filter) {
            let mut document = edit.original.clone();
            let filter = filter.clone();
            let worker_filter = filter.clone();
            let mask_target = self.editing_mask();
            let cancel = Arc::new(AtomicBool::new(false));
            let worker_cancel = cancel.clone();
            let (send, receive) = mpsc::channel();
            let context = self.context.clone();
            let gpu = if preview.applying
                && !mask_target
                && matches!(filter, Filter::MotionBlur { .. })
            {
                self.session().and_then(|session| {
                    Some(
                        session
                            .gpu
                            .as_ref()?
                            .motion_blur_worker(document.active()?.pixels.as_ref()?),
                    )
                })
            } else {
                None
            };
            xuan::gpu::spawn(move || {
                let _cancel = xuan::gpu::cancellation(worker_cancel.clone());
                let result = if let Some(gpu) = gpu {
                    xuan::effects::apply_filter_with_gpu(
                        &mut document,
                        &worker_filter,
                        mask_target,
                        &worker_cancel,
                        &gpu,
                    )
                } else {
                    xuan::effects::apply_filter_cancellable(
                        &mut document,
                        &worker_filter,
                        mask_target,
                        &worker_cancel,
                    )
                }
                .map(|()| document)
                .map_err(|error| error.to_string());
                let _ = send.send(result);
                context.request_repaint();
            });
            preview.job = Some(FilterJob {
                filter,
                receive,
                cancel,
            });
            self.context.request_repaint();
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Dialog;
    use std::time::{Duration, Instant};

    fn setup() -> (EditorApp, EffectEdit) {
        let context = egui::Context::default();
        let mut app = EditorApp::with_context(&context, Vec::new(), false, None);
        app.dimensions = [16, 12];
        app.new_document();
        app.brush.color = [210, 80, 40, 255];
        app.command("fill_fg");
        app.start_filter(Filter::MotionBlur {
            distance: 15.0,
            angle: 0.0,
        });
        let edit = app.effect.take().unwrap();
        (app, edit)
    }

    fn pending(edit: &mut EffectEdit) -> mpsc::Sender<Result<Document, String>> {
        let (send, receive) = mpsc::channel();
        edit.filter_preview.job = Some(FilterJob {
            filter: edit.filter.clone().unwrap(),
            receive,
            cancel: Arc::new(AtomicBool::new(false)),
        });
        edit.refresh = false;
        send
    }

    fn wait(app: &mut EditorApp, edit: &mut EffectEdit) -> bool {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let finished = app.update_filter_preview(edit, false, false);
            if finished || !edit.filter_preview.busy() {
                return finished;
            }
            assert!(Instant::now() < deadline, "Filter worker did not finish");
            std::thread::yield_now();
        }
    }

    #[test]
    fn pending_filter_keeps_dialog_responsive_and_escape_cancels() {
        let (mut app, mut edit) = setup();
        let original = edit.original.clone();
        let revision = app.session().unwrap().history.revision;
        let send = pending(&mut edit);
        let cancel = edit.filter_preview.job.as_ref().unwrap().cancel.clone();
        app.effect = Some(edit);
        let context = app.context.clone();
        let _ = context.run(egui::RawInput::default(), |ctx| app.show(ctx));
        assert!(app.effect.as_ref().unwrap().filter_preview.busy());
        assert_eq!(
            app.session().unwrap().document.active().unwrap().pixels,
            original.active().unwrap().pixels
        );

        let _ = context.run(
            egui::RawInput {
                events: vec![egui::Event::Key {
                    key: egui::Key::Escape,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: egui::Modifiers::NONE,
                }],
                ..Default::default()
            },
            |ctx| app.show(ctx),
        );
        assert!(app.dialog.is_none());
        assert!(app.effect.is_none());
        assert!(cancel.load(Ordering::Relaxed));
        assert!(send.send(Ok(original.clone())).is_err());
        assert_eq!(app.session().unwrap().history.revision, revision);
        assert_eq!(
            app.session().unwrap().document.active().unwrap().pixels,
            original.active().unwrap().pixels
        );
    }

    #[test]
    fn slider_changes_cancel_and_coalesce_work_then_apply_reuses_preview() {
        let (mut app, mut edit) = setup();
        let revision = app.session().unwrap().history.revision;
        let send = pending(&mut edit);
        let cancel = edit.filter_preview.job.as_ref().unwrap().cancel.clone();
        for distance in [30.0, 50.0, 25.0] {
            edit.filter = Some(Filter::MotionBlur {
                distance,
                angle: 35.0,
            });
            assert!(!app.update_filter_preview(&mut edit, true, false));
            assert!(cancel.load(Ordering::Relaxed));
            // No replacement worker starts until the cancelled worker exits.
            assert!(Arc::ptr_eq(
                &cancel,
                &edit.filter_preview.job.as_ref().unwrap().cancel
            ));
        }
        let mut stale = edit.original.clone();
        stale.active_mut().unwrap().name = "Stale result".into();
        send.send(Ok(stale)).unwrap();
        assert!(!app.update_filter_preview(&mut edit, false, false));
        assert_ne!(
            app.session().unwrap().document.active().unwrap().name,
            "Stale result"
        );
        assert!(!wait(&mut app, &mut edit));
        let mut expected = edit.original.clone();
        xuan::effects::apply_filter(&mut expected, edit.filter.as_ref().unwrap(), false).unwrap();
        let pixels = app
            .session()
            .unwrap()
            .document
            .active()
            .unwrap()
            .pixels
            .clone()
            .unwrap();
        assert_eq!(
            &*pixels,
            &**expected.active().unwrap().pixels.as_ref().unwrap()
        );
        assert_eq!(app.session().unwrap().history.revision, revision);

        assert!(app.update_filter_preview(&mut edit, false, true));
        assert!(!edit.filter_preview.busy());
        assert!(Arc::ptr_eq(
            &pixels,
            app.session()
                .unwrap()
                .document
                .active()
                .unwrap()
                .pixels
                .as_ref()
                .unwrap()
        ));
        assert_eq!(app.session().unwrap().history.revision, revision + 1);
        app.command("undo");
        assert_eq!(
            app.session().unwrap().document.active().unwrap().pixels,
            edit.original.active().unwrap().pixels
        );
        app.command("redo");
        assert_eq!(
            app.session().unwrap().document.active().unwrap().pixels,
            Some(pixels)
        );
    }

    #[test]
    fn preview_off_discards_late_results_and_apply_waits_for_latest_settings() {
        let (mut app, mut edit) = setup();
        let send = pending(&mut edit);
        edit.preview = false;
        assert!(!app.update_filter_preview(&mut edit, true, false));
        let mut stale = edit.original.clone();
        stale.active_mut().unwrap().name = "Stale result".into();
        send.send(Ok(stale)).unwrap();
        assert!(!app.update_filter_preview(&mut edit, false, false));
        assert!(!edit.filter_preview.busy());
        assert_eq!(
            app.session().unwrap().document.active().unwrap().pixels,
            edit.original.active().unwrap().pixels
        );
        assert_ne!(
            app.session().unwrap().document.active().unwrap().name,
            "Stale result"
        );

        edit.filter = Some(Filter::MotionBlur {
            distance: 8.0,
            angle: -45.0,
        });
        let revision = app.session().unwrap().history.revision;
        let send = pending(&mut edit);
        assert!(!app.update_filter_preview(&mut edit, false, true));
        assert!(edit.filter_preview.applying);
        assert_eq!(app.session().unwrap().history.revision, revision);
        let mut expected = edit.original.clone();
        xuan::effects::apply_filter(&mut expected, edit.filter.as_ref().unwrap(), false).unwrap();
        send.send(Ok(expected.clone())).unwrap();
        assert!(app.update_filter_preview(&mut edit, false, false));
        assert_eq!(
            app.session().unwrap().document.active().unwrap().pixels,
            expected.active().unwrap().pixels
        );
        assert_eq!(app.session().unwrap().history.revision, revision + 1);
    }

    #[test]
    fn worker_failure_restores_original_without_committing() {
        let (mut app, mut edit) = setup();
        let revision = app.session().unwrap().history.revision;
        let send = pending(&mut edit);
        app.session_mut()
            .unwrap()
            .document
            .active_mut()
            .unwrap()
            .name = "Previous preview".into();
        drop(send);
        assert!(app.update_filter_preview(&mut edit, false, true));
        assert!(app.error.as_ref().unwrap().contains("worker stopped"));
        assert!(app.dialog.is_none());
        assert_eq!(app.session().unwrap().history.revision, revision);
        assert_eq!(
            app.session().unwrap().document.active().unwrap().name,
            edit.original.active().unwrap().name
        );
    }

    #[test]
    fn opening_filter_defers_processing_and_applies_without_preview() {
        let (mut app, mut edit) = setup();
        edit.preview = false;
        assert!(!app.update_filter_preview(&mut edit, false, false));
        assert!(!edit.filter_preview.busy());
        assert!(!app.update_filter_preview(&mut edit, false, true));
        assert_eq!(
            app.session().unwrap().document.active().unwrap().pixels,
            edit.original.active().unwrap().pixels
        );
        assert!(edit.filter_preview.busy());
        assert!(wait(&mut app, &mut edit));
        assert!(app.dialog != Some(Dialog::Effect));
        assert_ne!(
            app.session().unwrap().document.active().unwrap().pixels,
            edit.original.active().unwrap().pixels
        );
    }
}
