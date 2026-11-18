//! Loads the shell CSS and applies light/dark appearance and the accent colour.

use gtk::prelude::*;
use gtk::{gio, glib};
use indium_core::config::{gtk_accent_css, ShellConfig, ACCENTS};
use indium_core::profile::HardwareProfile;
use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;

const BASE_CSS: &str = include_str!("../../../data/theme/indium.css");

/// Colour tokens substituted into the base CSS for light or dark mode.
/// `translucent` is only used when the compositor can blur what is behind
/// the surface; otherwise surfaces stay nearly opaque so they remain readable.
fn tokens(dark: bool, translucent: bool) -> Vec<(&'static str, &'static str)> {
    match (dark, translucent) {
        (false, true) => vec![
            ("panel_bg", "rgba(246,246,246,0.72)"),
            ("panel_fg", "#1d1d1f"),
            ("panel_border", "rgba(0,0,0,0.10)"),
            ("hover", "rgba(0,0,0,0.08)"),
            ("pressed", "rgba(0,0,0,0.14)"),
            ("popover_bg", "rgba(250,250,250,0.92)"),
            ("card_bg", "rgba(250,250,250,0.90)"),
            ("dock_bg", "rgba(255,255,255,0.45)"),
            ("dock_border", "rgba(255,255,255,0.55)"),
            ("field_bg", "rgba(0,0,0,0.06)"),
            ("shadow", "rgba(0,0,0,0.28)"),
            ("sep", "rgba(0,0,0,0.15)"),
            ("group_bg", "rgba(0,0,0,0.04)"),
        ],
        (false, false) => vec![
            ("panel_bg", "rgba(246,246,246,0.97)"),
            ("panel_fg", "#1d1d1f"),
            ("panel_border", "rgba(0,0,0,0.10)"),
            ("hover", "rgba(0,0,0,0.08)"),
            ("pressed", "rgba(0,0,0,0.14)"),
            ("popover_bg", "rgba(250,250,250,0.99)"),
            ("card_bg", "rgba(250,250,250,0.98)"),
            ("dock_bg", "rgba(240,240,242,0.88)"),
            ("dock_border", "rgba(0,0,0,0.10)"),
            ("field_bg", "rgba(0,0,0,0.06)"),
            ("shadow", "rgba(0,0,0,0.24)"),
            ("sep", "rgba(0,0,0,0.15)"),
            ("group_bg", "rgba(0,0,0,0.04)"),
        ],
        (true, true) => vec![
            ("panel_bg", "rgba(30,30,32,0.72)"),
            ("panel_fg", "#f5f5f7"),
            ("panel_border", "rgba(255,255,255,0.10)"),
            ("hover", "rgba(255,255,255,0.12)"),
            ("pressed", "rgba(255,255,255,0.20)"),
            ("popover_bg", "rgba(40,40,44,0.92)"),
            ("card_bg", "rgba(44,44,48,0.90)"),
            ("dock_bg", "rgba(60,60,64,0.45)"),
            ("dock_border", "rgba(255,255,255,0.14)"),
            ("field_bg", "rgba(255,255,255,0.08)"),
            ("shadow", "rgba(0,0,0,0.50)"),
            ("sep", "rgba(255,255,255,0.18)"),
            ("group_bg", "rgba(255,255,255,0.06)"),
        ],
        (true, false) => vec![
            ("panel_bg", "rgba(30,30,32,0.97)"),
            ("panel_fg", "#f5f5f7"),
            ("panel_border", "rgba(255,255,255,0.10)"),
            ("hover", "rgba(255,255,255,0.12)"),
            ("pressed", "rgba(255,255,255,0.20)"),
            ("popover_bg", "rgba(40,40,44,0.99)"),
            ("card_bg", "rgba(44,44,48,0.98)"),
            ("dock_bg", "rgba(52,52,56,0.90)"),
            ("dock_border", "rgba(255,255,255,0.12)"),
            ("field_bg", "rgba(255,255,255,0.08)"),
            ("shadow", "rgba(0,0,0,0.45)"),
            ("sep", "rgba(255,255,255,0.18)"),
            ("group_bg", "rgba(255,255,255,0.06)"),
        ],
    }
}

/// Produce the final CSS for a look. Pure function, unit-tested.
pub fn build_css(accent_hex: &str, dark: bool, translucent: bool) -> String {
    let mut css = BASE_CSS.to_string();
    css = css.replace("@@accent@@", accent_hex);
    for (name, value) in tokens(dark, translucent) {
        css = css.replace(&format!("@@{name}@@"), value);
    }
    let swatches: String = ACCENTS
        .iter()
        .map(|(key, hex)| format!(".swatch.{key} {{ background-color: {hex}; }}\n"))
        .collect();
    css.replace("/* @@swatches@@ */", &swatches)
}

fn user_css_path() -> PathBuf {
    indium_core::config::user_config_path().with_file_name("user.css")
}

pub fn current_hour() -> u8 {
    glib::DateTime::now_local()
        .map(|t| t.hour() as u8)
        .unwrap_or(12)
}

/// Owns the CSS providers installed on the display.
pub struct Theme {
    provider: gtk::CssProvider,
    user_provider: gtk::CssProvider,
    last_dark: RefCell<Option<bool>>,
}

impl Theme {
    /// Install the shell CSS (and the user's optional `~/.config/indium/user.css`).
    pub fn install(profile: &HardwareProfile, config: &ShellConfig) -> Theme {
        let theme = Theme {
            provider: gtk::CssProvider::new(),
            user_provider: gtk::CssProvider::new(),
            last_dark: RefCell::new(None),
        };
        if let Some(display) = gtk::gdk::Display::default() {
            gtk::style_context_add_provider_for_display(
                &display,
                &theme.provider,
                gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
            );
            gtk::style_context_add_provider_for_display(
                &display,
                &theme.user_provider,
                gtk::STYLE_PROVIDER_PRIORITY_USER,
            );
        }
        theme.reload(profile, config);
        theme
    }

    /// Re-read config and user.css, then restyle everything.
    pub fn reload(&self, profile: &HardwareProfile, config: &ShellConfig) {
        let dark = config.is_dark(current_hour());
        self.provider
            .load_from_string(&build_css(config.accent_hex(), dark, profile.blur));
        let user_css = fs::read_to_string(user_css_path()).unwrap_or_default();
        self.user_provider.load_from_string(&user_css);
        apply_system_appearance(profile, config, dark);
        *self.last_dark.borrow_mut() = Some(dark);
    }

    /// Cheap check used by a timer: reload only when auto mode crosses sunset/sunrise.
    pub fn refresh_if_needed(&self, profile: &HardwareProfile, config: &ShellConfig) {
        let dark = config.is_dark(current_hour());
        if *self.last_dark.borrow() != Some(dark) {
            self.reload(profile, config);
        }
    }
}

/// Tell GTK, libadwaita apps and the desktop portal about dark mode, and
/// write the accent colour for other GTK apps.
pub fn apply_system_appearance(profile: &HardwareProfile, config: &ShellConfig, dark: bool) {
    if let Some(settings) = gtk::Settings::default() {
        settings.set_gtk_application_prefer_dark_theme(dark);
        settings.set_gtk_enable_animations(profile.animations_enabled());
    }

    // libadwaita and portal-aware apps follow this key.
    let schema = "org.gnome.desktop.interface";
    let has_schema = gio::SettingsSchemaSource::default()
        .and_then(|source| source.lookup(schema, true))
        .is_some();
    if has_schema {
        let settings = gio::Settings::new(schema);
        let value = if dark { "prefer-dark" } else { "prefer-light" };
        if settings.string("color-scheme") != value {
            let _ = settings.set_string("color-scheme", value);
        }
    }

    write_accent_css(config.accent_hex());
}

/// Write `~/.config/gtk-{3,4}.0/gtk.css` so other apps use the same accent.
fn write_accent_css(hex: &str) {
    let base = indium_core::config::user_config_path();
    let Some(config_dir) = base.parent().and_then(|p| p.parent()) else {
        return;
    };
    let css = gtk_accent_css(hex);
    for dir in ["gtk-4.0", "gtk-3.0"] {
        let path = config_dir.join(dir).join("gtk.css");
        if fs::read_to_string(&path).ok().as_deref() == Some(css.as_str()) {
            continue;
        }
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        let _ = fs::write(&path, &css);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_token_is_substituted_in_every_mode() {
        for dark in [false, true] {
            for translucent in [false, true] {
                let css = build_css("#0a84ff", dark, translucent);
                assert!(
                    !css.contains("@@"),
                    "unreplaced token in dark={dark} translucent={translucent}: {}",
                    css.lines().find(|l| l.contains("@@")).unwrap_or("")
                );
            }
        }
    }

    #[test]
    fn accent_and_swatches_are_injected() {
        let css = build_css("#5e5ce6", false, true);
        assert!(css.contains("#5e5ce6"));
        assert!(css.contains(".swatch.graphite"));
        assert!(css.contains("background-color: #0a84ff"));
    }

    #[test]
    fn dark_and_light_differ_and_opaque_is_more_opaque() {
        let light = build_css("#0a84ff", false, true);
        let dark = build_css("#0a84ff", true, true);
        assert_ne!(light, dark);
        assert!(light.contains("#1d1d1f"));
        assert!(dark.contains("#f5f5f7"));
        let flat = build_css("#0a84ff", false, false);
        assert!(flat.contains("rgba(246,246,246,0.97)"));
        assert!(light.contains("rgba(246,246,246,0.72)"));
    }

    #[test]
    fn both_token_tables_define_the_same_names() {
        let names = |dark, tr| -> Vec<&'static str> {
            tokens(dark, tr).into_iter().map(|(n, _)| n).collect()
        };
        let reference = names(false, true);
        assert_eq!(names(false, false), reference);
        assert_eq!(names(true, true), reference);
        assert_eq!(names(true, false), reference);
    }
}
