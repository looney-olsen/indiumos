//! User-facing shell settings (appearance, accent colour, dock).
//!
//! Defaults come from `/etc/indium/shell.ini`; the user's own changes live in
//! `~/.config/indium/shell.ini` and win over the system file.

use crate::ini::Ini;
use std::io;
use std::path::PathBuf;

pub const SYSTEM_CONFIG: &str = "/etc/indium/shell.ini";

/// Accent colours offered in Settings: (key, hex).
pub const ACCENTS: &[(&str, &str)] = &[
    ("blue", "#0a84ff"),
    ("indigo", "#5e5ce6"),
    ("purple", "#bf5af2"),
    ("pink", "#ff375f"),
    ("red", "#ff453a"),
    ("orange", "#ff9f0a"),
    ("green", "#30d158"),
    ("graphite", "#8e8e93"),
];

/// Desktop IDs pinned to the dock on a fresh install.
pub const DEFAULT_FAVORITES: &[&str] = &[
    "indium-browser.desktop",
    "thunar.desktop",
    "foot.desktop",
    "indium-settings.desktop",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Appearance {
    Light,
    Dark,
    Auto,
}

impl Appearance {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            "auto" => Some(Self::Auto),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Dark => "dark",
            Self::Auto => "auto",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellConfig {
    pub appearance: Appearance,
    /// One of the keys in [`ACCENTS`].
    pub accent: String,
    pub dock_icon_size: u32,
    pub dock_favorites: Vec<String>,
    pub clock_24h: bool,
    /// In Auto mode the theme is dark from this hour (0-23)...
    pub dark_from: u8,
    /// ...until this hour.
    pub dark_until: u8,
}

impl Default for ShellConfig {
    fn default() -> Self {
        ShellConfig {
            appearance: Appearance::Auto,
            accent: "blue".into(),
            dock_icon_size: 48,
            dock_favorites: DEFAULT_FAVORITES.iter().map(|s| s.to_string()).collect(),
            clock_24h: true,
            dark_from: 19,
            dark_until: 6,
        }
    }
}

pub fn user_config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    base.join("indium").join("shell.ini")
}

impl ShellConfig {
    pub fn from_ini(ini: &Ini) -> Self {
        let mut c = ShellConfig::default();
        if let Some(a) = ini.get("appearance", "mode").and_then(Appearance::parse) {
            c.appearance = a;
        }
        if let Some(a) = ini.get("appearance", "accent") {
            if ACCENTS.iter().any(|(k, _)| *k == a) {
                c.accent = a.to_string();
            }
        }
        if let Some(h) = ini.get_u32("appearance", "dark_from").filter(|h| *h < 24) {
            c.dark_from = h as u8;
        }
        if let Some(h) = ini.get_u32("appearance", "dark_until").filter(|h| *h < 24) {
            c.dark_until = h as u8;
        }
        if let Some(s) = ini.get_u32("dock", "icon_size") {
            c.dock_icon_size = s.clamp(32, 96);
        }
        if let Some(list) = ini.get_list("dock", "favorites") {
            c.dock_favorites = list;
        }
        if let Some(b) = ini.get_bool("bar", "clock_24h") {
            c.clock_24h = b;
        }
        c
    }

    /// Load system defaults, then the user's overrides.
    pub fn load() -> Self {
        let mut ini = Ini::load(std::path::Path::new(SYSTEM_CONFIG));
        ini.merge(&Ini::load(&user_config_path()));
        Self::from_ini(&ini)
    }

    /// Write into the user's file, preserving any keys this struct does not know.
    pub fn save(&self) -> io::Result<()> {
        let path = user_config_path();
        let mut ini = Ini::load(&path);
        ini.set("appearance", "mode", self.appearance.as_str());
        ini.set("appearance", "accent", &self.accent);
        ini.set("appearance", "dark_from", &self.dark_from.to_string());
        ini.set("appearance", "dark_until", &self.dark_until.to_string());
        ini.set("dock", "icon_size", &self.dock_icon_size.to_string());
        ini.set_list("dock", "favorites", &self.dock_favorites);
        ini.set_bool("bar", "clock_24h", self.clock_24h);
        ini.save(&path)
    }

    pub fn accent_hex(&self) -> &'static str {
        accent_hex(&self.accent)
    }

    /// Should the theme be dark at this hour of the day (0-23)?
    pub fn is_dark(&self, hour: u8) -> bool {
        match self.appearance {
            Appearance::Dark => true,
            Appearance::Light => false,
            Appearance::Auto => {
                if self.dark_from == self.dark_until {
                    false
                } else if self.dark_from < self.dark_until {
                    hour >= self.dark_from && hour < self.dark_until
                } else {
                    hour >= self.dark_from || hour < self.dark_until
                }
            }
        }
    }
}

pub fn accent_hex(key: &str) -> &'static str {
    ACCENTS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, hex)| *hex)
        .unwrap_or(ACCENTS[0].1)
}

/// CSS that recolours every libadwaita (GTK4) app to the chosen accent.
/// Written to `~/.config/gtk-4.0/gtk.css` by the shell.
pub fn gtk_accent_css(hex: &str) -> String {
    format!(
        "/* Generated by Indium OS. Change the accent in Settings > Appearance. */\n\
         @define-color accent_bg_color {hex};\n\
         @define-color accent_color {hex};\n\
         @define-color accent_fg_color #ffffff;\n\
         @define-color theme_selected_bg_color {hex};\n\
         @define-color theme_selected_fg_color #ffffff;\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_design_choices() {
        let c = ShellConfig::default();
        assert_eq!(c.appearance, Appearance::Auto);
        assert_eq!(c.accent, "blue");
        assert_eq!(c.accent_hex(), "#0a84ff");
        assert_eq!(c.dock_icon_size, 48);
        assert_eq!(c.dock_favorites[0], "indium-browser.desktop");
    }

    #[test]
    fn auto_mode_switches_overnight() {
        let c = ShellConfig::default(); // dark 19:00 - 06:00
        assert!(c.is_dark(23));
        assert!(c.is_dark(0));
        assert!(c.is_dark(5));
        assert!(!c.is_dark(6));
        assert!(!c.is_dark(12));
        assert!(c.is_dark(19));
        assert!(!c.is_dark(18));
    }

    #[test]
    fn fixed_modes_ignore_the_clock() {
        let light = ShellConfig {
            appearance: Appearance::Light,
            ..ShellConfig::default()
        };
        assert!(!light.is_dark(23));
        let dark = ShellConfig {
            appearance: Appearance::Dark,
            ..ShellConfig::default()
        };
        assert!(dark.is_dark(12));
    }

    #[test]
    fn invalid_values_are_ignored_and_ranges_clamped() {
        let ini = Ini::parse(
            "[appearance]\nmode=neon\naccent=chartreuse\ndark_from=40\n[dock]\nicon_size=500\n",
        );
        let c = ShellConfig::from_ini(&ini);
        assert_eq!(c.appearance, Appearance::Auto);
        assert_eq!(c.accent, "blue");
        assert_eq!(c.dark_from, 19);
        assert_eq!(c.dock_icon_size, 96);
    }

    #[test]
    fn explicit_empty_favorites_is_respected() {
        let ini = Ini::parse("[dock]\nfavorites=\n");
        assert!(ShellConfig::from_ini(&ini).dock_favorites.is_empty());
    }

    #[test]
    fn accent_css_names_the_libadwaita_colors() {
        let css = gtk_accent_css("#5e5ce6");
        assert!(css.contains("@define-color accent_bg_color #5e5ce6;"));
        assert!(css.contains("accent_color #5e5ce6"));
    }

    #[test]
    fn unknown_accent_falls_back_to_blue() {
        assert_eq!(accent_hex("nope"), "#0a84ff");
        assert_eq!(accent_hex("green"), "#30d158");
    }
}
