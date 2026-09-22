//! XInput2 valuators use driver-advertised pressure ranges, not vendor IDs.
use std::collections::HashMap;
use std::os::fd::AsFd;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use anyhow::Result;
use calloop::{EventLoop, Interest, Mode, PostAction, channel, generic::Generic};
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xinput::{
    self, ConnectionExt as _, DeviceClassData, DeviceType, EventMask, XIEventMask,
};
use x11rb::protocol::xproto::ConnectionExt as _;
use x11rb::rust_connection::RustConnection;

use super::PenFrame;

pub(super) fn start(
    window: u32,
    context: egui::Context,
) -> Result<(Receiver<PenFrame>, channel::Sender<()>, JoinHandle<()>)> {
    let (sender, receiver) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let thread = thread::Builder::new()
        .name("tablet-xinput".into())
        .spawn(move || {
            let setup = || -> Result<_> {
                let (connection, screen) = RustConnection::connect(None)?;
                connection.xinput_xi_query_version(2, 0)?.reply()?;
                let pressure = connection
                    .intern_atom(false, b"Abs Pressure")?
                    .reply()?
                    .atom;
                let tilt_x = connection.intern_atom(false, b"Abs Tilt X")?.reply()?.atom;
                let tilt_y = connection.intern_atom(false, b"Abs Tilt Y")?.reply()?.atom;
                let mut state = State {
                    connection,
                    pressure,
                    tilt: [tilt_x, tilt_y],
                    tools: HashMap::new(),
                    sender,
                    context: context.clone(),
                };
                state.refresh()?;
                // One master event stream avoids duplicate slave/master packets.
                state
                    .connection
                    .xinput_xi_select_events(
                        window,
                        &[EventMask {
                            deviceid: xinput::Device::ALL_MASTER.into(),
                            mask: vec![
                                XIEventMask::MOTION
                                    | XIEventMask::BUTTON_PRESS
                                    | XIEventMask::BUTTON_RELEASE
                                    | XIEventMask::LEAVE
                                    | XIEventMask::FOCUS_OUT
                                    | XIEventMask::DEVICE_CHANGED,
                            ],
                        }],
                    )?
                    .check()?;
                let root = state.connection.setup().roots[screen].root;
                state
                    .connection
                    .xinput_xi_select_events(
                        root,
                        &[EventMask {
                            deviceid: xinput::Device::ALL.into(),
                            mask: vec![XIEventMask::HIERARCHY],
                        }],
                    )?
                    .check()?;
                let event_loop = EventLoop::<State>::try_new()?;
                let signal = event_loop.get_signal();
                let (stop, stopped) = channel::channel();
                event_loop
                    .handle()
                    .insert_source(stopped, move |_, _, _| signal.stop())
                    .map_err(|error| anyhow::anyhow!("{error}"))?;
                let fd = state.connection.stream().as_fd().try_clone_to_owned()?;
                event_loop
                    .handle()
                    .insert_source(
                        Generic::new(fd, Interest::READ, Mode::Level),
                        |_, _, state| {
                            state.poll().map_err(std::io::Error::other)?;
                            Ok(PostAction::Continue)
                        },
                    )
                    .map_err(|error| anyhow::anyhow!("{error}"))?;
                Ok((state, event_loop, stop))
            };
            match setup() {
                Ok((mut state, mut event_loop, stop)) => {
                    if ready_tx.send(Ok(stop)).is_ok()
                        && let Err(error) = state.poll().and_then(|()| {
                            event_loop.run(None, &mut state, |_| {}).map_err(Into::into)
                        })
                    {
                        eprintln!("XInput tablet input stopped: {error}");
                    }
                }
                Err(error) => {
                    let _ = ready_tx.send(Err(error));
                }
            }
            context.request_repaint();
        })?;
    match ready_rx.recv()? {
        Ok(stop) => Ok((receiver, stop, thread)),
        Err(error) => {
            let _ = thread.join();
            Err(error)
        }
    }
}

struct Axis {
    number: u16,
    min: f64,
    max: f64,
}
struct Tool {
    pressure: Axis,
    tilt: [Option<Axis>; 2],
    frame: PenFrame,
}
struct State {
    connection: RustConnection,
    pressure: u32,
    tilt: [u32; 2],
    tools: HashMap<u16, Tool>,
    sender: Sender<PenFrame>,
    context: egui::Context,
}

fn fixed(value: xinput::Fp3232) -> f64 {
    value.integral as f64 + value.frac as f64 / 4294967296.0
}

fn valuator(mask: &[u32], values: &[xinput::Fp3232], number: u16) -> Option<f64> {
    let number = number as usize;
    let word = number / 32;
    let bit = number % 32;
    if mask.get(word)? & (1 << bit) == 0 {
        return None;
    }
    let preceding = mask[..word]
        .iter()
        .map(|word| word.count_ones() as usize)
        .sum::<usize>()
        + (mask[word] & ((1_u32 << bit) - 1)).count_ones() as usize;
    values.get(preceding).copied().map(fixed)
}

impl State {
    fn refresh(&mut self) -> Result<()> {
        let mut tools = HashMap::new();
        for device in self
            .connection
            .xinput_xi_query_device(xinput::Device::ALL)?
            .reply()?
            .infos
        {
            if !device.enabled
                || device.type_ == DeviceType::MASTER_POINTER
                || device
                    .classes
                    .iter()
                    .any(|class| matches!(class.data, DeviceClassData::Touch(_)))
            {
                continue;
            }
            let axis = |label| {
                device.classes.iter().find_map(|class| match &class.data {
                    DeviceClassData::Valuator(axis)
                        if axis.label == label && fixed(axis.max) > fixed(axis.min) =>
                    {
                        Some(Axis {
                            number: axis.number,
                            min: fixed(axis.min),
                            max: fixed(axis.max),
                        })
                    }
                    _ => None,
                })
            };
            let Some(pressure) = axis(self.pressure) else {
                continue;
            };
            let frame = self
                .tools
                .remove(&device.deviceid)
                .map(|tool| tool.frame)
                .unwrap_or(PenFrame {
                    tool: device.deviceid as u32,
                    position: None,
                    proximity: false,
                    buttons: [false; 3],
                    pressure: None,
                    tilt: None,
                    eraser: String::from_utf8_lossy(&device.name)
                        .to_lowercase()
                        .contains("eraser"),
                    mouse_handoff: false,
                });
            tools.insert(
                device.deviceid,
                Tool {
                    pressure,
                    tilt: self.tilt.map(axis),
                    frame,
                },
            );
        }
        for tool in self.tools.values() {
            if tool.frame.proximity {
                self.send(PenFrame {
                    proximity: false,
                    buttons: [false; 3],
                    ..tool.frame
                });
            }
        }
        self.tools = tools;
        Ok(())
    }

    fn send(&self, frame: PenFrame) {
        let _ = self.sender.send(frame);
        self.context.request_repaint();
    }

    fn motion(&mut self, event: xinput::ButtonPressEvent, button: Option<bool>) {
        let Some(tool) = self.tools.get_mut(&event.sourceid) else {
            let frames: Vec<_> = self
                .tools
                .values_mut()
                .filter(|tool| tool.frame.proximity)
                .map(|tool| {
                    tool.frame.proximity = false;
                    tool.frame.buttons = [false; 3];
                    PenFrame {
                        mouse_handoff: true,
                        ..tool.frame
                    }
                })
                .collect();
            for frame in frames {
                self.send(frame);
            }
            return;
        };
        tool.frame.proximity = true;
        tool.frame.position = Some(egui::pos2(
            event.event_x as f32 / 65536.0,
            event.event_y as f32 / 65536.0,
        ));
        // XInput buttons 1/3/2 are primary/secondary/middle. The mask is the
        // state before a button event, so apply its transition explicitly.
        for (index, number) in [1, 3, 2].into_iter().enumerate() {
            tool.frame.buttons[index] = event
                .button_mask
                .first()
                .is_some_and(|mask| mask & (1 << number) != 0);
            if event.detail == number
                && let Some(pressed) = button
            {
                tool.frame.buttons[index] = pressed;
            }
        }
        if let Some(value) = valuator(
            &event.valuator_mask,
            &event.axisvalues,
            tool.pressure.number,
        ) {
            tool.frame.pressure = Some(
                ((value - tool.pressure.min) / (tool.pressure.max - tool.pressure.min))
                    .clamp(0.0, 1.0) as f32,
            );
        }
        for (index, axis) in tool.tilt.iter().enumerate() {
            if let Some(axis) = axis
                && let Some(value) = valuator(&event.valuator_mask, &event.axisvalues, axis.number)
            {
                // XInput tilt axes span [-1, 1] after calibration; convert to
                // degrees like Wayland and Windows Ink. Zero is upright.
                let degrees = if value >= 0.0 {
                    value / axis.max.max(1.0)
                } else {
                    -value / axis.min.min(-1.0)
                } * 90.0;
                tool.frame.tilt.get_or_insert([0.0; 2])[index] = degrees.clamp(-90.0, 90.0) as f32;
            }
        }
        let frame = tool.frame;
        self.send(frame);
    }

    fn poll(&mut self) -> Result<()> {
        while let Some(event) = self.connection.poll_for_event()? {
            match event {
                Event::XinputMotion(event) => self.motion(event, None),
                Event::XinputButtonPress(event) => self.motion(event, Some(true)),
                Event::XinputButtonRelease(event) => self.motion(event, Some(false)),
                Event::XinputLeave(event) => {
                    if let Some(tool) = self.tools.get_mut(&event.sourceid) {
                        tool.frame.proximity = false;
                        tool.frame.buttons = [false; 3];
                        let frame = tool.frame;
                        self.send(frame);
                    }
                }
                Event::XinputFocusOut(_) => {
                    let frames: Vec<_> = self
                        .tools
                        .values_mut()
                        .filter(|tool| tool.frame.proximity)
                        .map(|tool| {
                            tool.frame.proximity = false;
                            tool.frame.buttons = [false; 3];
                            tool.frame
                        })
                        .collect();
                    for frame in frames {
                        self.send(frame);
                    }
                }
                Event::XinputHierarchy(_) | Event::XinputDeviceChanged(_) => self.refresh()?,
                _ => {}
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sparse_valuator_masks_and_fractional_signed_values_are_decoded() {
        let values = [
            xinput::Fp3232 {
                integral: -2,
                frac: 1 << 31,
            },
            xinput::Fp3232 {
                integral: 2048,
                frac: 0,
            },
            xinput::Fp3232 {
                integral: 9,
                frac: 0,
            },
        ];
        assert_eq!(valuator(&[0b1001, 0b10], &values, 0), Some(-1.5));
        assert_eq!(valuator(&[0b1001, 0b10], &values, 3), Some(2048.0));
        assert_eq!(valuator(&[0b1001, 0b10], &values, 33), Some(9.0));
        assert_eq!(valuator(&[0b1001, 0b10], &values, 2), None);
    }
}
