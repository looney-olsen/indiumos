//! The top menu bar: system menu, active app name, status icons, control
//! center and clock.

use crate::control_center;
use crate::layer::{self, Keyboard, Layer, Spec, Zone};
use crate::shell::Shell;
use crate::status::Status;
use gtk::prelude::*;
use gtk::{gio, glib};
use indium_core::config::ShellConfig;
use indium_core::sys;
use std::rc::Rc;
use std::time::Duration;

fn volume_icon(status: &Status) -> &'static str {
    match status.volume {
        None => "audio-volume-muted-symbolic",
        Some(v) if v.muted || v.percent == 0 => "audio-volume-muted-symbolic",
        Some(v) if v.percent < 34 => "audio-volume-low-symbolic",
        Some(v) if v.percent < 67 => "audio-volume-medium-symbolic",
        Some(_) => "audio-volume-high-symbolic",
    }
}

fn system_menu() -> gio::Menu {
    let menu = gio::Menu::new();
    let about = gio::Menu::new();
    about.append(Some("About This Computer"), Some("app.about"));
    menu.append_section(None, &about);
    let settings = gio::Menu::new();
    settings.append(Some("System Settings…"), Some("app.settings"));
    menu.append_section(None, &settings);
    let session = gio::Menu::new();
    session.append(Some("Lock Screen"), Some("app.lock"));
    session.append(Some("Sleep"), Some("app.sleep"));
    menu.append_section(None, &session);
    let power = gio::Menu::new();
    power.append(Some("Restart…"), Some("app.restart"));
    power.append(Some("Shut Down…"), Some("app.shutdown"));
    power.append(Some("Log Out…"), Some("app.logout"));
    menu.append_section(None, &power);
    menu
}

/// The logo image if the icon is installed, otherwise a text mark (development builds).
fn logo_widget() -> gtk::Widget {
    let has_icon = gtk::gdk::Display::default()
        .map(|d| gtk::IconTheme::for_display(&d).has_icon("indium-os-symbolic"))
        .unwrap_or(false);
    if has_icon {
        gtk::Image::from_icon_name("indium-os-symbolic").upcast()
    } else {
        gtk::Label::new(Some("In")).upcast()
    }
}

fn clock_text(config: &ShellConfig) -> String {
    let format = if config.clock_24h {
        "%a %e %b  %H:%M"
    } else {
        "%a %e %b  %l:%M %p"
    };
    glib::DateTime::now_local()
        .and_then(|now| now.format(format))
        .map(|s| s.split_whitespace().collect::<Vec<_>>().join(" "))
        .unwrap_or_default()
}

/// Update the clock now and again at the start of every minute.
fn run_clock(shell: &Rc<Shell>, label: &gtk::Label) {
    label.set_text(&clock_text(&shell.config.borrow()));
    let seconds_left = glib::DateTime::now_local()
        .map(|now| 60 - now.second() as u64)
        .unwrap_or(30)
        .max(1);
    let (shell, label) = (Rc::clone(shell), label.clone());
    glib::timeout_add_local_once(Duration::from_secs(seconds_left), move || {
        run_clock(&shell, &label)
    });
}

pub fn build(shell: &Rc<Shell>) -> gtk::ApplicationWindow {
    let window = gtk::ApplicationWindow::builder()
        .application(&shell.app)
        .decorated(false)
        .resizable(false)
        .build();
    window.add_css_class("indium-bar");
    window.add_css_class(shell.profile.effects_class());
    let is_layer = layer::apply(
        &window,
        &Spec {
            top: true,
            left: true,
            right: true,
            zone: Zone::Auto,
            keyboard: Keyboard::OnDemand,
            ..Spec::new("indium-bar", Layer::Top)
        },
    );
    if !is_layer {
        window.set_default_size(1100, 28);
    }

    let bar = gtk::CenterBox::new();
    bar.add_css_class("bar");
    window.set_child(Some(&bar));

    // ---- left: system menu and the active app
    let start = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    let logo = gtk::MenuButton::new();
    logo.set_child(Some(&logo_widget()));
    logo.set_menu_model(Some(&system_menu()));
    if let Some(popover) = logo.popover() {
        popover.add_css_class("indium");
    }
    start.append(&logo);

    let app_name = gtk::Label::new(Some("Desktop"));
    app_name.add_css_class("app-name");
    start.append(&app_name);
    bar.set_start_widget(Some(&start));

    let refresh_name = {
        let (shell, label) = (Rc::clone(shell), app_name.clone());
        move || {
            let name = match shell.toplevels.active() {
                Some(w) => shell
                    .app_index(&w.app_id)
                    .map(|i| shell.metas.borrow()[i].name.clone())
                    .or_else(|| (!w.app_id.is_empty()).then(|| w.app_id.clone()))
                    .unwrap_or_else(|| w.title.clone()),
                None => "Desktop".to_string(),
            };
            label.set_text(&name);
        }
    };
    refresh_name();
    shell.toplevels.on_change(refresh_name.clone());
    shell.apps_changed.add(move |_| refresh_name());

    // ---- right: battery, control center, clock
    let end = gtk::Box::new(gtk::Orientation::Horizontal, 0);

    let battery_box = gtk::Box::new(gtk::Orientation::Horizontal, 4);
    let battery_icon = gtk::Image::new();
    let battery_label = gtk::Label::new(None);
    battery_label.add_css_class("battery-label");
    battery_box.append(&battery_icon);
    battery_box.append(&battery_label);
    battery_box.set_visible(false);
    battery_box.set_margin_end(6);
    end.append(&battery_box);

    let cc_button = gtk::MenuButton::new();
    let cc_icons = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let network_icon = gtk::Image::from_icon_name("network-wireless-offline-symbolic");
    let sound_icon = gtk::Image::from_icon_name("audio-volume-medium-symbolic");
    cc_icons.append(&network_icon);
    cc_icons.append(&sound_icon);
    cc_button.set_child(Some(&cc_icons));
    cc_button.set_popover(Some(&control_center::build(shell)));
    end.append(&cc_button);

    let clock_button = gtk::MenuButton::new();
    let clock = gtk::Label::new(None);
    clock.add_css_class("clock");
    clock_button.set_child(Some(&clock));
    let calendar_popover = gtk::Popover::new();
    calendar_popover.add_css_class("indium");
    calendar_popover.set_child(Some(&gtk::Calendar::new()));
    clock_button.set_popover(Some(&calendar_popover));
    end.append(&clock_button);
    run_clock(shell, &clock);
    {
        // Settings can switch 12/24-hour format while the bar is running.
        let (shell_for_clock, clock) = (Rc::clone(shell), clock.clone());
        shell.config_changed.add(move |_| {
            clock.set_text(&clock_text(&shell_for_clock.config.borrow()));
        });
    }
    bar.set_end_widget(Some(&end));

    let apply_status = move |status: &Status| {
        match status.battery {
            Some(b) => {
                battery_icon.set_icon_name(Some(&sys::battery_icon(b)));
                battery_label.set_text(&format!("{}%", b.percent));
                battery_box.set_visible(true);
            }
            None => battery_box.set_visible(false),
        }
        network_icon.set_icon_name(Some(sys::network_icon(&status.network)));
        sound_icon.set_icon_name(Some(volume_icon(status)));
        sound_icon.set_visible(status.volume.is_some());
    };
    apply_status(&shell.status.current());
    shell.status.subscribe(apply_status);

    window.present();
    window
}
