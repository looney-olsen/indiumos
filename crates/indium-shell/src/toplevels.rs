//! Tracks open windows through the `wlr-foreign-toplevel-management` protocol
//! (supported by both labwc and Wayfire) so the dock can show running apps and
//! the menu bar can show the active one.
//!
//! The Wayland socket is polled from the GLib main loop, so everything stays on
//! the UI thread and no locking is needed.

use crate::util::Listeners;
use gtk::glib;
use std::cell::RefCell;
use std::os::fd::AsRawFd;
use std::rc::Rc;
use wayland_client::protocol::{wl_registry, wl_seat};
use wayland_client::{Connection, Dispatch, QueueHandle};
use wayland_protocols_wlr::foreign_toplevel::v1::client::{
    zwlr_foreign_toplevel_handle_v1 as handle_proto,
    zwlr_foreign_toplevel_manager_v1 as manager_proto,
};

use handle_proto::ZwlrForeignToplevelHandleV1 as Handle;
use manager_proto::ZwlrForeignToplevelManagerV1 as Manager;

/// One open window.
#[derive(Debug, Clone)]
pub struct Window {
    pub handle: Handle,
    pub app_id: String,
    pub title: String,
    pub activated: bool,
    pub minimized: bool,
}

impl PartialEq for Window {
    fn eq(&self, other: &Self) -> bool {
        self.handle == other.handle
    }
}

type SharedWindows = Rc<RefCell<Vec<(Window, bool)>>>; // (window, received its first `done`)
type SharedSeat = Rc<RefCell<Option<wl_seat::WlSeat>>>;

struct State {
    windows: SharedWindows,
    seat: SharedSeat,
    manager_found: bool,
    changed: bool,
}

/// Handle to the window list. If the compositor lacks the protocol (or there is
/// no Wayland at all) the tracker stays empty and the shell still works.
pub struct Toplevels {
    windows: SharedWindows,
    seat: SharedSeat,
    conn: Option<Connection>,
    listeners: Listeners<()>,
}

impl Toplevels {
    pub fn connect() -> Rc<Toplevels> {
        let windows: SharedWindows = Rc::new(RefCell::new(Vec::new()));
        let seat: SharedSeat = Rc::new(RefCell::new(None));
        let tracker = Rc::new(Toplevels {
            windows: Rc::clone(&windows),
            seat: Rc::clone(&seat),
            conn: Connection::connect_to_env().ok(),
            listeners: Listeners::default(),
        });
        match tracker.start() {
            Ok(()) => {}
            Err(reason) => eprintln!("indium: window tracking disabled ({reason})"),
        }
        tracker
    }

    fn start(self: &Rc<Self>) -> Result<(), String> {
        let conn = self.conn.clone().ok_or("no Wayland connection")?;
        let mut queue = conn.new_event_queue::<State>();
        let qh = queue.handle();
        conn.display().get_registry(&qh, ());

        let mut state = State {
            windows: Rc::clone(&self.windows),
            seat: Rc::clone(&self.seat),
            manager_found: false,
            changed: false,
        };
        queue.roundtrip(&mut state).map_err(|e| e.to_string())?;
        if !state.manager_found {
            return Err("compositor does not offer wlr-foreign-toplevel-management".into());
        }
        // A second round trip delivers the windows that are already open.
        queue.roundtrip(&mut state).map_err(|e| e.to_string())?;

        let fd = conn.backend().poll_fd().as_raw_fd();
        let tracker = Rc::clone(self);
        let connection = conn.clone();
        glib_unix::unix_fd_add_local(
            fd,
            glib::IOCondition::IN | glib::IOCondition::HUP,
            move |_, condition| {
                if condition.contains(glib::IOCondition::HUP) {
                    eprintln!("indium: compositor connection closed");
                    return glib::ControlFlow::Break;
                }
                let _ = queue.dispatch_pending(&mut state);
                if let Some(guard) = queue.prepare_read() {
                    if guard.read().is_err() {
                        return glib::ControlFlow::Break;
                    }
                }
                let _ = queue.dispatch_pending(&mut state);
                let _ = connection.flush();
                if std::mem::take(&mut state.changed) {
                    tracker.listeners.emit(&());
                }
                glib::ControlFlow::Continue
            },
        );

        Ok(())
    }

    /// Windows that have finished announcing themselves, in the order they opened.
    pub fn windows(&self) -> Vec<Window> {
        self.windows
            .borrow()
            .iter()
            .filter(|(_, ready)| *ready)
            .map(|(w, _)| w.clone())
            .collect()
    }

    pub fn active(&self) -> Option<Window> {
        self.windows().into_iter().find(|w| w.activated)
    }

    pub fn on_change(&self, callback: impl Fn() + 'static) {
        self.listeners.add(move |_| callback());
    }

    fn flush(&self) {
        if let Some(conn) = &self.conn {
            let _ = conn.flush();
        }
    }

    /// Bring a window to the front, restoring it first if it was minimized.
    pub fn activate(&self, window: &Window) {
        if let Some(seat) = self.seat.borrow().as_ref() {
            if window.minimized {
                window.handle.unset_minimized();
            }
            window.handle.activate(seat);
            self.flush();
        }
    }

    pub fn close(&self, window: &Window) {
        window.handle.close();
        self.flush();
    }
}

impl Dispatch<wl_registry::WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &wl_registry::WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            match interface.as_str() {
                "zwlr_foreign_toplevel_manager_v1" => {
                    registry.bind::<Manager, _, _>(name, version.min(3), qh, ());
                    state.manager_found = true;
                }
                "wl_seat" if state.seat.borrow().is_none() => {
                    let seat = registry.bind::<wl_seat::WlSeat, _, _>(name, version.min(5), qh, ());
                    *state.seat.borrow_mut() = Some(seat);
                }
                _ => {}
            }
        }
    }
}

impl Dispatch<wl_seat::WlSeat, ()> for State {
    fn event(
        _: &mut Self,
        _: &wl_seat::WlSeat,
        _: wl_seat::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
    }
}

impl Dispatch<Manager, ()> for State {
    fn event(
        state: &mut Self,
        _: &Manager,
        event: manager_proto::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        if let manager_proto::Event::Toplevel { toplevel } = event {
            state.windows.borrow_mut().push((
                Window {
                    handle: toplevel,
                    app_id: String::new(),
                    title: String::new(),
                    activated: false,
                    minimized: false,
                },
                false,
            ));
        }
    }

    wayland_client::event_created_child!(State, Manager, [
        manager_proto::EVT_TOPLEVEL_OPCODE => (Handle, ()),
    ]);
}

/// Window state bit values from the protocol's `state` array.
const STATE_MINIMIZED: u32 = 1;
const STATE_ACTIVATED: u32 = 2;

impl Dispatch<Handle, ()> for State {
    fn event(
        state: &mut Self,
        handle: &Handle,
        event: handle_proto::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let mut windows = state.windows.borrow_mut();
        let Some(index) = windows.iter().position(|(w, _)| &w.handle == handle) else {
            return;
        };
        match event {
            handle_proto::Event::Title { title } => windows[index].0.title = title,
            handle_proto::Event::AppId { app_id } => windows[index].0.app_id = app_id,
            handle_proto::Event::State { state: bytes } => {
                let values: Vec<u32> = (0..bytes.len() / 4)
                    .map(|i| {
                        let b = &bytes[4 * i..4 * i + 4];
                        u32::from_ne_bytes([b[0], b[1], b[2], b[3]])
                    })
                    .collect();
                windows[index].0.minimized = values.contains(&STATE_MINIMIZED);
                windows[index].0.activated = values.contains(&STATE_ACTIVATED);
            }
            handle_proto::Event::Done => {
                windows[index].1 = true;
                state.changed = true;
            }
            handle_proto::Event::Closed => {
                let (window, _) = windows.remove(index);
                window.handle.destroy();
                state.changed = true;
            }
            _ => {}
        }
    }
}

/// Which of `windows` belong to the app with this desktop ID or app_id (case-insensitive).
pub fn windows_for_app_id<'a>(windows: &'a [Window], app_id: &str) -> Vec<&'a Window> {
    let wanted = app_id.to_lowercase();
    windows
        .iter()
        .filter(|w| w.app_id.to_lowercase() == wanted)
        .collect()
}
