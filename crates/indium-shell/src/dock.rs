//! The dock: pinned apps plus running apps, with a dot under each running one.

use crate::layer::{self, Keyboard, Layer, Spec, Zone};
use crate::shell::Shell;
use crate::toplevels::Window;
use crate::util::launch_app;
use gtk::prelude::*;
use indium_core::apps::match_app_id;
use std::rc::Rc;

/// Pixels the dock occupies below the icons: button padding, dock padding,
/// border and the gap to the screen edge.
const CHROME: i32 = 8 + 12 + 2 + 6;

/// One icon in the dock.
struct Item {
    /// Index into `Shell::apps`, when the app is installed.
    app: Option<usize>,
    /// Wayland app_id for running windows that match no installed app.
    app_id: String,
    windows: Vec<Window>,
    favorite: bool,
}

fn collect_items(shell: &Shell) -> Vec<Item> {
    let metas = shell.metas.borrow();
    let config = shell.config.borrow();
    let windows = shell.toplevels.windows();

    let mut items: Vec<Item> = config
        .dock_favorites
        .iter()
        .filter_map(|id| metas.iter().position(|m| &m.id == id))
        .map(|app| Item {
            app: Some(app),
            app_id: String::new(),
            windows: Vec::new(),
            favorite: true,
        })
        .collect();

    for window in windows {
        // A pinned launcher that lists this window's app ID (e.g. the browser
        // launcher for Epiphany/Firefox) claims it first.
        if let Some(item) = items.iter_mut().find(|item| {
            item.favorite
                && item
                    .app
                    .is_some_and(|a| metas[a].alt_matches(&window.app_id))
        }) {
            item.windows.push(window);
            continue;
        }
        let app = match_app_id(&metas, &window.app_id);
        let slot = items.iter_mut().find(|item| match app {
            Some(_) => item.app == app,
            None => item.app.is_none() && item.app_id == window.app_id,
        });
        match slot {
            Some(item) => item.windows.push(window),
            None => items.push(Item {
                app,
                app_id: window.app_id.clone(),
                windows: vec![window],
                favorite: false,
            }),
        }
    }
    items
}

pub fn build(shell: &Rc<Shell>) -> gtk::ApplicationWindow {
    let window = gtk::ApplicationWindow::builder()
        .application(&shell.app)
        .decorated(false)
        .resizable(false)
        .build();
    window.add_css_class("indium-dock");
    window.add_css_class(shell.profile.effects_class());
    if shell.profile.dock_magnification() {
        window.add_css_class("magnify");
    }

    let dock = gtk::Box::new(gtk::Orientation::Horizontal, 2);
    dock.add_css_class("dock");
    let outer = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    outer.add_css_class("dock-outer");
    outer.append(&dock);
    window.set_child(Some(&outer));

    let size = shell.config.borrow().dock_icon_size as i32;
    let is_layer = layer::apply(
        &window,
        &Spec {
            bottom: true,
            margin: (0, 0, 6, 0),
            zone: Zone::Fixed(size + CHROME),
            keyboard: Keyboard::None,
            ..Spec::new("indium-dock", Layer::Top)
        },
    );
    if !is_layer {
        window.set_default_size(520, 90);
    }

    let refresh = {
        let (shell, window, dock) = (Rc::clone(shell), window.clone(), dock.clone());
        move || {
            let size = shell.config.borrow().dock_icon_size as i32;
            layer::set_zone(&window, size + CHROME);
            rebuild(&shell, &dock);
        }
    };
    refresh();
    shell.toplevels.on_change(refresh.clone());
    let again = refresh.clone();
    shell.config_changed.add(move |_| again());
    shell.apps_changed.add(move |_| refresh());

    window.present();
    window
}

fn rebuild(shell: &Rc<Shell>, dock: &gtk::Box) {
    while let Some(child) = dock.first_child() {
        dock.remove(&child);
    }
    let size = shell.config.borrow().dock_icon_size as i32;
    let items = collect_items(shell);

    let mut separator_added = false;
    for item in items {
        if !item.favorite && !separator_added && dock.first_child().is_some() {
            let sep = gtk::Separator::new(gtk::Orientation::Vertical);
            sep.add_css_class("dock-sep");
            dock.append(&sep);
            separator_added = true;
        }
        dock.append(&make_button(shell, item, size));
    }
}

fn item_name(shell: &Shell, item: &Item) -> String {
    match item.app {
        Some(i) => shell.metas.borrow()[i].name.clone(),
        None => item.app_id.clone(),
    }
}

fn make_button(shell: &Rc<Shell>, item: Item, size: i32) -> gtk::Button {
    let icon = match item.app.and_then(|i| shell.apps.borrow()[i].info.icon()) {
        Some(gicon) => gtk::Image::from_gicon(&gicon),
        None if !item.app_id.is_empty() => gtk::Image::from_icon_name(&item.app_id),
        None => gtk::Image::from_icon_name("application-x-executable"),
    };
    icon.set_pixel_size(size);

    let overlay = gtk::Overlay::new();
    overlay.set_child(Some(&icon));
    if !item.windows.is_empty() {
        let dot = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        dot.add_css_class("run-dot");
        dot.set_halign(gtk::Align::Center);
        dot.set_valign(gtk::Align::End);
        overlay.add_overlay(&dot);
    }

    let button = gtk::Button::new();
    button.add_css_class("dock-item");
    button.set_child(Some(&overlay));
    button.set_tooltip_text(Some(&item_name(shell, &item)));

    let item = Rc::new(item);
    {
        let (shell, item) = (Rc::clone(shell), Rc::clone(&item));
        button.connect_clicked(move |_| activate(&shell, &item));
    }

    let menu = context_menu(shell, &item, &button);
    let right_click = gtk::GestureClick::builder().button(3).build();
    {
        let menu = menu.clone();
        right_click.connect_pressed(move |_, _, _, _| menu.popup());
    }
    button.add_controller(right_click);
    // The popover is parented to the button, so it must go away with it.
    button.connect_destroy(move |_| menu.unparent());
    button
}

/// Click: launch if closed, otherwise bring the app forward (cycling its windows).
fn activate(shell: &Rc<Shell>, item: &Item) {
    if item.windows.is_empty() {
        if let Some(i) = item.app {
            launch_app(&shell.apps.borrow()[i]);
        }
        return;
    }
    let count = item.windows.len();
    let target = match item.windows.iter().position(|w| w.activated) {
        Some(current) if count > 1 => &item.windows[(current + 1) % count],
        Some(current) => &item.windows[current],
        None => &item.windows[0],
    };
    shell.toplevels.activate(target);
}

fn context_menu(shell: &Rc<Shell>, item: &Rc<Item>, parent: &gtk::Button) -> gtk::Popover {
    let popover = gtk::Popover::new();
    popover.add_css_class("indium");
    popover.set_parent(parent);

    let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
    column.append(
        &gtk::Label::builder()
            .label(item_name(shell, item))
            .xalign(0.0)
            .css_classes(["menu-title"])
            .build(),
    );

    let entry = |label: &str| {
        let b = gtk::Button::with_label(label);
        b.add_css_class("menu-item");
        if let Some(child) = b.child().and_downcast::<gtk::Label>() {
            child.set_xalign(0.0);
        }
        b
    };

    if item.app.is_some() {
        let open = entry(if item.windows.is_empty() {
            "Open"
        } else {
            "New Window"
        });
        let (shell, item, popover) = (Rc::clone(shell), Rc::clone(item), popover.clone());
        open.connect_clicked(move |_| {
            popover.popdown();
            if let Some(i) = item.app {
                launch_app(&shell.apps.borrow()[i]);
            }
        });
        column.append(&open);
    }

    if !item.windows.is_empty() {
        let close = entry("Close");
        let (shell, item, popover) = (Rc::clone(shell), Rc::clone(item), popover.clone());
        close.connect_clicked(move |_| {
            popover.popdown();
            for window in &item.windows {
                shell.toplevels.close(window);
            }
        });
        column.append(&close);
    }

    if let Some(app) = item.app {
        let id = shell.metas.borrow()[app].id.clone();
        let favorite = item.favorite;
        let pin = entry(if favorite {
            "Remove from Dock"
        } else {
            "Keep in Dock"
        });
        let (shell, popover) = (Rc::clone(shell), popover.clone());
        pin.connect_clicked(move |_| {
            popover.popdown();
            let mut favorites = shell.config.borrow().dock_favorites.clone();
            if favorite {
                favorites.retain(|f| f != &id);
            } else if !favorites.contains(&id) {
                favorites.push(id.clone());
            }
            shell.config.borrow_mut().dock_favorites = favorites;
            if let Err(err) = shell.config.borrow().save() {
                eprintln!("indium: could not save dock settings: {err}");
            }
            shell.config_changed.emit(&());
        });
        column.append(&pin);
    }

    popover.set_child(Some(&column));
    popover
}
