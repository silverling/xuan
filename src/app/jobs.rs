use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc::{self, Receiver},
};

use anyhow::Result;
use uuid::Uuid;
use xuan::document::Document;

use super::EditorApp;

pub(super) struct Job {
    pub name: String,
    target: Uuid,
    receive: Receiver<Result<Document, String>>,
    pub cancel: Arc<AtomicBool>,
}

impl EditorApp {
    pub(super) fn start_job(
        &mut self,
        name: &str,
        operation: impl FnOnce(&mut Document, &AtomicBool) -> Result<()> + Send + 'static,
    ) {
        if self.job.is_some() {
            return;
        }
        let Some(session) = self.session_mut() else {
            return;
        };
        session.history.begin(name, &session.document);
        let mut document = session.document.clone();
        let target = document.id;
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (send, receive) = mpsc::channel();
        let context = self.context.clone();
        std::thread::spawn(move || {
            let result = operation(&mut document, &worker_cancel)
                .map(|()| document)
                .map_err(|e| e.to_string());
            let _ = send.send(result);
            context.request_repaint();
        });
        self.job = Some(Job {
            name: name.into(),
            target,
            receive,
            cancel,
        });
    }

    pub(super) fn poll_job(&mut self) {
        let Some(job) = &self.job else {
            return;
        };
        let result = match job.receive.try_recv() {
            Ok(result) => result,
            Err(mpsc::TryRecvError::Empty) => return,
            Err(mpsc::TryRecvError::Disconnected) => {
                Err("The editing worker stopped unexpectedly".into())
            }
        };
        let job = self.job.take().unwrap();
        if let Some(session) = self
            .sessions
            .iter_mut()
            .find(|s| s.document.id == job.target)
        {
            match result {
                Ok(document) if !job.cancel.load(Ordering::Relaxed) => {
                    session.document = document;
                    session.history.commit();
                    self.status = job.name;
                }
                result => {
                    session.history.cancel(&mut session.document);
                    if job.cancel.load(Ordering::Relaxed) {
                        self.status = "Cancelled".into();
                    } else if let Err(error) = result {
                        self.error = Some(error);
                    }
                }
            }
            session.invalidate();
        }
    }
}
