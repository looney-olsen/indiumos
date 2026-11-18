//! Polls volume, battery, network (and, when asked, brightness and Bluetooth)
//! off the UI thread and tells subscribers when something changed.

use crate::util::{run_blocking, spawn_ui, Listeners};
use gtk::glib;
use indium_core::sys::{self, Battery, Network, Volume};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub volume: Option<Volume>,
    pub battery: Option<Battery>,
    pub network: Network,
    pub brightness: Option<u8>,
    pub bluetooth: bool,
}

pub struct StatusMonitor {
    current: RefCell<Status>,
    listeners: Listeners<Status>,
    busy: Cell<bool>,
}

fn read(detailed: bool, previous: Status) -> Status {
    let mut status = Status {
        volume: sys::volume(),
        battery: sys::battery(),
        network: sys::network(),
        // Brightness and Bluetooth are slower to query: only refresh them on request.
        brightness: previous.brightness,
        bluetooth: previous.bluetooth,
    };
    if detailed {
        status.brightness = sys::brightness();
        status.bluetooth = sys::bluetooth_powered();
    }
    status
}

impl StatusMonitor {
    pub fn new() -> Rc<Self> {
        Rc::new(StatusMonitor {
            current: RefCell::new(Status::default()),
            listeners: Listeners::default(),
            busy: Cell::new(false),
        })
    }

    pub fn current(&self) -> Status {
        self.current.borrow().clone()
    }

    pub fn subscribe(&self, callback: impl Fn(&Status) + 'static) {
        self.listeners.add(callback);
    }

    /// Poll now, and then every `interval` seconds.
    pub fn start(self: &Rc<Self>, interval: u32) {
        self.refresh(true);
        let monitor = Rc::clone(self);
        glib::timeout_add_seconds_local(interval, move || {
            monitor.refresh(false);
            glib::ControlFlow::Continue
        });
    }

    /// Read the system state in the background and notify subscribers if it changed.
    pub fn refresh(self: &Rc<Self>, detailed: bool) {
        if self.busy.replace(true) {
            return;
        }
        let monitor = Rc::clone(self);
        let previous = self.current();
        spawn_ui(async move {
            let fresh = run_blocking(move || read(detailed, previous)).await;
            monitor.busy.set(false);
            if let Some(status) = fresh {
                let changed = *monitor.current.borrow() != status;
                if changed {
                    *monitor.current.borrow_mut() = status.clone();
                    monitor.listeners.emit(&status);
                }
            }
        });
    }
}
