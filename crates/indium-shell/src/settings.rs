//! `indium-settings`: the System Settings app.
//!
//! Pages: Appearance, Performance (hardware profile and GPU/storage overrides),
//! Display, Network, Sound and About. It runs as its own on-demand process so
//! its memory is returned when it closes.

use crate::theme::Theme;
use crate::util::{run_blocking, spawn_detached, spawn_ui, Debounce};
use gtk::prelude::*;
use gtk::{gio, glib};
use indium_core::config::{Appearance, ShellConfig, ACCENTS};
use indium_core::profile::{
    HardwareProfile, Overrides, GPU_CHOICES, OVERRIDE_HELPER, PROFILE_CHOICES, STORAGE_CHOICES,
};
use indium_core::sys;
use std::cell::RefCell;
use std::process::Command;
use std::rc::Rc;

struct State {
    config: RefCell<ShellConfig>,
    profile: HardwareProfile,
    theme: Theme,
    save: Debounce,
}

impl State {
    /// A setting changed: restyle this window now and save shortly after.
    /// The running shell notices the saved file and restyles itself too.
    fn changed(&self) {
        let config = self.config.borrow().clone();
        self.theme.reload(&self.profile, &config);
        self.save.call(250, move || {
            if let Err(err) = config.save() {
                eprintln!("indium-settings: could not save: {err}");
            }
        });
    }
}

// ---------------------------------------------------------------- helpers

fn page(title: &str) -> (gtk::ScrolledWindow, gtk::Box) {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 14);
    content.add_css_class("settings-page");
    content.append(
        &gtk::Label::builder()
            .label(title)
            .xalign(0.0)
            .css_classes(["settings-title"])
            .build(),
    );
    let scroller = gtk::ScrolledWindow::builder()
        .child(&content)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .hexpand(true)
        .vexpand(true)
        .build();
    (scroller, content)
}

fn group(rows: Vec<gtk::Widget>) -> gtk::Box {
    let group = gtk::Box::new(gtk::Orientation::Vertical, 0);
    group.add_css_class("settings-group");
    for (i, row) in rows.iter().enumerate() {
        if i > 0 {
            let sep = gtk::Separator::new(gtk::Orientation::Horizontal);
            sep.add_css_class("settings-row-sep");
            group.append(&sep);
        }
        group.append(row);
    }
    group
}

fn row(title: &str, subtitle: &str, control: &impl IsA<gtk::Widget>) -> gtk::Widget {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 12);
    row.add_css_class("settings-row");
    let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
    text.set_hexpand(true);
    text.set_valign(gtk::Align::Center);
    text.append(&gtk::Label::builder().label(title).xalign(0.0).build());
    if !subtitle.is_empty() {
        text.append(
            &gtk::Label::builder()
                .label(subtitle)
                .xalign(0.0)
                .wrap(true)
                .css_classes(["settings-hint"])
                .build(),
        );
    }
    row.append(&text);
    control.set_valign(gtk::Align::Center);
    row.append(control);
    row.upcast()
}

fn value_label(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(1.0)
        .selectable(true)
        .wrap(true)
        .max_width_chars(34)
        .css_classes(["settings-value"])
        .build()
}

fn hint(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .css_classes(["settings-hint"])
        .build()
}

fn launch_button(label: &str, program: &'static str) -> gtk::Button {
    let button = gtk::Button::with_label(label);
    button.connect_clicked(move |_| spawn_detached(program, &[]));
    button
}

// ------------------------------------------------------------- appearance

fn appearance_page(state: &Rc<State>) -> gtk::ScrolledWindow {
    let (scroller, content) = page("Appearance");
    let config = state.config.borrow().clone();

    let mode = gtk::DropDown::from_strings(&["Light", "Dark", "Automatic"]);
    mode.set_selected(match config.appearance {
        Appearance::Light => 0,
        Appearance::Dark => 1,
        Appearance::Auto => 2,
    });
    {
        let state = Rc::clone(state);
        mode.connect_selected_notify(move |d| {
            state.config.borrow_mut().appearance = match d.selected() {
                0 => Appearance::Light,
                1 => Appearance::Dark,
                _ => Appearance::Auto,
            };
            state.changed();
        });
    }

    let swatches = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let mut first: Option<gtk::ToggleButton> = None;
    for (key, _) in ACCENTS {
        let button = gtk::ToggleButton::new();
        button.add_css_class("swatch");
        button.add_css_class(key);
        button.set_tooltip_text(Some(key));
        match &first {
            Some(group_leader) => button.set_group(Some(group_leader)),
            None => first = Some(button.clone()),
        }
        button.set_active(config.accent == *key);
        let state = Rc::clone(state);
        button.connect_toggled(move |b| {
            if b.is_active() {
                state.config.borrow_mut().accent = key.to_string();
                state.changed();
            }
        });
        swatches.append(&button);
    }

    let dock_size = gtk::Scale::with_range(gtk::Orientation::Horizontal, 32.0, 96.0, 4.0);
    dock_size.set_value(config.dock_icon_size as f64);
    dock_size.set_size_request(220, -1);
    dock_size.set_draw_value(true);
    dock_size.set_value_pos(gtk::PositionType::Right);
    {
        let state = Rc::clone(state);
        dock_size.connect_value_changed(move |s| {
            state.config.borrow_mut().dock_icon_size = s.value() as u32;
            state.changed();
        });
    }

    let clock = gtk::Switch::new();
    clock.set_active(config.clock_24h);
    {
        let state = Rc::clone(state);
        clock.connect_active_notify(move |s| {
            state.config.borrow_mut().clock_24h = s.is_active();
            state.changed();
        });
    }

    content.append(&group(vec![
        row(
            "Appearance",
            "Automatic switches to dark from 19:00 to 06:00.",
            &mode,
        ),
        row(
            "Accent colour",
            "Used for buttons, selections and highlights.",
            &swatches,
        ),
    ]));
    content.append(&group(vec![
        row("Dock icon size", "", &dock_size),
        row("24-hour clock", "", &clock),
    ]));
    scroller
}

// ------------------------------------------------------------ performance

fn dropdown_for(labels: &[&str], choices: &[&str], current: &str) -> gtk::DropDown {
    let dropdown = gtk::DropDown::from_strings(labels);
    dropdown.set_selected(choices.iter().position(|c| *c == current).unwrap_or(0) as u32);
    dropdown
}

/// Without a boot-time profile (for example when developing on another distro)
/// show what the running system reports instead of zeros.
fn fill_from_live_system(profile: &mut HardwareProfile) {
    if profile.ram_mb == 0 {
        profile.ram_mb = sys::meminfo().map(|m| m.total_mb).unwrap_or(0);
    }
    if profile.cpus <= 1 {
        let cores = std::thread::available_parallelism().map(|n| n.get() as u32);
        profile.cpus = cores.unwrap_or(1);
    }
}

fn performance_page() -> gtk::ScrolledWindow {
    let (scroller, content) = page("Performance");
    let mut profile = HardwareProfile::load();
    fill_from_live_system(&mut profile);
    let overrides = Overrides::load();

    let gpu_text = format!(
        "{} ({}), hardware rendering {}",
        profile.gpu_vendor,
        profile.gpu_driver,
        if profile.gpu_accel { "on" } else { "off" }
    );
    let profile_text = if profile.lowmem {
        format!("{} (low-memory mode)", profile.kind.label())
    } else {
        profile.kind.label().to_string()
    };
    content.append(&group(vec![
        row(
            "Hardware profile",
            "Chosen automatically at every boot.",
            &value_label(&profile_text),
        ),
        row(
            "Memory",
            "",
            &value_label(&format!("{} MB", profile.ram_mb)),
        ),
        row(
            "Processor cores",
            "",
            &value_label(&profile.cpus.to_string()),
        ),
        row("Graphics", "", &value_label(&gpu_text)),
        row("Storage", "", &value_label(&profile.storage)),
        row("Desktop engine", "", &value_label(&profile.compositor)),
    ]));

    let profile_choice = dropdown_for(
        &["Automatic", "Lite", "Standard", "Pro"],
        PROFILE_CHOICES,
        &overrides.profile,
    );
    let gpu_choice = dropdown_for(
        &[
            "Automatic",
            "Force hardware acceleration",
            "Software rendering only",
        ],
        GPU_CHOICES,
        &overrides.gpu,
    );
    let storage_choice = dropdown_for(
        &["Automatic", "Treat as SSD", "Treat as hard disk"],
        STORAGE_CHOICES,
        &overrides.storage,
    );

    content.append(&hint(
        "Override the automatic choices if Indium picked wrong for your machine. \
         Changes apply the next time you log in.",
    ));
    content.append(&group(vec![
        row(
            "Profile",
            "Lite is lightest. Pro enables blur and animations.",
            &profile_choice,
        ),
        row(
            "Graphics",
            "Force assumes the GPU works. Software is the safe mode for driver problems.",
            &gpu_choice,
        ),
        row(
            "Storage",
            "SSD turns on TRIM and a faster disk scheduler.",
            &storage_choice,
        ),
    ]));

    let apply = gtk::Button::with_label("Apply…");
    apply.add_css_class("suggested-action");
    apply.set_halign(gtk::Align::Start);
    let message = hint("");
    content.append(&apply);
    content.append(&message);

    apply.connect_clicked(move |button| {
        let chosen = Overrides {
            profile: PROFILE_CHOICES[profile_choice.selected() as usize].to_string(),
            gpu: GPU_CHOICES[gpu_choice.selected() as usize].to_string(),
            storage: STORAGE_CHOICES[storage_choice.selected() as usize].to_string(),
        };
        button.set_sensitive(false);
        message.remove_css_class("accent-ok");
        message.remove_css_class("accent-bad");
        message.set_text("Waiting for authentication…");
        let (button, message) = (button.clone(), message.clone());
        spawn_ui(async move {
            let args = chosen.helper_args();
            let result = run_blocking(move || {
                Command::new("pkexec")
                    .arg(OVERRIDE_HELPER)
                    .args(args)
                    .status()
            })
            .await;
            button.set_sensitive(true);
            match result {
                Some(Ok(status)) if status.success() => {
                    message.add_css_class("accent-ok");
                    message.set_text("Saved. Log out and back in to apply.");
                }
                Some(Ok(status)) if matches!(status.code(), Some(126) | Some(127)) => {
                    message.set_text("Cancelled. Nothing was changed.");
                }
                _ => {
                    message.add_css_class("accent-bad");
                    message.set_text("Could not save the change.");
                }
            }
        });
    });
    scroller
}

// ---------------------------------------------------------------- display

fn display_page() -> gtk::ScrolledWindow {
    let (scroller, content) = page("Display");

    let brightness = gtk::Scale::with_range(gtk::Orientation::Horizontal, 1.0, 100.0, 1.0);
    brightness.set_size_request(240, -1);
    brightness.set_sensitive(false);
    let status = hint("Looking for a backlight…");
    let arrange = launch_button("Arrange Displays…", "wdisplays");

    content.append(&group(vec![
        row("Brightness", "", &brightness),
        row(
            "Resolution and layout",
            "Scale, position and refresh rate.",
            &arrange,
        ),
    ]));
    content.append(&status);

    let updating = Rc::new(std::cell::Cell::new(true));
    {
        let updating = Rc::clone(&updating);
        let debounce = Debounce::new();
        brightness.connect_value_changed(move |s| {
            if updating.get() {
                return;
            }
            let value = s.value() as u8;
            debounce.call(120, move || {
                spawn_ui(async move {
                    run_blocking(move || sys::set_brightness(value)).await;
                });
            });
        });
    }
    {
        let brightness = brightness.clone();
        spawn_ui(async move {
            match run_blocking(sys::brightness).await.flatten() {
                Some(value) => {
                    brightness.set_value(value as f64);
                    brightness.set_sensitive(true);
                    status.set_text("");
                }
                None => status.set_text(
                    "No adjustable backlight found. Desktop monitors use their own buttons.",
                ),
            }
            updating.set(false);
        });
    }
    scroller
}

// ---------------------------------------------------------------- network

fn network_page() -> gtk::ScrolledWindow {
    let (scroller, content) = page("Network");

    let wifi = gtk::Switch::new();
    wifi.set_sensitive(false);
    let connection = value_label("Checking…");
    let editor = launch_button("Network Connections…", "nm-connection-editor");

    content.append(&group(vec![
        row("Wi-Fi", "", &wifi),
        row("Connected to", "", &connection),
        row(
            "Add or edit connections",
            "VPN, wired and saved Wi-Fi networks.",
            &editor,
        ),
    ]));

    let updating = Rc::new(std::cell::Cell::new(true));
    {
        let updating = Rc::clone(&updating);
        wifi.connect_active_notify(move |s| {
            if updating.get() {
                return;
            }
            let on = s.is_active();
            spawn_ui(async move {
                run_blocking(move || sys::set_wifi(on)).await;
            });
        });
    }
    {
        let wifi = wifi.clone();
        spawn_ui(async move {
            if let Some(net) = run_blocking(sys::network).await {
                wifi.set_active(net.wifi_enabled);
                wifi.set_sensitive(true);
                connection.set_text(&match (net.wifi_ssid, net.ethernet) {
                    (Some(ssid), _) => ssid,
                    (None, true) => "Wired connection".to_string(),
                    (None, false) => "Not connected".to_string(),
                });
            }
            updating.set(false);
        });
    }
    scroller
}

// ------------------------------------------------------------------ sound

fn sound_page() -> gtk::ScrolledWindow {
    let (scroller, content) = page("Sound");

    let volume = gtk::Scale::with_range(gtk::Orientation::Horizontal, 0.0, 100.0, 1.0);
    volume.set_size_request(240, -1);
    volume.set_sensitive(false);
    let mute = gtk::Switch::new();
    mute.set_sensitive(false);
    let mixer = launch_button("Sound Mixer…", "pavucontrol");

    content.append(&group(vec![
        row("Output volume", "", &volume),
        row("Mute", "", &mute),
        row(
            "Devices and apps",
            "Choose speakers, microphones and per-app volume.",
            &mixer,
        ),
    ]));

    let updating = Rc::new(std::cell::Cell::new(true));
    {
        let updating = Rc::clone(&updating);
        let debounce = Debounce::new();
        volume.connect_value_changed(move |s| {
            if updating.get() {
                return;
            }
            let value = s.value() as u8;
            debounce.call(80, move || {
                spawn_ui(async move {
                    run_blocking(move || sys::set_volume(value)).await;
                });
            });
        });
    }
    {
        let updating = Rc::clone(&updating);
        mute.connect_active_notify(move |s| {
            if updating.get() {
                return;
            }
            let wanted = s.is_active();
            spawn_ui(async move {
                run_blocking(move || {
                    if sys::volume().map(|v| v.muted) != Some(wanted) {
                        sys::toggle_mute();
                    }
                })
                .await;
            });
        });
    }
    {
        let (volume, mute) = (volume.clone(), mute.clone());
        spawn_ui(async move {
            if let Some(v) = run_blocking(sys::volume).await.flatten() {
                volume.set_value(v.percent.min(100) as f64);
                mute.set_active(v.muted);
                volume.set_sensitive(true);
                mute.set_sensitive(true);
            }
            updating.set(false);
        });
    }
    scroller
}

// ------------------------------------------------------------------ about

struct AboutInfo {
    os: String,
    kernel: String,
    cpu: String,
    memory: String,
    host: String,
}

fn read_about() -> AboutInfo {
    AboutInfo {
        os: sys::os_release("PRETTY_NAME").unwrap_or_else(|| "Indium OS".into()),
        kernel: sys::kernel_version().unwrap_or_default(),
        cpu: sys::cpu_model().unwrap_or_default(),
        memory: sys::meminfo()
            .map(|m| format!("{} MB ({} MB free)", m.total_mb, m.available_mb))
            .unwrap_or_default(),
        host: sys::hostname().unwrap_or_default(),
    }
}

fn about_page() -> gtk::ScrolledWindow {
    let (scroller, content) = page("About");
    let profile = HardwareProfile::load();

    let has_icon = gtk::gdk::Display::default()
        .map(|d| gtk::IconTheme::for_display(&d).has_icon("indium-os"))
        .unwrap_or(false);
    let logo = if has_icon {
        gtk::Image::from_icon_name("indium-os")
    } else {
        gtk::Image::from_icon_name("computer-symbolic")
    };
    logo.set_pixel_size(96);
    logo.set_margin_top(6);
    content.append(&logo);

    let os = gtk::Label::builder()
        .label("Indium OS")
        .css_classes(["settings-title"])
        .build();
    content.append(&os);

    let version = value_label("");
    let kernel = value_label("");
    let cpu = value_label("");
    let memory = value_label("");
    let host = value_label("");
    content.append(&group(vec![
        row("Version", "", &version),
        row("Device name", "", &host),
        row("Processor", "", &cpu),
        row("Memory", "", &memory),
        row(
            "Graphics",
            "",
            &value_label(&format!("{} ({})", profile.gpu_vendor, profile.gpu_driver)),
        ),
        row("Hardware profile", "", &value_label(profile.kind.label())),
        row("Kernel", "", &kernel),
        row("Shell", "", &value_label(indium_core::VERSION)),
    ]));

    spawn_ui(async move {
        if let Some(info) = run_blocking(read_about).await {
            version.set_text(&info.os);
            kernel.set_text(&info.kernel);
            cpu.set_text(&info.cpu);
            memory.set_text(&info.memory);
            host.set_text(&info.host);
        }
    });
    scroller
}

// ------------------------------------------------------------------- app

fn build_window(app: &gtk::Application) -> (gtk::ApplicationWindow, gtk::Stack) {
    let profile = HardwareProfile::load();
    let config = ShellConfig::load();
    let state = Rc::new(State {
        theme: Theme::install(&profile, &config),
        config: RefCell::new(config),
        profile,
        save: Debounce::new(),
    });

    let stack = gtk::Stack::new();
    stack.add_titled(&appearance_page(&state), Some("appearance"), "Appearance");
    stack.add_titled(&performance_page(), Some("performance"), "Performance");
    stack.add_titled(&display_page(), Some("display"), "Display");
    stack.add_titled(&network_page(), Some("network"), "Network");
    stack.add_titled(&sound_page(), Some("sound"), "Sound");
    stack.add_titled(&about_page(), Some("about"), "About");

    let sidebar = gtk::StackSidebar::new();
    sidebar.set_stack(&stack);
    sidebar.set_size_request(190, -1);

    let layout = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    layout.append(&sidebar);
    layout.append(&gtk::Separator::new(gtk::Orientation::Vertical));
    layout.append(&stack);

    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("System Settings")
        .default_width(820)
        .default_height(560)
        .child(&layout)
        .build();
    // Keep the state (and its theme providers) alive as long as the window exists.
    window.connect_destroy(move |_| drop(Rc::clone(&state)));
    (window, stack)
}

pub fn run() -> glib::ExitCode {
    let page_name = std::env::args().nth(1).unwrap_or_default();
    let app = gtk::Application::builder()
        .application_id("org.indium.Settings")
        .build();

    if let Err(err) = app.register(gio::Cancellable::NONE) {
        eprintln!("indium-settings: could not reach the session bus: {err}");
        return glib::ExitCode::FAILURE;
    }
    if app.is_remote() {
        // Already open: just show it, on the requested page.
        app.activate_action("show-page", Some(&page_name.to_variant()));
        if let Some(connection) = app.dbus_connection() {
            let _ = connection.flush_sync(gio::Cancellable::NONE);
        }
        std::process::exit(0);
    }

    let shown: Rc<RefCell<Option<(gtk::ApplicationWindow, gtk::Stack)>>> =
        Rc::new(RefCell::new(None));
    {
        let shown = Rc::clone(&shown);
        let action = gio::SimpleAction::new("show-page", Some(glib::VariantTy::STRING));
        action.connect_activate(move |_, parameter| {
            if let Some((window, stack)) = shown.borrow().as_ref() {
                if let Some(name) = parameter.and_then(|p| p.str().map(String::from)) {
                    if stack.child_by_name(&name).is_some() {
                        stack.set_visible_child_name(&name);
                    }
                }
                window.present();
            }
        });
        app.add_action(&action);
    }
    {
        let shown = Rc::clone(&shown);
        app.connect_activate(move |app| {
            if let Some((window, _)) = shown.borrow().as_ref() {
                window.present();
                return;
            }
            let (window, stack) = build_window(app);
            if stack.child_by_name(&page_name).is_some() {
                stack.set_visible_child_name(&page_name);
            }
            window.present();
            *shown.borrow_mut() = Some((window, stack));
        });
    }
    app.run_with_args::<&str>(&[])
}
