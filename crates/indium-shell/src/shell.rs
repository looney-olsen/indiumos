//! The shell object: owns shared state and wires the components together.

use crate::launcher::Launcher;
use crate::notifications::Notifications;
use crate::status::StatusMonitor;
use crate::theme::Theme;
use crate::toplevels::Toplevels;
use crate::util::{load_apps, spawn_detached, AppEntry, Listeners};
use crate::{bar, dock};
use gtk::prelude::*;
use gtk::{gio, glib};
use indium_core::apps::{match_app_id, AppMeta};
use indium_core::config::{user_config_path, ShellConfig};
use indium_core::profile::HardwareProfile;
use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

pub struct Shell {
    pub app: gtk::Application,
    pub profile: HardwareProfile,
    pub config: RefCell<ShellConfig>,
    pub apps: RefCell<Vec<AppEntry>>,
    /// The same apps as plain data, for searching and matching.
    pub metas: RefCell<Vec<AppMeta>>,
    pub toplevels: Rc<Toplevels>,
    pub status: Rc<StatusMonitor>,
    pub notifications: Rc<Notifications>,
    pub theme: Theme,
    pub launcher: RefCell<Option<Rc<Launcher>>>,
    pub config_changed: Listeners<()>,
    pub apps_changed: Listeners<()>,
    monitors: RefCell<Vec<gio::FileMonitor>>,
    _hold: gio::ApplicationHoldGuard,
}

impl Shell {
    /// Build every component. Called once, on the first `activate`.
    pub fn start(app: &gtk::Application) -> Rc<Shell> {
        let profile = HardwareProfile::load();
        let config = ShellConfig::load();
        eprintln!(
            "indium: profile {} ({} MB RAM, gpu accel {}), compositor {}, layer-shell {}",
            profile.kind.as_str(),
            profile.ram_mb,
            profile.gpu_accel,
            profile.compositor,
            crate::layer::available()
        );

        let theme = Theme::install(&profile, &config);
        let shell = Rc::new(Shell {
            app: app.clone(),
            toplevels: Toplevels::connect(),
            status: StatusMonitor::new(),
            notifications: Notifications::new(app, &profile),
            theme,
            profile,
            config: RefCell::new(config),
            apps: RefCell::new(Vec::new()),
            metas: RefCell::new(Vec::new()),
            launcher: RefCell::new(None),
            config_changed: Listeners::default(),
            apps_changed: Listeners::default(),
            monitors: RefCell::new(Vec::new()),
            _hold: app.hold(),
        });

        shell.reload_apps();
        install_actions(&shell);
        bar::build(&shell);
        dock::build(&shell);
        shell.status.start(shell.profile.poll_seconds());
        shell.watch_files();
        shell.start_theme_timer();
        shell
    }

    pub fn reload_apps(&self) {
        let apps = load_apps();
        *self.metas.borrow_mut() = apps.iter().map(|a| a.meta.clone()).collect();
        *self.apps.borrow_mut() = apps;
    }

    /// Index of the installed app that owns a window with this Wayland app_id.
    pub fn app_index(&self, app_id: &str) -> Option<usize> {
        match_app_id(&self.metas.borrow(), app_id)
    }

    pub fn toggle_launcher(self: &Rc<Self>) {
        let launcher = {
            let mut slot = self.launcher.borrow_mut();
            Rc::clone(slot.get_or_insert_with(|| Launcher::new(self)))
        };
        launcher.toggle();
    }

    /// Re-read the user's settings and restyle. Triggered by Settings writing the file.
    fn reload_config(self: &Rc<Self>) {
        *self.config.borrow_mut() = ShellConfig::load();
        self.theme.reload(&self.profile, &self.config.borrow());
        self.config_changed.emit(&());
    }

    fn watch_files(self: &Rc<Self>) {
        // Settings saves atomically (new file renamed over the old), so watch the directory.
        let config_path = user_config_path();
        let Some(dir) = config_path.parent().map(|p| p.to_path_buf()) else {
            return;
        };
        let _ = std::fs::create_dir_all(&dir);
        let pending = Rc::new(std::cell::Cell::new(false));
        match gio::File::for_path(&dir)
            .monitor_directory(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
        {
            Ok(monitor) => {
                let shell = Rc::clone(self);
                monitor.connect_changed(move |_, file, _, _| {
                    let name = file.basename().map(|p| p.to_string_lossy().into_owned());
                    if !matches!(name.as_deref(), Some("shell.ini") | Some("user.css")) {
                        return;
                    }
                    if pending.replace(true) {
                        return; // a reload is already scheduled; bursts collapse into one
                    }
                    let (shell, pending) = (Rc::clone(&shell), Rc::clone(&pending));
                    glib::timeout_add_local_once(Duration::from_millis(250), move || {
                        pending.set(false);
                        shell.reload_config();
                    });
                });
                self.monitors.borrow_mut().push(monitor);
            }
            Err(err) => eprintln!("indium: not watching settings: {err}"),
        }

        // New or removed apps (package installs) refresh the launcher and dock.
        let apps_monitor = gio::AppInfoMonitor::get();
        let shell = Rc::clone(self);
        apps_monitor.connect_changed(move |_| {
            shell.reload_apps();
            shell.apps_changed.emit(&());
        });
        // The monitor is a process-wide singleton; leak our handle so it is never dropped.
        std::mem::forget(apps_monitor);
    }

    /// Auto mode switches between light and dark: check once a minute.
    fn start_theme_timer(self: &Rc<Self>) {
        let shell = Rc::clone(self);
        glib::timeout_add_seconds_local(60, move || {
            shell
                .theme
                .refresh_if_needed(&shell.profile, &shell.config.borrow());
            glib::ControlFlow::Continue
        });
    }
}

/// Ask for confirmation, then run `action` if the user agrees.
fn confirm(title: &str, detail: &str, button: &str, action: impl Fn() + 'static) {
    let dialog = gtk::AlertDialog::builder()
        .message(title)
        .detail(detail)
        .buttons(["Cancel", button])
        .cancel_button(0)
        .default_button(0)
        .modal(true)
        .build();
    dialog.choose(
        None::<&gtk::Window>,
        gio::Cancellable::NONE,
        move |answer| {
            if answer == Ok(1) {
                action();
            }
        },
    );
}

fn add_action(app: &gtk::Application, name: &str, run: impl Fn() + 'static) {
    let action = gio::SimpleAction::new(name, None);
    action.connect_activate(move |_, _| run());
    app.add_action(&action);
}

fn install_actions(shell: &Rc<Shell>) {
    let app = &shell.app;

    add_action(app, "about", || {
        spawn_detached("indium-settings", &["about"])
    });
    add_action(app, "settings", || spawn_detached("indium-settings", &[]));
    add_action(app, "lock", || {
        spawn_detached("swaylock", &["-f", "-c", "1d1d1f"])
    });
    add_action(app, "sleep", || spawn_detached("systemctl", &["suspend"]));
    add_action(app, "restart", || {
        confirm(
            "Restart now?",
            "Open apps will be closed.",
            "Restart",
            || spawn_detached("systemctl", &["reboot"]),
        )
    });
    add_action(app, "shutdown", || {
        confirm(
            "Shut down now?",
            "Open apps will be closed.",
            "Shut Down",
            || spawn_detached("systemctl", &["poweroff"]),
        )
    });
    add_action(app, "logout", || {
        confirm(
            "Log out now?",
            "Open apps will be closed.",
            "Log Out",
            || match std::env::var("XDG_SESSION_ID") {
                Ok(id) => spawn_detached("loginctl", &["terminate-session", &id]),
                Err(_) => {
                    let user = std::env::var("USER").unwrap_or_default();
                    spawn_detached("loginctl", &["terminate-user", &user]);
                }
            },
        )
    });

    let toggler = Rc::clone(shell);
    add_action(app, "toggle-launcher", move || toggler.toggle_launcher());
    let quitter = app.clone();
    add_action(app, "quit", move || quitter.quit());
}
