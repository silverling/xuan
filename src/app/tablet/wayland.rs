use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::{self, JoinHandle};

use anyhow::{Context, Result};
use calloop::{EventLoop, channel};
use calloop_wayland_source::WaylandSource;
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, Proxy, QueueHandle, WEnum, delegate_noop};
use wayland_protocols::wp::cursor_shape::v1::client::{
    wp_cursor_shape_device_v1 as cursor_device, wp_cursor_shape_manager_v1 as cursor_manager,
};
use wayland_protocols::wp::tablet::zv2::client::{
    zwp_tablet_manager_v2 as manager, zwp_tablet_pad_group_v2 as group,
    zwp_tablet_pad_ring_v2 as ring, zwp_tablet_pad_strip_v2 as strip, zwp_tablet_pad_v2 as pad,
    zwp_tablet_seat_v2 as seat, zwp_tablet_tool_v2 as tool, zwp_tablet_v2 as tablet,
};

use super::PenFrame;

pub(super) fn start(
    connection: Connection,
    surface: usize,
    context: egui::Context,
) -> Result<(Receiver<PenFrame>, channel::Sender<()>, JoinHandle<()>)> {
    let (sender, receiver) = mpsc::channel();
    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let thread = thread::Builder::new()
        .name("tablet-input".into())
        .spawn(move || {
            let setup = || -> Result<_> {
                let event_loop = EventLoop::<TabletState>::try_new()?;
                let signal = event_loop.get_signal();
                let (stop, stopped) = channel::channel();
                event_loop
                    .handle()
                    .insert_source(stopped, move |_, _, _| signal.stop())
                    .map_err(|error| anyhow::anyhow!("{error}"))?;
                let queue = connection.new_event_queue();
                let handle = queue.handle();
                WaylandSource::new(connection.clone(), queue)
                    .insert(event_loop.handle())
                    .map_err(|error| anyhow::anyhow!("{error}"))?;
                Ok((event_loop, handle, stop))
            };
            let (mut event_loop, handle, stop) = match setup() {
                Ok(setup) => setup,
                Err(error) => {
                    let _ = ready_tx.send(Err(error));
                    return;
                }
            };
            connection.display().get_registry(&handle, ());
            let mut state = TabletState {
                surface,
                sender,
                context: context.clone(),
                manager: None,
                cursor_manager: None,
                seats: HashMap::new(),
                tablets: HashMap::new(),
                tools: HashMap::new(),
            };
            if ready_tx.send(Ok(stop)).is_ok() {
                // Wake egui only for tablet input. No periodic redraw/polling is
                // needed while idle, and calloop coordinates reads with winit.
                if let Err(error) = event_loop.run(None, &mut state, |_| {}) {
                    eprintln!("Wayland tablet input stopped: {error}");
                }
            }
            state.clear_devices();
            for (_, seat) in state.seats.drain() {
                seat.destroy();
            }
            if let Some((_, manager)) = state.manager.take() {
                manager.destroy();
            }
            if let Some((_, manager)) = state.cursor_manager.take() {
                manager.destroy();
            }
            let _ = connection.flush();
            drop(state);
            // A receiver disconnect also releases any held pen buttons in egui.
            context.request_repaint();
        })?;
    match ready_rx
        .recv()
        .context("tablet input worker exited during setup")?
    {
        Ok(signal) => Ok((receiver, signal, thread)),
        Err(error) => {
            let _ = thread.join();
            Err(error)
        }
    }
}

struct Seat {
    proxy: wl_seat::WlSeat,
    tablet_seat: Option<seat::ZwpTabletSeatV2>,
}

impl Seat {
    fn destroy(self) {
        if let Some(tablet_seat) = self.tablet_seat {
            tablet_seat.destroy();
        }
        if self.proxy.version() >= 5 {
            self.proxy.release();
        }
    }
}

struct Tool {
    proxy: tool::ZwpTabletToolV2,
    seat: u32,
    tablet: Option<u32>,
    frame: PenFrame,
    was_over_surface: bool,
    changed: bool,
    cursor: Option<cursor_device::WpCursorShapeDeviceV1>,
}

struct TabletState {
    surface: usize,
    sender: Sender<PenFrame>,
    context: egui::Context,
    manager: Option<(u32, manager::ZwpTabletManagerV2)>,
    cursor_manager: Option<(u32, cursor_manager::WpCursorShapeManagerV1)>,
    seats: HashMap<u32, Seat>,
    tablets: HashMap<u32, (u32, tablet::ZwpTabletV2)>,
    tools: HashMap<u32, Tool>,
}

impl TabletState {
    fn bind_seats(&mut self, handle: &QueueHandle<Self>) {
        if let Some((_, manager)) = &self.manager {
            for (id, seat) in &mut self.seats {
                if seat.tablet_seat.is_none() {
                    seat.tablet_seat = Some(manager.get_tablet_seat(&seat.proxy, handle, *id));
                }
            }
        }
    }

    fn send(&self, frame: PenFrame) {
        let _ = self.sender.send(frame);
        self.context.request_repaint();
    }

    fn remove_tool(&mut self, id: u32) {
        if let Some(tool) = self.tools.remove(&id) {
            if tool.was_over_surface || tool.frame.proximity {
                self.send(PenFrame {
                    proximity: false,
                    buttons: [false; 3],
                    ..tool.frame
                });
            }
            tool.proxy.destroy();
            if let Some(cursor) = tool.cursor {
                cursor.destroy();
            }
        }
    }

    fn clear_devices(&mut self) {
        for id in self.tools.keys().copied().collect::<Vec<_>>() {
            self.remove_tool(id);
        }
        for (_, (_, tablet)) in self.tablets.drain() {
            tablet.destroy();
        }
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for TabletState {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        handle: &QueueHandle<Self>,
    ) {
        match event {
            wl_registry::Event::Global {
                name,
                interface,
                version,
            } => {
                match interface.as_str() {
                    "zwp_tablet_manager_v2" if state.manager.is_none() => {
                        state.manager = Some((name, registry.bind(name, 1, handle, ())));
                    }
                    "wl_seat" => {
                        state.seats.insert(
                            name,
                            Seat {
                                proxy: registry.bind(name, version.min(7), handle, ()),
                                tablet_seat: None,
                            },
                        );
                    }
                    "wp_cursor_shape_manager_v1" => {
                        let manager: cursor_manager::WpCursorShapeManagerV1 =
                            registry.bind(name, 1, handle, ());
                        for tool in state.tools.values_mut() {
                            tool.cursor = Some(manager.get_tablet_tool_v2(&tool.proxy, handle, ()));
                        }
                        state.cursor_manager = Some((name, manager));
                    }
                    _ => {}
                }
                state.bind_seats(handle);
            }
            wl_registry::Event::GlobalRemove { name } => {
                if state
                    .cursor_manager
                    .as_ref()
                    .is_some_and(|(id, _)| *id == name)
                {
                    for tool in state.tools.values_mut() {
                        if let Some(cursor) = tool.cursor.take() {
                            cursor.destroy();
                        }
                    }
                    state.cursor_manager.take().unwrap().1.destroy();
                }
                if let Some(seat) = state.seats.remove(&name) {
                    let tools: Vec<_> = state
                        .tools
                        .iter()
                        .filter(|(_, tool)| tool.seat == name)
                        .map(|(id, _)| *id)
                        .collect();
                    for id in tools {
                        state.remove_tool(id);
                    }
                    state.tablets.retain(|_, (seat, tablet)| {
                        if *seat == name {
                            tablet.destroy();
                            false
                        } else {
                            true
                        }
                    });
                    seat.destroy();
                }
                if state.manager.as_ref().is_some_and(|(id, _)| *id == name) {
                    state.clear_devices();
                    for seat in state.seats.values_mut() {
                        if let Some(tablet_seat) = seat.tablet_seat.take() {
                            tablet_seat.destroy();
                        }
                    }
                    state.manager.take().unwrap().1.destroy();
                }
            }
            _ => {}
        }
    }
}

impl Dispatch<seat::ZwpTabletSeatV2, u32> for TabletState {
    fn event(
        state: &mut Self,
        _: &seat::ZwpTabletSeatV2,
        event: seat::Event,
        seat: &u32,
        _: &Connection,
        handle: &QueueHandle<Self>,
    ) {
        match event {
            seat::Event::ToolAdded { id } => {
                let cursor = state
                    .cursor_manager
                    .as_ref()
                    .map(|(_, manager)| manager.get_tablet_tool_v2(&id, handle, ()));
                state.tools.insert(
                    id.id().protocol_id(),
                    Tool {
                        frame: PenFrame {
                            tool: id.id().protocol_id(),
                            position: None,
                            proximity: false,
                            buttons: [false; 3],
                            pressure: None,
                            tilt: None,
                            eraser: false,
                            mouse_handoff: false,
                        },
                        proxy: id,
                        seat: *seat,
                        tablet: None,
                        was_over_surface: false,
                        changed: false,
                        cursor,
                    },
                );
            }
            seat::Event::TabletAdded { id } => {
                state.tablets.insert(id.id().protocol_id(), (*seat, id));
            }
            // Express keys remain compositor/driver keyboard shortcuts.
            seat::Event::PadAdded { id } => id.destroy(),
            _ => {}
        }
    }

    wayland_client::event_created_child!(TabletState, seat::ZwpTabletSeatV2, [
        seat::EVT_TOOL_ADDED_OPCODE => (tool::ZwpTabletToolV2, ()),
        seat::EVT_TABLET_ADDED_OPCODE => (tablet::ZwpTabletV2, ()),
        seat::EVT_PAD_ADDED_OPCODE => (pad::ZwpTabletPadV2, ()),
    ]);
}

impl Dispatch<tool::ZwpTabletToolV2, ()> for TabletState {
    fn event(
        state: &mut Self,
        proxy: &tool::ZwpTabletToolV2,
        event: tool::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let id = proxy.id().protocol_id();
        if matches!(event, tool::Event::Removed) {
            state.remove_tool(id);
            return;
        }
        let Some(tool) = state.tools.get_mut(&id) else {
            return;
        };
        match event {
            tool::Event::ProximityIn {
                tablet,
                surface,
                serial,
            } => {
                tool.tablet = Some(tablet.id().protocol_id());
                tool.frame.proximity = surface.id().as_ptr() as usize == state.surface;
                tool.frame.position = None;
                tool.frame.pressure = None;
                tool.frame.tilt = None;
                tool.frame.buttons = [false; 3];
                if tool.frame.proximity
                    && let Some(cursor) = &tool.cursor
                {
                    cursor.set_shape(serial, cursor_device::Shape::Default);
                }
            }
            tool::Event::ProximityOut => {
                tool.frame.proximity = false;
                tool.frame.buttons = [false; 3];
            }
            tool::Event::Type { tool_type } => {
                tool.frame.eraser = tool_type == WEnum::Value(tool::Type::Eraser);
            }
            tool::Event::Tilt { tilt_x, tilt_y } => {
                tool.frame.tilt = Some([tilt_x as f32, tilt_y as f32]);
            }
            tool::Event::Pressure { pressure } => {
                tool.frame.pressure = Some(pressure.min(65535) as f32 / 65535.0);
            }
            tool::Event::Motion { x, y } => {
                tool.frame.position = Some(egui::pos2(x as f32, y as f32));
            }
            tool::Event::Down { .. } => tool.frame.buttons[0] = true,
            tool::Event::Up => tool.frame.buttons[0] = false,
            tool::Event::Button { button, state, .. } => {
                // Linux input-event-codes.h: BTN_STYLUS / BTN_STYLUS2.
                let index = match button {
                    0x14b => 1,
                    0x14c => 2,
                    _ => return,
                };
                tool.frame.buttons[index] = state == WEnum::Value(tool::ButtonState::Pressed);
            }
            tool::Event::Frame { .. } => {
                let send = tool.changed && (tool.frame.proximity || tool.was_over_surface);
                tool.was_over_surface = tool.frame.proximity;
                tool.changed = false;
                let frame = tool.frame;
                if send {
                    state.send(frame);
                }
                return;
            }
            _ => return,
        }
        tool.changed = true;
    }
}

impl Dispatch<tablet::ZwpTabletV2, ()> for TabletState {
    fn event(
        state: &mut Self,
        proxy: &tablet::ZwpTabletV2,
        event: tablet::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if matches!(event, tablet::Event::Removed) {
            let id = proxy.id().protocol_id();
            let tools: Vec<_> = state
                .tools
                .iter()
                .filter(|(_, tool)| tool.tablet == Some(id))
                .map(|(id, _)| *id)
                .collect();
            for tool in tools {
                // A tool can move to another tablet, so retain its protocol object.
                let tool = state.tools.get_mut(&tool).unwrap();
                tool.frame.proximity = false;
                tool.frame.buttons = [false; 3];
                let frame = tool.frame;
                tool.was_over_surface = false;
                state.send(frame);
            }
            if state.tablets.remove(&id).is_some() {
                proxy.destroy();
            }
        }
    }
}

// A pad can announce children before its destroy request reaches the server.
impl Dispatch<pad::ZwpTabletPadV2, ()> for TabletState {
    fn event(
        _: &mut Self,
        _: &pad::ZwpTabletPadV2,
        event: pad::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let pad::Event::Group { pad_group } = event {
            pad_group.destroy();
        }
    }
    wayland_client::event_created_child!(TabletState, pad::ZwpTabletPadV2, [
        pad::EVT_GROUP_OPCODE => (group::ZwpTabletPadGroupV2, ()),
    ]);
}

impl Dispatch<group::ZwpTabletPadGroupV2, ()> for TabletState {
    fn event(
        _: &mut Self,
        _: &group::ZwpTabletPadGroupV2,
        event: group::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        match event {
            group::Event::Ring { ring } => ring.destroy(),
            group::Event::Strip { strip } => strip.destroy(),
            _ => {}
        }
    }
    wayland_client::event_created_child!(TabletState, group::ZwpTabletPadGroupV2, [
        group::EVT_RING_OPCODE => (ring::ZwpTabletPadRingV2, ()),
        group::EVT_STRIP_OPCODE => (strip::ZwpTabletPadStripV2, ()),
    ]);
}

delegate_noop!(TabletState: ignore wl_seat::WlSeat);
delegate_noop!(TabletState: ignore manager::ZwpTabletManagerV2);
delegate_noop!(TabletState: ignore ring::ZwpTabletPadRingV2);
delegate_noop!(TabletState: ignore strip::ZwpTabletPadStripV2);
delegate_noop!(TabletState: ignore cursor_manager::WpCursorShapeManagerV1);
delegate_noop!(TabletState: ignore cursor_device::WpCursorShapeDeviceV1);

#[cfg(test)]
#[path = "wayland_tests.rs"]
mod tests;
