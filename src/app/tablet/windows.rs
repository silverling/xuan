//! Windows Ink pen packets, intercepted before winit converts them to touches.
use std::cell::Cell;
use std::sync::mpsc::{self, Receiver, Sender};

use anyhow::{Result, bail};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
use windows_sys::Win32::UI::Input::Pointer::{
    GetPointerPenInfo, GetPointerPenInfoHistory, GetPointerType, POINTER_FLAG_CANCELED,
    POINTER_FLAG_INCONTACT, POINTER_PEN_INFO, SkipPointerFrameMessages,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::*;

use super::PenFrame;

const SUBCLASS_ID: usize = 0x5855414e;

pub(super) struct WindowInput {
    window: HWND,
    // Stable allocation referenced by the window procedure until Drop removes it.
    _state: Box<State>,
}

struct State {
    sender: Sender<PenFrame>,
    context: egui::Context,
    last: Cell<Option<PenFrame>>,
}

impl WindowInput {
    pub(super) fn new(window: HWND, context: egui::Context) -> Result<(Self, Receiver<PenFrame>)> {
        let (sender, receiver) = mpsc::channel();
        let state = Box::new(State {
            sender,
            context,
            last: Cell::new(None),
        });
        // SAFETY: called on eframe's window thread. The allocation remains alive
        // until the subclass is removed by Drop or WM_NCDESTROY.
        if unsafe {
            SetWindowSubclass(
                window,
                Some(pen_proc),
                SUBCLASS_ID,
                &*state as *const State as usize,
            )
        } == 0
        {
            bail!("could not register the pen window procedure");
        }
        Ok((
            Self {
                window,
                _state: state,
            },
            receiver,
        ))
    }
}

impl Drop for WindowInput {
    fn drop(&mut self) {
        // SAFETY: eframe drops us on the same window thread, before the allocation
        // is freed. Removal is harmless if WM_NCDESTROY already removed it.
        unsafe {
            RemoveWindowSubclass(self.window, Some(pen_proc), SUBCLASS_ID);
        }
    }
}

unsafe extern "system" fn pen_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    data: usize,
) -> LRESULT {
    // SAFETY: SetWindowSubclass's reference data points at WindowInput's live Box.
    let state = unsafe { &*(data as *const State) };
    if matches!(
        message,
        WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN
    ) && unsafe { GetMessageExtraInfo() } as usize & 0xffffff00 != 0xff515700
        && let Some(frame) = state.last.take()
    {
        // Real mice can take over even while a stationary pen is in range.
        // The signature above identifies Windows' promoted pen/touch messages.
        let _ = state.sender.send(PenFrame {
            proximity: false,
            buttons: [false; 3],
            mouse_handoff: true,
            ..frame
        });
    }
    if message == WM_NCDESTROY {
        unsafe {
            RemoveWindowSubclass(window, Some(pen_proc), SUBCLASS_ID);
        }
    }
    if matches!(
        message,
        WM_POINTERDOWN
            | WM_POINTERUPDATE
            | WM_POINTERUP
            | WM_POINTERENTER
            | WM_POINTERLEAVE
            | WM_POINTERCAPTURECHANGED
    ) {
        let id = (wparam & 0xffff) as u32;
        let mut pointer_type = 0;
        let is_pen =
            unsafe { GetPointerType(id, &mut pointer_type) } != 0 && pointer_type == PT_PEN;
        if matches!(message, WM_POINTERLEAVE | WM_POINTERCAPTURECHANGED) {
            if state.last.get().is_some_and(|frame| frame.tool == id) {
                state.last.set(None);
            }
            // Capture can be cancelled after the OS has discarded the pen info.
            let _ = state.sender.send(PenFrame {
                tool: id,
                position: None,
                proximity: false,
                buttons: [false; 3],
                pressure: None,
                tilt: None,
                eraser: false,
                mouse_handoff: false,
            });
            state.context.request_repaint();
            if is_pen {
                return 0;
            }
        } else if is_pen {
            let mut current = POINTER_PEN_INFO::default();
            if unsafe { GetPointerPenInfo(id, &mut current) } != 0 {
                // Windows coalesces high-rate pen input. Replay its history in
                // chronological order so fast curves and pressure changes survive.
                let mut count = 0;
                unsafe {
                    GetPointerPenInfoHistory(id, &mut count, std::ptr::null_mut());
                }
                let mut packets = if count > 0 && count <= 4096 {
                    vec![POINTER_PEN_INFO::default(); count as usize]
                } else {
                    Vec::new()
                };
                if packets.is_empty()
                    || unsafe { GetPointerPenInfoHistory(id, &mut count, packets.as_mut_ptr()) }
                        == 0
                {
                    packets.clear();
                    packets.push(current);
                } else {
                    packets.truncate(count as usize);
                }
                for packet in packets.into_iter().rev() {
                    let info = packet.pointerInfo;
                    let mut position = info.ptPixelLocation;
                    if unsafe { ScreenToClient(window, &mut position) } == 0 {
                        continue;
                    }
                    let cancelled = info.pointerFlags & POINTER_FLAG_CANCELED != 0;
                    let contact = info.pointerFlags & POINTER_FLAG_INCONTACT != 0 && !cancelled;
                    let frame = PenFrame {
                        tool: id,
                        position: Some(egui::pos2(position.x as f32, position.y as f32)),
                        proximity: !cancelled,
                        buttons: [contact, packet.penFlags & PEN_FLAG_BARREL != 0, false],
                        pressure: (packet.penMask & PEN_MASK_PRESSURE != 0)
                            .then_some(packet.pressure.min(1024) as f32 / 1024.0),
                        tilt: (packet.penMask & (PEN_MASK_TILT_X | PEN_MASK_TILT_Y) != 0)
                            .then_some([packet.tiltX as f32, packet.tiltY as f32]),
                        eraser: packet.penFlags & (PEN_FLAG_ERASER | PEN_FLAG_INVERTED) != 0,
                        mouse_handoff: false,
                    };
                    state.last.set(Some(frame));
                    let _ = state.sender.send(frame);
                }
                unsafe {
                    SkipPointerFrameMessages(id);
                }
                state.context.request_repaint();
                // Consuming pen messages prevents duplicate synthetic mouse/touch
                // clicks. Touchscreens and ordinary mice still go through winit.
                return 0;
            }
        }
    }
    unsafe { DefSubclassProc(window, message, wparam, lparam) }
}
