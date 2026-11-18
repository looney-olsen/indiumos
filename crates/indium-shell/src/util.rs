//! Small helpers shared by the shell components.

use gtk::prelude::*;
use gtk::{gio, glib};
use indium_core::apps::AppMeta;
use std::process::{Command, Stdio};

/// An installed application: searchable metadata plus the GIO handle used to launch it.
pub struct AppEntry {
    pub meta: AppMeta,
    pub info: gio_unix::DesktopAppInfo,
}

/// Every application that should be shown in menus, sorted by name.
pub fn load_apps() -> Vec<AppEntry> {
    let mut apps: Vec<AppEntry> = gio::AppInfo::all()
        .into_iter()
        .filter(|info| info.should_show())
        .filter_map(|info| info.downcast::<gio_unix::DesktopAppInfo>().ok())
        .map(|info| {
            let meta = AppMeta {
                id: info.id().map(|s| s.to_string()).unwrap_or_default(),
                name: info.name().to_string(),
                generic_name: info
                    .generic_name()
                    .map(|s| s.to_string())
                    .unwrap_or_default(),
                comment: info
                    .description()
                    .map(|s| s.to_string())
                    .unwrap_or_default(),
                keywords: info.keywords().iter().map(|k| k.to_string()).collect(),
                icon: info
                    .icon()
                    .and_then(|i| IconExt::to_string(&i))
                    .map(|s| s.to_string())
                    .unwrap_or_default(),
                wm_class: info
                    .startup_wm_class()
                    .map(|s| s.to_string())
                    .unwrap_or_default(),
                exec: info
                    .commandline()
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                alt_ids: info
                    .string("X-Indium-AppIds")
                    .map(|v| {
                        v.split(';')
                            .filter(|p| !p.is_empty())
                            .map(str::to_string)
                            .collect()
                    })
                    .unwrap_or_default(),
            };
            AppEntry { meta, info }
        })
        .collect();
    apps.sort_by_key(|a| a.meta.name.to_lowercase());
    apps
}

/// Launch an app the way a desktop environment should (startup notification, field codes).
pub fn launch_app(entry: &AppEntry) {
    let context = gtk::gdk::Display::default().map(|d| d.app_launch_context());
    if let Err(err) = entry.info.launch(&[], context.as_ref()) {
        eprintln!("indium: could not launch {}: {err}", entry.meta.name);
    }
}

/// Start a command in the background and reap it when it exits.
pub fn spawn_detached(program: &str, args: &[&str]) {
    match Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(mut child) => {
            std::thread::spawn(move || {
                let _ = child.wait();
            });
        }
        Err(err) => eprintln!("indium: could not run {program}: {err}"),
    }
}

/// Run blocking work (external commands, file reads) off the UI thread.
/// Resolves to `None` if the worker panicked.
pub async fn run_blocking<T, F>(work: F) -> Option<T>
where
    T: Send + 'static,
    F: FnOnce() -> T + Send + 'static,
{
    gio::spawn_blocking(work).await.ok()
}

/// Run an async block on the main loop.
pub fn spawn_ui<F: std::future::Future<Output = ()> + 'static>(future: F) {
    glib::spawn_future_local(future);
}

/// Percent-encode text for use in a URL query.
pub fn url_escape(text: &str) -> String {
    glib::Uri::escape_string(text, None, false).to_string()
}

type Callback<T> = std::rc::Rc<dyn Fn(&T)>;

/// A shared list of callbacks.
pub struct Listeners<T> {
    callbacks: std::cell::RefCell<Vec<Callback<T>>>,
}

impl<T> Default for Listeners<T> {
    fn default() -> Self {
        Listeners {
            callbacks: std::cell::RefCell::new(Vec::new()),
        }
    }
}

impl<T> Listeners<T> {
    pub fn add(&self, callback: impl Fn(&T) + 'static) {
        self.callbacks.borrow_mut().push(std::rc::Rc::new(callback));
    }

    /// Call every listener. Callbacks may register more listeners while running.
    pub fn emit(&self, value: &T) {
        let snapshot: Vec<_> = self.callbacks.borrow().clone();
        for callback in snapshot {
            callback(value);
        }
    }
}

/// Runs only the last of a burst of calls, `ms` milliseconds after it was made.
/// Used for sliders so dragging does not spawn a command per pixel.
#[derive(Default)]
pub struct Debounce {
    pending: std::rc::Rc<std::cell::RefCell<Option<glib::SourceId>>>,
}

impl Debounce {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn call(&self, ms: u64, work: impl FnOnce() + 'static) {
        if let Some(id) = self.pending.borrow_mut().take() {
            id.remove();
        }
        let pending = std::rc::Rc::clone(&self.pending);
        let id = glib::timeout_add_local_once(std::time::Duration::from_millis(ms), move || {
            pending.borrow_mut().take(); // fired: nothing left to cancel
            work();
        });
        *self.pending.borrow_mut() = Some(id);
    }
}
