//! Normalize native pen input into egui pointer events and pressure samples.

#[cfg(target_os = "linux")]
mod wayland;
#[cfg(windows)]
mod windows;
#[cfg(target_os = "linux")]
mod x11;

use std::sync::mpsc::{Receiver, TryRecvError};
#[cfg(target_os = "linux")]
use std::thread::JoinHandle;

use egui::{Event, PointerButton, Pos2};
#[cfg(target_os = "linux")]
use raw_window_handle::RawDisplayHandle;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle, RawWindowHandle};

const BUTTONS: [PointerButton; 3] = [
    PointerButton::Primary,
    PointerButton::Secondary,
    PointerButton::Middle,
];

/// A complete native pen frame, in window-local coordinates. Wayland supplies
/// logical pixels; Windows and X11 supply physical pixels.
#[derive(Clone, Copy, Debug)]
struct PenFrame {
    tool: u32,
    position: Option<Pos2>,
    proximity: bool,
    buttons: [bool; 3],
    pressure: Option<f32>,
    tilt: Option<[f32; 2]>,
    eraser: bool,
    mouse_handoff: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Phase {
    Hover,
    Down,
    Move,
    Up,
    Leave,
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Sample {
    pub position: Pos2,
    pub pressure: Option<f32>,
    pub tilt: Option<[f32; 2]>,
    pub eraser: bool,
    pub phase: Phase,
}

#[cfg(target_os = "linux")]
struct Worker {
    stop: calloop::channel::Sender<()>,
    thread: Option<JoinHandle<()>>,
}

#[cfg(target_os = "linux")]
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

pub(super) struct TabletInput {
    receiver: Receiver<PenFrame>,
    #[cfg(target_os = "linux")]
    _worker: Worker,
    #[cfg(windows)]
    _window: windows::WindowInput,
    physical_coordinates: bool,
    pointer: PenPointer,
}

impl TabletInput {
    pub(super) fn new(cc: &eframe::CreationContext<'_>) -> Option<Self> {
        let display = cc.display_handle().ok()?.as_raw();
        let window = cc.window_handle().ok()?.as_raw();
        #[cfg(target_os = "linux")]
        {
            let (result, physical_coordinates) = match (display, window) {
                (RawDisplayHandle::Wayland(display), RawWindowHandle::Wayland(window)) => {
                    // SAFETY: eframe keeps its window/display alive while EditorApp is
                    // alive. on_exit drops this bridge; Worker::drop joins the thread
                    // before the borrowed display is released, including on unwind.
                    let backend = unsafe {
                        wayland_backend::client::Backend::from_foreign_display(
                            display.display.as_ptr().cast(),
                        )
                    };
                    (
                        wayland::start(
                            wayland_client::Connection::from_backend(backend),
                            window.surface.as_ptr() as usize,
                            cc.egui_ctx.clone(),
                        ),
                        false,
                    )
                }
                (_, RawWindowHandle::Xlib(window)) => {
                    (x11::start(window.window as u32, cc.egui_ctx.clone()), true)
                }
                (_, RawWindowHandle::Xcb(window)) => {
                    (x11::start(window.window.get(), cc.egui_ctx.clone()), true)
                }
                _ => return None,
            };
            match result {
                Ok((receiver, stop, thread)) => Some(Self {
                    receiver,
                    _worker: Worker {
                        stop,
                        thread: Some(thread),
                    },
                    physical_coordinates,
                    pointer: PenPointer::default(),
                }),
                Err(error) => {
                    eprintln!("Could not initialize tablet input: {error}");
                    None
                }
            }
        }
        #[cfg(windows)]
        {
            let _ = display;
            let RawWindowHandle::Win32(window) = window else {
                return None;
            };
            match windows::WindowInput::new(window.hwnd.get() as _, cc.egui_ctx.clone()) {
                Ok((window, receiver)) => Some(Self {
                    receiver,
                    _window: window,
                    physical_coordinates: true,
                    pointer: PenPointer::default(),
                }),
                Err(error) => {
                    eprintln!("Could not initialize Windows pen input: {error}");
                    None
                }
            }
        }
    }

    pub(super) fn update(
        &mut self,
        ctx: &egui::Context,
        input: &mut egui::RawInput,
    ) -> Vec<Sample> {
        let mut frames = Vec::new();
        let disconnected = loop {
            match self.receiver.try_recv() {
                Ok(frame) => frames.push(frame),
                Err(TryRecvError::Empty) => break false,
                Err(TryRecvError::Disconnected) => break true,
            }
        };
        // Wayland coordinates already include monitor scaling; X11 and Windows
        // coordinates are physical pixels. Both also need egui's UI zoom.
        let scale = if self.physical_coordinates {
            ctx.pixels_per_point()
        } else {
            ctx.zoom_factor()
        };
        self.pointer.update(input, &frames, scale, disconnected);
        std::mem::take(&mut self.pointer.samples)
    }

    pub(super) fn sample(&self) -> Option<Sample> {
        self.pointer.sample
    }
}

#[derive(Default)]
struct PenPointer {
    tool: Option<u32>,
    position: Option<Pos2>,
    buttons: [bool; 3],
    wait_for_lift: bool,
    sample: Option<Sample>,
    samples: Vec<Sample>,
}

impl PenPointer {
    fn update(
        &mut self,
        input: &mut egui::RawInput,
        frames: &[PenFrame],
        zoom: f32,
        disconnected: bool,
    ) {
        self.samples.clear();
        if frames.last().is_some_and(|frame| frame.mouse_handoff) {
            let mouse_events = std::mem::take(&mut input.events);
            self.release(input);
            input.events.extend(mouse_events);
            self.wait_for_lift = false;
            return;
        }
        if self.tool.is_some() || frames.iter().any(|frame| frame.proximity) {
            // Do not mix legacy pointer emulation with the native pen stream.
            // Keyboard, scroll, and touchscreen events remain available.
            input.events.retain(|event| {
                !matches!(
                    event,
                    Event::PointerMoved(_)
                        | Event::PointerButton { .. }
                        | Event::PointerGone
                        | Event::MouseMoved(_)
                )
            });
        }
        let lost_focus = input.events.contains(&Event::WindowFocused(false));
        if lost_focus || disconnected {
            self.release(input);
            self.wait_for_lift = lost_focus;
            return;
        }
        for frame in frames {
            if frame.mouse_handoff {
                continue;
            }
            if self.wait_for_lift {
                if frame.proximity && frame.buttons.iter().any(|pressed| *pressed) {
                    continue;
                }
                self.wait_for_lift = false;
            }
            if !frame.proximity {
                if self.tool == Some(frame.tool) {
                    self.release(input);
                }
                continue;
            }
            if self.tool != Some(frame.tool) {
                // A second pen must not steal an ongoing stroke.
                if self.buttons.iter().any(|pressed| *pressed) {
                    continue;
                }
                self.release(input);
                self.tool = Some(frame.tool);
            }
            let Some(position) = frame.position.filter(|pos| pos.is_finite()) else {
                continue;
            };
            let position = position / zoom;
            let phase = match (self.buttons[0], frame.buttons[0]) {
                (false, true) => Phase::Down,
                (true, true) => Phase::Move,
                (true, false) => Phase::Up,
                (false, false) => Phase::Hover,
            };
            let sample = Sample {
                position,
                pressure: frame
                    .pressure
                    .filter(|value| value.is_finite())
                    .map(|value| value.clamp(0.0, 1.0)),
                tilt: frame
                    .tilt
                    .filter(|tilt| tilt.iter().all(|value| value.is_finite()))
                    .map(|tilt| tilt.map(|value| value.clamp(-90.0, 90.0))),
                eraser: frame.eraser,
                phase,
            };
            self.sample = Some(sample);
            self.samples.push(sample);
            self.position = Some(position);
            input.events.push(Event::PointerMoved(position));
            for (index, button) in BUTTONS.into_iter().enumerate() {
                if self.buttons[index] != frame.buttons[index] {
                    self.buttons[index] = frame.buttons[index];
                    input.events.push(Event::PointerButton {
                        pos: position,
                        button,
                        pressed: frame.buttons[index],
                        modifiers: input.modifiers,
                    });
                }
            }
        }
    }

    fn release(&mut self, input: &mut egui::RawInput) {
        if let Some(sample) = self.sample.take() {
            self.samples.push(Sample {
                phase: Phase::Leave,
                ..sample
            });
        }
        if let Some(position) = self.position.take() {
            for (index, button) in BUTTONS.into_iter().enumerate() {
                if self.buttons[index] {
                    input.events.push(Event::PointerButton {
                        pos: position,
                        button,
                        pressed: false,
                        modifiers: input.modifiers,
                    });
                }
            }
            input.events.push(Event::PointerGone);
        }
        self.buttons = [false; 3];
        self.tool = None;
    }
}

#[cfg(test)]
mod tests;
