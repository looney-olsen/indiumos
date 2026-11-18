//! `indium-shell`: the Indium OS desktop shell daemon.
//!
//!   indium-shell                   start the shell (menu bar, dock, notifications, launcher)
//!   indium-shell --toggle-launcher show or hide Spotlight in the running shell
//!   indium-shell --quit            stop the running shell

use gtk::prelude::*;
use gtk::{gio, glib};
use indium_shell::shell::Shell;
use std::cell::RefCell;
use std::rc::Rc;

fn main() -> glib::ExitCode {
    let command = std::env::args().nth(1);
    let app = gtk::Application::builder()
        .application_id("org.indium.Shell")
        .build();

    if let Err(err) = app.register(gio::Cancellable::NONE) {
        eprintln!("indium-shell: could not reach the session bus: {err}");
        return glib::ExitCode::FAILURE;
    }

    if app.is_remote() {
        // A shell is already running: forward the request to it and exit.
        match command.as_deref() {
            Some("--toggle-launcher") | Some("launcher") => {
                app.activate_action("toggle-launcher", None)
            }
            Some("--quit") => app.activate_action("quit", None),
            _ => eprintln!("indium-shell: already running"),
        }
        if let Some(connection) = app.dbus_connection() {
            let _ = connection.flush_sync(gio::Cancellable::NONE);
        }
        std::process::exit(0);
    }

    let shell: Rc<RefCell<Option<Rc<Shell>>>> = Rc::new(RefCell::new(None));
    app.connect_activate(move |app| {
        if shell.borrow().is_none() {
            *shell.borrow_mut() = Some(Shell::start(app));
        }
    });
    app.run_with_args::<&str>(&[])
}
