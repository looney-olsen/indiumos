//! The control center popover: Wi-Fi, Bluetooth, Do Not Disturb, brightness,
//! volume and the notification history.

use crate::shell::Shell;
use crate::status::Status;
use crate::util::{run_blocking, spawn_ui, Debounce};
use gtk::prelude::*;
use indium_core::sys;
use std::cell::Cell;
use std::rc::Rc;

/// A square toggle with an icon, a title and a small status line.
fn tile(icon: &str, title: &str) -> (gtk::ToggleButton, gtk::Label) {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 2);
    content.set_halign(gtk::Align::Start);
    let image = gtk::Image::from_icon_name(icon);
    image.set_pixel_size(20);
    image.set_halign(gtk::Align::Start);
    content.append(&image);
    content.append(&gtk::Label::builder().label(title).xalign(0.0).build());
    let sub = gtk::Label::builder()
        .label("")
        .xalign(0.0)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .max_width_chars(14)
        .css_classes(["tile-sub"])
        .build();
    content.append(&sub);
    let button = gtk::ToggleButton::new();
    button.add_css_class("cc-tile");
    button.set_child(Some(&content));
    button.set_hexpand(true);
    (button, sub)
}

fn slider_row(icon: &str, scale: &gtk::Scale) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    row.add_css_class("cc-slider-box");
    row.append(&gtk::Image::from_icon_name(icon));
    scale.set_hexpand(true);
    scale.set_draw_value(false);
    row.append(scale);
    row
}

fn volume_icon(volume: Option<sys::Volume>) -> &'static str {
    match volume {
        None => "audio-volume-muted-symbolic",
        Some(v) if v.muted || v.percent == 0 => "audio-volume-muted-symbolic",
        Some(v) if v.percent < 34 => "audio-volume-low-symbolic",
        Some(v) if v.percent < 67 => "audio-volume-medium-symbolic",
        Some(_) => "audio-volume-high-symbolic",
    }
}

pub fn build(shell: &Rc<Shell>) -> gtk::Popover {
    let popover = gtk::Popover::new();
    popover.add_css_class("cc");
    let root = gtk::Box::new(gtk::Orientation::Vertical, 8);
    popover.set_child(Some(&root));

    // Programmatic updates must not be mistaken for the user's clicks and drags.
    let updating = Rc::new(Cell::new(false));

    // ---- toggles
    let tiles = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    tiles.set_homogeneous(true);
    let (wifi, wifi_sub) = tile("network-wireless-symbolic", "Wi-Fi");
    let (bluetooth, bluetooth_sub) = tile("bluetooth-active-symbolic", "Bluetooth");
    let (dnd, dnd_sub) = tile("notifications-disabled-symbolic", "Focus");
    tiles.append(&wifi);
    tiles.append(&bluetooth);
    tiles.append(&dnd);
    root.append(&tiles);

    {
        let (shell, updating) = (Rc::clone(shell), Rc::clone(&updating));
        wifi.connect_toggled(move |button| {
            if updating.get() {
                return;
            }
            let on = button.is_active();
            let shell = Rc::clone(&shell);
            spawn_ui(async move {
                run_blocking(move || sys::set_wifi(on)).await;
                shell.status.refresh(true);
            });
        });
    }
    {
        let (shell, updating) = (Rc::clone(shell), Rc::clone(&updating));
        bluetooth.connect_toggled(move |button| {
            if updating.get() {
                return;
            }
            let on = button.is_active();
            let shell = Rc::clone(&shell);
            spawn_ui(async move {
                run_blocking(move || sys::set_bluetooth(on)).await;
                shell.status.refresh(true);
            });
        });
    }
    {
        let (shell, updating) = (Rc::clone(shell), Rc::clone(&updating));
        let dnd_sub = dnd_sub.clone();
        dnd.connect_toggled(move |button| {
            if updating.get() {
                return;
            }
            shell.notifications.set_do_not_disturb(button.is_active());
            dnd_sub.set_text(if button.is_active() {
                "Do Not Disturb"
            } else {
                "Off"
            });
        });
    }
    dnd_sub.set_text("Off");

    // ---- sliders
    let display_label = section_label("Display");
    root.append(&display_label);
    let brightness = gtk::Scale::with_range(gtk::Orientation::Horizontal, 1.0, 100.0, 1.0);
    let brightness_row = slider_row("display-brightness-symbolic", &brightness);
    root.append(&brightness_row);
    {
        let (updating, debounce) = (Rc::clone(&updating), Debounce::new());
        brightness.connect_value_changed(move |scale| {
            if updating.get() {
                return;
            }
            let value = scale.value() as u8;
            debounce.call(120, move || {
                spawn_ui(async move {
                    run_blocking(move || sys::set_brightness(value)).await;
                });
            });
        });
    }

    let sound_label = section_label("Sound");
    root.append(&sound_label);
    let volume = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    let volume_row = slider_row("audio-volume-medium-symbolic", &volume);
    let volume_icon_widget = volume_row
        .first_child()
        .and_downcast::<gtk::Image>()
        .expect("slider row starts with an image");
    root.append(&volume_row);
    {
        let (updating, debounce) = (Rc::clone(&updating), Debounce::new());
        volume.connect_value_changed(move |scale| {
            if updating.get() {
                return;
            }
            let value = scale.value() as u8;
            debounce.call(80, move || {
                spawn_ui(async move {
                    run_blocking(move || sys::set_volume(value)).await;
                });
            });
        });
    }

    // ---- notification history
    let header = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let title = section_label("Notifications");
    title.set_hexpand(true);
    header.append(&title);
    let clear = gtk::Button::with_label("Clear");
    clear.add_css_class("flat");
    header.append(&clear);
    root.append(&header);

    let history = gtk::ListBox::new();
    history.add_css_class("cc-history");
    history.set_selection_mode(gtk::SelectionMode::None);
    let scroller = gtk::ScrolledWindow::builder()
        .child(&history)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .max_content_height(260)
        .propagate_natural_height(true)
        .build();
    root.append(&scroller);

    {
        let shell = Rc::clone(shell);
        clear.connect_clicked(move |_| shell.notifications.clear_history());
    }
    let rebuild_history = {
        let (shell, history) = (Rc::clone(shell), history.clone());
        move || {
            while let Some(row) = history.first_child() {
                history.remove(&row);
            }
            let items = shell.notifications.history();
            if items.is_empty() {
                history.append(
                    &gtk::Label::builder()
                        .label("No notifications")
                        .css_classes(["cc-empty"])
                        .build(),
                );
                return;
            }
            for n in items {
                let text = gtk::Box::new(gtk::Orientation::Vertical, 1);
                text.set_hexpand(true);
                text.append(
                    &gtk::Label::builder()
                        .label(format!("{} · {}", n.app_name, n.summary))
                        .xalign(0.0)
                        .ellipsize(gtk::pango::EllipsizeMode::End)
                        .max_width_chars(34)
                        .build(),
                );
                if !n.body.is_empty() {
                    text.append(
                        &gtk::Label::builder()
                            .label(&n.body)
                            .xalign(0.0)
                            .ellipsize(gtk::pango::EllipsizeMode::End)
                            .max_width_chars(34)
                            .css_classes(["tile-sub"])
                            .build(),
                    );
                }
                history.append(&text);
            }
        }
    };
    rebuild_history();
    shell.notifications.on_history_change(rebuild_history);

    // ---- keep widgets in sync with the system
    let apply = {
        let updating = Rc::clone(&updating);
        move |status: &Status| {
            updating.set(true);
            wifi.set_active(status.network.wifi_enabled);
            wifi_sub.set_text(
                match (&status.network.wifi_ssid, status.network.wifi_enabled) {
                    (Some(ssid), _) => ssid,
                    (None, true) => "Not connected",
                    (None, false) => "Off",
                },
            );
            bluetooth.set_active(status.bluetooth);
            bluetooth_sub.set_text(if status.bluetooth { "On" } else { "Off" });
            match status.brightness {
                Some(b) => {
                    brightness.set_value(b as f64);
                    brightness_row.set_visible(true);
                    display_label.set_visible(true);
                }
                None => {
                    // Desktop PCs have no backlight: hide the whole section.
                    brightness_row.set_visible(false);
                    display_label.set_visible(false);
                }
            }
            volume_row.set_visible(status.volume.is_some());
            sound_label.set_visible(status.volume.is_some());
            if let Some(v) = status.volume {
                volume.set_value(v.percent.min(100) as f64);
            }
            volume_icon_widget.set_icon_name(Some(volume_icon(status.volume)));
            updating.set(false);
        }
    };
    apply(&shell.status.current());
    shell.status.subscribe(apply);

    {
        let shell = Rc::clone(shell);
        popover.connect_show(move |_| shell.status.refresh(true));
    }
    popover
}

fn section_label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .css_classes(["cc-section"])
        .build()
}
