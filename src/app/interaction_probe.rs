//! Opt-in semantic interaction adapter using real application state and egui buttons.
//! This module is compiled only with the development feature `interaction-probe`.
use std::io::{self, BufRead, Read, Write};

use anyhow::{Context, Result, bail, ensure};
use serde::Deserialize;

use super::{Dialog, EditorApp};
use egui::{Event, Modifiers, PointerButton, Pos2, Vec2};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    protocol: u8,
    id: u64,
    event: Input,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Input {
    New,
    Rename,
    Undo,
    Redo,
    OpenSize {
        kind: SizeKind,
    },
    SetSize {
        width: u32,
        height: u32,
        resolution: f32,
    },
    Apply,
    Cancel,
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum SizeKind {
    Canvas,
    Image,
}

/// Read JSONL requests and write one observed state per event. No window is created.
pub fn run() -> Result<()> {
    let context = egui::Context::default();
    let mut app = EditorApp::with_context(&context, Vec::new(), false, None);
    let input = io::stdin();
    let mut input = input.lock();
    let output = io::stdout();
    let mut output = output.lock();
    loop {
        // Bound each line even when the adapter is invoked without the runner.
        let mut line = Vec::new();
        let bytes = std::io::Read::by_ref(&mut input)
            .take(65_537)
            .read_until(b'\n', &mut line)?;
        if bytes == 0 {
            break;
        }
        ensure!(bytes <= 65_536, "request exceeds 64 KiB");
        let request: Request = serde_json::from_slice(&line).context("invalid request")?;
        ensure!(request.protocol == 1, "expected protocol 1");
        dispatch(&context, &mut app, request.event)?;
        let session = app.session().context("create a document before editing")?;
        let layer = session
            .document
            .layers
            .first()
            .context("missing fixture layer")?;
        serde_json::to_writer(
            &mut output,
            &serde_json::json!({
                "protocol": 1,
                "id": request.id,
                "state": {
                    "width": session.document.width,
                    "height": session.document.height,
                    "resolution": session.document.resolution,
                    "undo_count": session.history.names().count(),
                    "redo_name": session.history.redo_name(),
                    "dirty": session.history.dirty(),
                    "dialog_open": app.dialog.is_some(),
                    "layer_name": layer.name,
                }
            }),
        )?;
        writeln!(output)?;
        output.flush()?;
    }
    Ok(())
}

fn dispatch(context: &egui::Context, app: &mut EditorApp, event: Input) -> Result<()> {
    match event {
        Input::New => {
            ensure!(app.sessions.is_empty(), "a fixture document already exists");
            app.dimensions = [8, 6];
            app.new_document();
        }
        Input::Rename => {
            ensure!(app.session().is_some(), "create a document first");
            app.edit("Rename", |document| {
                document
                    .layers
                    .first_mut()
                    .context("missing fixture layer")?
                    .name = "Renamed".into();
                Ok(())
            });
        }
        Input::Undo => app.command("undo"),
        Input::Redo => app.command("redo"),
        Input::OpenSize { kind } => {
            ensure!(app.dialog.is_none(), "a dialog is already open");
            ensure!(app.session().is_some(), "create a document first");
            app.command(match kind {
                SizeKind::Canvas => "canvas_size",
                SizeKind::Image => "image_size",
            });
        }
        Input::SetSize {
            width,
            height,
            resolution,
        } => {
            ensure!(
                matches!(app.dialog, Some(Dialog::CanvasSize | Dialog::ImageSize)),
                "open a size dialog first"
            );
            xuan::document::validate_size(width, height)?;
            ensure!(
                resolution.is_finite() && (1.0..=9600.0).contains(&resolution),
                "resolution must be finite and between 1 and 9600"
            );
            app.dimensions = [width, height];
            app.resolution = resolution;
        }
        Input::Apply => click(context, app, "Apply")?,
        Input::Cancel => click(context, app, "Cancel")?,
    }
    if let Some(error) = &app.error {
        bail!("application error: {error}");
    }
    Ok(())
}

fn frame(context: &egui::Context, app: &mut EditorApp, events: Vec<Event>) -> egui::FullOutput {
    context.run(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                Pos2::ZERO,
                Vec2::new(1280.0, 860.0),
            )),
            events,
            time: Some(app.frames as f64 / 60.0),
            ..Default::default()
        },
        |context| app.show(context),
    )
}

fn click(context: &egui::Context, app: &mut EditorApp, label: &str) -> Result<()> {
    ensure!(
        matches!(app.dialog, Some(Dialog::CanvasSize | Dialog::ImageSize)),
        "open a size dialog before clicking"
    );
    frame(context, app, Vec::new());
    let output = frame(context, app, Vec::new());
    let position = output
        .shapes
        .iter()
        .find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.text() == label => {
                Some(text.pos + egui::vec2(5.0, 5.0))
            }
            _ => None,
        })
        .context("button label was not rendered")?;
    for pressed in [true, false] {
        frame(
            context,
            app,
            vec![
                Event::PointerMoved(position),
                Event::PointerButton {
                    pos: position,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
    }
    ensure!(app.dialog.is_none(), "button did not close the dialog");
    Ok(())
}
