//! Exercise the actual wire protocol and worker using an in-process compositor.
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::time::Duration;

use super::*;
use wayland_protocols::wp::tablet::zv2::server::{
    zwp_tablet_manager_v2::Request as ManagerRequest,
    zwp_tablet_manager_v2::ZwpTabletManagerV2,
    zwp_tablet_seat_v2::ZwpTabletSeatV2,
    zwp_tablet_tool_v2::{self as server_tool, ZwpTabletToolV2},
    zwp_tablet_v2::ZwpTabletV2,
};
use wayland_server::protocol::{wl_compositor, wl_region, wl_seat, wl_surface};
use wayland_server::{Client, DataInit, Display, DisplayHandle, GlobalDispatch, New};

enum ServerEvent {
    Surface(wl_surface::WlSurface),
    Tablet(ZwpTabletV2, ZwpTabletToolV2),
}

struct ServerState {
    events: Sender<ServerEvent>,
}

macro_rules! global {
    ($interface:ty) => {
        impl GlobalDispatch<$interface, ()> for ServerState {
            fn bind(
                _: &mut Self,
                _: &DisplayHandle,
                _: &Client,
                resource: New<$interface>,
                _: &(),
                init: &mut DataInit<'_, Self>,
            ) {
                init.init(resource, ());
            }
        }
    };
}

global!(wl_compositor::WlCompositor);
global!(wl_seat::WlSeat);
global!(ZwpTabletManagerV2);

impl wayland_server::Dispatch<wl_compositor::WlCompositor, ()> for ServerState {
    fn request(
        state: &mut Self,
        _: &Client,
        _: &wl_compositor::WlCompositor,
        request: wl_compositor::Request,
        _: &(),
        _: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        match request {
            wl_compositor::Request::CreateSurface { id } => {
                state
                    .events
                    .send(ServerEvent::Surface(init.init(id, ())))
                    .unwrap();
            }
            wl_compositor::Request::CreateRegion { id } => {
                init.init(id, ());
            }
            _ => {}
        }
    }
}

impl wayland_server::Dispatch<ZwpTabletManagerV2, ()> for ServerState {
    fn request(
        state: &mut Self,
        client: &Client,
        _: &ZwpTabletManagerV2,
        request: ManagerRequest,
        _: &(),
        display: &DisplayHandle,
        init: &mut DataInit<'_, Self>,
    ) {
        if let ManagerRequest::GetTabletSeat { tablet_seat, .. } = request {
            let seat = init.init(tablet_seat, ());
            let tablet = client
                .create_resource::<ZwpTabletV2, (), Self>(display, 1, ())
                .unwrap();
            let tool = client
                .create_resource::<ZwpTabletToolV2, (), Self>(display, 1, ())
                .unwrap();
            seat.tablet_added(&tablet);
            tablet.name("Test tablet".into());
            tablet.done();
            seat.tool_added(&tool);
            tool._type(server_tool::Type::Pen);
            tool.done();
            state
                .events
                .send(ServerEvent::Tablet(tablet, tool))
                .unwrap();
        }
    }
}

macro_rules! ignore_requests {
    ($interface:ty) => {
        impl wayland_server::Dispatch<$interface, ()> for ServerState {
            fn request(
                _: &mut Self,
                _: &Client,
                _: &$interface,
                _: <$interface as wayland_server::Resource>::Request,
                _: &(),
                _: &DisplayHandle,
                _: &mut DataInit<'_, Self>,
            ) {
            }
        }
    };
}

ignore_requests!(wl_seat::WlSeat);
ignore_requests!(wl_surface::WlSurface);
ignore_requests!(wl_region::WlRegion);
ignore_requests!(ZwpTabletSeatV2);
ignore_requests!(ZwpTabletV2);
ignore_requests!(ZwpTabletToolV2);

struct ClientState;
impl
    Dispatch<
        wayland_client::protocol::wl_registry::WlRegistry,
        wayland_client::globals::GlobalListContents,
    > for ClientState
{
    fn event(
        _: &mut Self,
        _: &wayland_client::protocol::wl_registry::WlRegistry,
        _: wayland_client::protocol::wl_registry::Event,
        _: &wayland_client::globals::GlobalListContents,
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}
delegate_noop!(ClientState: ignore wayland_client::protocol::wl_compositor::WlCompositor);
delegate_noop!(ClientState: ignore wayland_client::protocol::wl_surface::WlSurface);

#[test]
fn tablet_protocol_delivers_contact_without_mouse_events_and_shuts_down_when_idle() {
    let (client_socket, server_socket) = UnixStream::pair().unwrap();
    let mut display = Display::<ServerState>::new().unwrap();
    let mut display_handle = display.handle();
    display_handle
        .insert_client(server_socket, Arc::new(()))
        .unwrap();
    display_handle.create_global::<ServerState, wl_compositor::WlCompositor, _>(1, ());
    display_handle.create_global::<ServerState, wl_seat::WlSeat, _>(7, ());
    display_handle.create_global::<ServerState, ZwpTabletManagerV2, _>(1, ());
    let (events_tx, events_rx) = mpsc::channel();
    let (server_stop, server_stopped) = mpsc::channel();
    let server = thread::spawn(move || {
        let mut state = ServerState { events: events_tx };
        loop {
            display.dispatch_clients(&mut state).unwrap();
            display.flush_clients().unwrap();
            if server_stopped
                .recv_timeout(Duration::from_millis(1))
                .is_ok()
            {
                break;
            }
        }
    });

    let connection = Connection::from_socket(client_socket).unwrap();
    let (globals, mut queue) =
        wayland_client::globals::registry_queue_init::<ClientState>(&connection).unwrap();
    let compositor: wayland_client::protocol::wl_compositor::WlCompositor =
        globals.bind(&queue.handle(), 1..=1, ()).unwrap();
    let surface = compositor.create_surface(&queue.handle(), ());
    let other_surface = compositor.create_surface(&queue.handle(), ());
    queue.roundtrip(&mut ClientState).unwrap();
    let timeout = Duration::from_secs(3);
    let ServerEvent::Surface(server_surface) = events_rx.recv_timeout(timeout).unwrap() else {
        panic!()
    };
    let ServerEvent::Surface(server_other_surface) = events_rx.recv_timeout(timeout).unwrap()
    else {
        panic!()
    };

    // Match eframe: another library owns the live display and surface.
    // SAFETY: connection/surface outlive the joined tablet worker below.
    let backend = unsafe {
        wayland_backend::client::Backend::from_foreign_display(
            connection.backend().display_ptr().cast(),
        )
    };
    let (frames, stop, worker) = start(
        Connection::from_backend(backend),
        surface.id().as_ptr() as usize,
        egui::Context::default(),
    )
    .unwrap();
    let ServerEvent::Tablet(tablet, tool) = events_rx.recv_timeout(timeout).unwrap() else {
        panic!()
    };

    // Ignore events targeting another window, even on the same display.
    tool.proximity_in(1, &tablet, &server_other_surface);
    tool.motion(10.0, 20.0);
    tool.frame(1);
    display_handle.flush_clients().unwrap();
    assert!(frames.recv_timeout(Duration::from_millis(30)).is_err());
    tool.proximity_out();
    tool.frame(2);

    // Motion may follow Down within the same protocol frame. The press must
    // carry the final coordinates, and must not depend on a pressure event.
    tool.proximity_in(2, &tablet, &server_surface);
    tool.down(3);
    tool.motion(120.0, 80.0);
    tool.frame(3);
    display_handle.flush_clients().unwrap();
    let frame = frames.recv_timeout(timeout).unwrap();
    assert_eq!(frame.position, Some(egui::pos2(120.0, 80.0)));
    assert!(frame.proximity);
    assert_eq!(frame.buttons, [true, false, false]);

    tool.button(4, 0x14b, server_tool::ButtonState::Pressed);
    tool.pressure(32768);
    tool.tilt(45.0, -30.0);
    tool.frame(4);
    display_handle.flush_clients().unwrap();
    let frame = frames.recv_timeout(timeout).unwrap();
    assert_eq!(frame.buttons, [true, true, false]);
    assert!((frame.pressure.unwrap() - 0.5).abs() < 0.0001);
    assert_eq!(frame.tilt, Some([45.0, -30.0]));
    tool.up();
    tool.proximity_out();
    tool.frame(5);
    display_handle.flush_clients().unwrap();
    let frame = frames.recv_timeout(timeout).unwrap();
    assert!(!frame.proximity);
    assert_eq!(frame.buttons, [false; 3]);

    // Hot-unplug without Up must also release an ongoing stroke.
    tool.proximity_in(5, &tablet, &server_surface);
    tool.motion(60.0, 70.0);
    tool.down(6);
    tool.frame(6);
    tool.removed();
    display_handle.flush_clients().unwrap();
    assert!(frames.recv_timeout(timeout).unwrap().buttons[0]);
    assert!(!frames.recv_timeout(timeout).unwrap().proximity);

    stop.send(()).unwrap();
    let (joined_tx, joined_rx) = mpsc::channel();
    let joiner = thread::spawn(move || {
        worker.join().unwrap();
        joined_tx.send(()).unwrap();
    });
    joined_rx
        .recv_timeout(timeout)
        .expect("tablet worker must stop without waiting for new input");
    joiner.join().unwrap();
    other_surface.destroy();
    surface.destroy();
    drop(connection);
    server_stop.send(()).unwrap();
    server.join().unwrap();
}
