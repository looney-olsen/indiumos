//! The Spotlight-style launcher: apps, a calculator, system commands and web search.
//!
//! The window is created on first use and kept hidden afterwards, so it costs
//! nothing while the machine is idle.

use crate::layer::{self, Keyboard, Layer, Spec, Zone};
use crate::shell::Shell;
use crate::util::{launch_app, spawn_detached, url_escape};
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;
use indium_core::apps::{rank, score, AppMeta};
use indium_core::calc;
use std::cell::RefCell;
use std::rc::{Rc, Weak};

const MAX_RESULTS: usize = 8;

/// System commands that can be found by typing, with the app action each runs.
const COMMANDS: &[(&str, &str, &str, &str)] = &[
    (
        "Lock Screen",
        "Lock the screen",
        "system-lock-screen-symbolic",
        "lock",
    ),
    (
        "Sleep",
        "Suspend this computer",
        "media-playback-pause-symbolic",
        "sleep",
    ),
    (
        "Restart",
        "Restart this computer",
        "system-reboot-symbolic",
        "restart",
    ),
    (
        "Shut Down",
        "Turn off this computer",
        "system-shutdown-symbolic",
        "shutdown",
    ),
    (
        "Log Out",
        "End this session",
        "system-log-out-symbolic",
        "logout",
    ),
    (
        "System Settings",
        "Appearance, display, performance",
        "preferences-system-symbolic",
        "settings",
    ),
];

#[derive(Clone)]
enum Item {
    App(usize),
    Calc(String),
    Command(&'static str),
    Web(String),
}

pub struct Launcher {
    shell: Weak<Shell>,
    window: gtk::Window,
    entry: gtk::SearchEntry,
    list: gtk::ListBox,
    items: RefCell<Vec<Item>>,
}

fn icon_row(icon: &gtk::Image, title: &str, subtitle: &str) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    icon.set_pixel_size(32);
    row.append(icon);
    let text = gtk::Box::new(gtk::Orientation::Vertical, 0);
    text.set_valign(gtk::Align::Center);
    text.append(
        &gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .css_classes(["result-title"])
            .build(),
    );
    if !subtitle.is_empty() {
        text.append(
            &gtk::Label::builder()
                .label(subtitle)
                .xalign(0.0)
                .ellipsize(gtk::pango::EllipsizeMode::End)
                .css_classes(["result-sub"])
                .build(),
        );
    }
    row.append(&text);
    row
}

impl Launcher {
    pub fn new(shell: &Rc<Shell>) -> Rc<Launcher> {
        let window = gtk::Window::builder()
            .application(&shell.app)
            .decorated(false)
            .build();
        window.add_css_class("indium-launcher");
        window.add_css_class(shell.profile.effects_class());
        let is_layer = layer::apply(
            &window,
            &Spec {
                top: true,
                bottom: true,
                left: true,
                right: true,
                zone: Zone::Ignore,
                keyboard: Keyboard::Exclusive,
                ..Spec::new("indium-launcher", Layer::Overlay)
            },
        );
        if !is_layer {
            window.set_default_size(900, 600);
        }

        let entry = gtk::SearchEntry::new();
        entry.set_placeholder_text(Some("Spotlight Search"));
        let list = gtk::ListBox::new();
        list.set_selection_mode(gtk::SelectionMode::Single);
        list.set_activate_on_single_click(true);
        list.set_visible(false);

        let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
        content.add_css_class("launcher");
        content.set_halign(gtk::Align::Center);
        content.set_valign(gtk::Align::Start);
        content.set_margin_top(140);
        content.append(&entry);
        content.append(&list);

        // The window covers the whole screen; clicking outside the panel closes it.
        let backdrop = gtk::Box::new(gtk::Orientation::Vertical, 0);
        backdrop.set_hexpand(true);
        backdrop.set_vexpand(true);
        backdrop.append(&content);
        window.set_child(Some(&backdrop));

        let launcher = Rc::new(Launcher {
            shell: Rc::downgrade(shell),
            window,
            entry,
            list,
            items: RefCell::new(Vec::new()),
        });
        launcher.connect_signals(&backdrop, &content);
        launcher
    }

    fn connect_signals(self: &Rc<Self>, backdrop: &gtk::Box, content: &gtk::Box) {
        {
            let this = Rc::clone(self);
            self.entry.connect_search_changed(move |_| this.refresh());
        }
        {
            let this = Rc::clone(self);
            self.entry.connect_activate(move |_| {
                let index = this.list.selected_row().map(|r| r.index()).unwrap_or(0);
                this.run(index);
            });
        }
        {
            let this = Rc::clone(self);
            self.list
                .connect_row_activated(move |_, row| this.run(row.index()));
        }
        {
            let this = Rc::clone(self);
            self.entry.connect_stop_search(move |_| this.hide());
        }

        // Arrow keys move through the results while the cursor stays in the entry.
        let keys = gtk::EventControllerKey::new();
        keys.set_propagation_phase(gtk::PropagationPhase::Capture);
        {
            let this = Rc::clone(self);
            keys.connect_key_pressed(move |_, key, _, _| match key {
                gdk::Key::Escape => {
                    this.hide();
                    glib::Propagation::Stop
                }
                gdk::Key::Down | gdk::Key::Tab => {
                    this.move_selection(1);
                    glib::Propagation::Stop
                }
                gdk::Key::Up | gdk::Key::ISO_Left_Tab => {
                    this.move_selection(-1);
                    glib::Propagation::Stop
                }
                _ => glib::Propagation::Proceed,
            });
        }
        self.window.add_controller(keys);

        let click = gtk::GestureClick::new();
        {
            let this = Rc::clone(self);
            let (backdrop, content) = (backdrop.clone(), content.clone());
            click.connect_pressed(move |_, _, x, y| {
                let inside = backdrop
                    .pick(x, y, gtk::PickFlags::DEFAULT)
                    .map(|w| {
                        w == content.clone().upcast::<gtk::Widget>() || w.is_ancestor(&content)
                    })
                    .unwrap_or(false);
                if !inside {
                    this.hide();
                }
            });
        }
        backdrop.add_controller(click);
    }

    pub fn toggle(self: &Rc<Self>) {
        if self.window.is_visible() {
            self.hide();
        } else {
            self.entry.set_text("");
            self.refresh();
            self.window.present();
            self.entry.grab_focus();
        }
    }

    fn hide(&self) {
        self.window.set_visible(false);
    }

    fn move_selection(&self, delta: i32) {
        let count = self.items.borrow().len() as i32;
        if count == 0 {
            return;
        }
        let current = self.list.selected_row().map(|r| r.index()).unwrap_or(-1);
        let next = (current + delta).rem_euclid(count);
        if let Some(row) = self.list.row_at_index(next) {
            self.list.select_row(Some(&row));
        }
    }

    /// Work out the results for the current text and redraw the list.
    fn refresh(&self) {
        let Some(shell) = self.shell.upgrade() else {
            return;
        };
        let query = self.entry.text().to_string();
        let query = query.trim();
        let mut items: Vec<Item> = Vec::new();

        if !query.is_empty() {
            if let Some(answer) = calc::calculate(query) {
                items.push(Item::Calc(answer));
            }
            for index in rank(&shell.metas.borrow(), query, MAX_RESULTS) {
                items.push(Item::App(index));
            }
            for (name, _, _, action) in COMMANDS {
                let meta = AppMeta {
                    name: name.to_string(),
                    ..AppMeta::default()
                };
                if score(&meta, query) >= 400 {
                    items.push(Item::Command(action));
                }
            }
            items.truncate(MAX_RESULTS);
            items.push(Item::Web(query.to_string()));
        }

        while let Some(row) = self.list.first_child() {
            self.list.remove(&row);
        }
        for item in &items {
            self.list.append(&self.make_row(&shell, item));
        }
        self.list.set_visible(!items.is_empty());
        if let Some(first) = self.list.row_at_index(0) {
            self.list.select_row(Some(&first));
        }
        *self.items.borrow_mut() = items;
    }

    fn make_row(&self, shell: &Shell, item: &Item) -> gtk::Widget {
        match item {
            Item::App(index) => {
                let apps = shell.apps.borrow();
                let app = &apps[*index];
                let icon = match app.info.icon() {
                    Some(gicon) => gtk::Image::from_gicon(&gicon),
                    None => gtk::Image::from_icon_name("application-x-executable"),
                };
                let sub = if app.meta.generic_name.is_empty() {
                    &app.meta.comment
                } else {
                    &app.meta.generic_name
                };
                icon_row(&icon, &app.meta.name, sub).upcast()
            }
            Item::Calc(answer) => {
                let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
                row.append(&gtk::Image::from_icon_name(
                    "accessories-calculator-symbolic",
                ));
                row.append(
                    &gtk::Label::builder()
                        .label(format!("= {answer}"))
                        .hexpand(true)
                        .xalign(0.0)
                        .css_classes(["calc-result"])
                        .build(),
                );
                row.append(
                    &gtk::Label::builder()
                        .label("Enter to copy")
                        .css_classes(["result-sub"])
                        .build(),
                );
                row.upcast()
            }
            Item::Command(action) => {
                let (name, sub, icon, _) = COMMANDS
                    .iter()
                    .find(|(_, _, _, a)| a == action)
                    .copied()
                    .unwrap_or(("", "", "application-x-executable", ""));
                icon_row(&gtk::Image::from_icon_name(icon), name, sub).upcast()
            }
            Item::Web(text) => icon_row(
                &gtk::Image::from_icon_name("system-search-symbolic"),
                &format!("Search the web for “{text}”"),
                "Opens your default browser",
            )
            .upcast(),
        }
    }

    fn run(&self, index: i32) {
        let Some(shell) = self.shell.upgrade() else {
            return;
        };
        let item = self.items.borrow().get(index.max(0) as usize).cloned();
        let Some(item) = item else {
            return;
        };
        self.hide();
        match item {
            Item::App(i) => launch_app(&shell.apps.borrow()[i]),
            Item::Calc(answer) => {
                if let Some(display) = gdk::Display::default() {
                    display.clipboard().set_text(&answer);
                }
            }
            Item::Command(action) => {
                gtk::gio::prelude::ActionGroupExt::activate_action(&shell.app, action, None);
            }
            Item::Web(text) => {
                let url = format!("https://duckduckgo.com/?q={}", url_escape(&text));
                spawn_detached("xdg-open", &[&url]);
            }
        }
    }
}
