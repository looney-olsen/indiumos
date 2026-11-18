//! The hardware profile chosen at boot by `indium-hwprofile`.
//!
//! `indium-hwprofile` writes `/etc/indium/profile` (shell `KEY=VALUE` lines).
//! The shell reads it to scale its own effects to the machine.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub const PROFILE_PATH: &str = "/etc/indium/profile";
pub const OVERRIDE_PATH: &str = "/etc/indium/override.conf";
/// Privileged helper that validates and writes the override file (run via pkexec).
pub const OVERRIDE_HELPER: &str = "/usr/lib/indium/indium-override";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileKind {
    Lite,
    Standard,
    Pro,
}

impl ProfileKind {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "lite" => Some(Self::Lite),
            "standard" => Some(Self::Standard),
            "pro" => Some(Self::Pro),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lite => "lite",
            Self::Standard => "standard",
            Self::Pro => "pro",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Lite => "Lite",
            Self::Standard => "Standard",
            Self::Pro => "Pro",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effects {
    None,
    Light,
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Animations {
    Off,
    Short,
    Full,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardwareProfile {
    pub kind: ProfileKind,
    pub lowmem: bool,
    pub ram_mb: u64,
    pub cpus: u32,
    pub gpu_vendor: String,
    pub gpu_driver: String,
    pub gpu_accel: bool,
    pub storage: String,
    pub compositor: String,
    pub effects: Effects,
    pub animations: Animations,
    pub blur: bool,
}

impl Default for HardwareProfile {
    /// Used when `/etc/indium/profile` does not exist (for example when
    /// developing on another distribution): behave like the Standard profile.
    fn default() -> Self {
        Self::from_kind(ProfileKind::Standard)
    }
}

impl HardwareProfile {
    fn from_kind(kind: ProfileKind) -> Self {
        let (effects, animations, blur) = match kind {
            ProfileKind::Lite => (Effects::None, Animations::Off, false),
            ProfileKind::Standard => (Effects::Light, Animations::Short, false),
            ProfileKind::Pro => (Effects::Full, Animations::Full, true),
        };
        HardwareProfile {
            kind,
            lowmem: false,
            ram_mb: 0,
            cpus: 1,
            gpu_vendor: "unknown".into(),
            gpu_driver: "none".into(),
            gpu_accel: false,
            storage: "unknown".into(),
            compositor: "labwc".into(),
            effects,
            animations,
            blur,
        }
    }

    pub fn parse(text: &str) -> Self {
        let env = parse_env(text);
        let get = |k: &str| env.get(k).map(String::as_str);

        let kind = get("INDIUM_PROFILE")
            .and_then(ProfileKind::parse)
            .unwrap_or(ProfileKind::Standard);
        let mut p = Self::from_kind(kind);

        p.lowmem = get("INDIUM_LOWMEM") == Some("1");
        p.ram_mb = get("INDIUM_RAM_MB")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        p.cpus = get("INDIUM_CPUS").and_then(|v| v.parse().ok()).unwrap_or(1);
        if let Some(v) = get("INDIUM_GPU_VENDOR") {
            p.gpu_vendor = v.into();
        }
        if let Some(v) = get("INDIUM_GPU_DRIVER") {
            p.gpu_driver = v.into();
        }
        p.gpu_accel = get("INDIUM_GPU_ACCEL") == Some("1");
        if let Some(v) = get("INDIUM_STORAGE") {
            p.storage = v.into();
        }
        if let Some(v) = get("INDIUM_COMPOSITOR") {
            p.compositor = v.into();
        }
        p.effects = match get("INDIUM_EFFECTS") {
            Some("none") => Effects::None,
            Some("light") => Effects::Light,
            Some("full") => Effects::Full,
            _ => p.effects,
        };
        p.animations = match get("INDIUM_ANIMATIONS") {
            Some("off") => Animations::Off,
            Some("short") => Animations::Short,
            Some("full") => Animations::Full,
            _ => p.animations,
        };
        p.blur = match get("INDIUM_BLUR") {
            Some("on") => true,
            Some("off") => false,
            _ => p.blur,
        };
        p
    }

    pub fn load_from(path: &Path) -> Self {
        fs::read_to_string(path)
            .map(|t| Self::parse(&t))
            .unwrap_or_default()
    }

    pub fn load() -> Self {
        Self::load_from(Path::new(PROFILE_PATH))
    }

    /// Seconds between status polls (volume, battery, network).
    pub fn poll_seconds(&self) -> u32 {
        match self.kind {
            ProfileKind::Lite => 10,
            ProfileKind::Standard => 6,
            ProfileKind::Pro => 4,
        }
    }

    /// Whether the dock should enlarge icons under the pointer.
    pub fn dock_magnification(&self) -> bool {
        self.kind == ProfileKind::Pro
    }

    /// CSS class describing how rich the visual effects should be.
    pub fn effects_class(&self) -> &'static str {
        match self.effects {
            Effects::None => "fx-none",
            Effects::Light => "fx-light",
            Effects::Full => "fx-full",
        }
    }

    pub fn animations_enabled(&self) -> bool {
        self.animations != Animations::Off
    }
}

/// Parse `KEY=VALUE` lines, ignoring comments and blank lines and stripping quotes.
pub fn parse_env(text: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            let value = value.trim().trim_matches(|c| c == '"' || c == '\'');
            map.insert(key.trim().to_string(), value.to_string());
        }
    }
    map
}

/// The user's manual overrides from `/etc/indium/override.conf`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Overrides {
    pub profile: String,
    pub gpu: String,
    pub storage: String,
}

pub const PROFILE_CHOICES: &[&str] = &["auto", "lite", "standard", "pro"];
pub const GPU_CHOICES: &[&str] = &["auto", "force", "off"];
pub const STORAGE_CHOICES: &[&str] = &["auto", "ssd", "hdd"];

impl Default for Overrides {
    fn default() -> Self {
        Overrides {
            profile: "auto".into(),
            gpu: "auto".into(),
            storage: "auto".into(),
        }
    }
}

impl Overrides {
    pub fn parse(text: &str) -> Self {
        let env = parse_env(text);
        let pick = |key: &str, allowed: &[&str]| -> String {
            env.get(key)
                .filter(|v| allowed.contains(&v.as_str()))
                .cloned()
                .unwrap_or_else(|| "auto".into())
        };
        Overrides {
            profile: pick("profile", PROFILE_CHOICES),
            gpu: pick("gpu", GPU_CHOICES),
            storage: pick("storage", STORAGE_CHOICES),
        }
    }

    pub fn load() -> Self {
        fs::read_to_string(OVERRIDE_PATH)
            .map(|t| Self::parse(&t))
            .unwrap_or_default()
    }

    /// Arguments for the privileged helper: `["profile=lite", "gpu=force", ...]`.
    pub fn helper_args(&self) -> Vec<String> {
        vec![
            format!("profile={}", self.profile),
            format!("gpu={}", self.gpu),
            format!("storage={}", self.storage),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const IDEAPAD: &str = "\
INDIUM_PROFILE=lite
INDIUM_LOWMEM=0
INDIUM_RAM_MB=3613
INDIUM_CPUS=4
INDIUM_GPU_VENDOR=amd
INDIUM_GPU_DRIVER=amdgpu
INDIUM_GPU_ACCEL=1
INDIUM_STORAGE=nvme
INDIUM_COMPOSITOR=labwc
INDIUM_EFFECTS=none
INDIUM_ANIMATIONS=off
INDIUM_BLUR=off
WLR_RENDERER=gles2
";

    #[test]
    fn parses_what_hwprofile_writes() {
        let p = HardwareProfile::parse(IDEAPAD);
        assert_eq!(p.kind, ProfileKind::Lite);
        assert_eq!(p.ram_mb, 3613);
        assert_eq!(p.cpus, 4);
        assert!(p.gpu_accel);
        assert_eq!(p.gpu_vendor, "amd");
        assert_eq!(p.storage, "nvme");
        assert_eq!(p.effects, Effects::None);
        assert!(!p.animations_enabled());
        assert!(!p.blur);
        assert_eq!(p.effects_class(), "fx-none");
        assert_eq!(p.poll_seconds(), 10);
        assert!(!p.dock_magnification());
    }

    #[test]
    fn pro_enables_everything() {
        let p = HardwareProfile::parse("INDIUM_PROFILE=pro\nINDIUM_BLUR=on\n");
        assert_eq!(p.kind, ProfileKind::Pro);
        assert_eq!(p.effects, Effects::Full);
        assert!(p.blur);
        assert!(p.dock_magnification());
        assert!(p.animations_enabled());
    }

    #[test]
    fn garbage_and_missing_input_fall_back_to_standard() {
        assert_eq!(HardwareProfile::parse("").kind, ProfileKind::Standard);
        assert_eq!(
            HardwareProfile::parse("INDIUM_PROFILE=banana\n").kind,
            ProfileKind::Standard
        );
        assert_eq!(
            HardwareProfile::load_from(Path::new("/definitely/not/here")),
            HardwareProfile::default()
        );
    }

    #[test]
    fn env_parser_strips_quotes_and_comments() {
        let env = parse_env("# hi\nA=\"x y\"\nB='z'\n\nC=3\n");
        assert_eq!(env["A"], "x y");
        assert_eq!(env["B"], "z");
        assert_eq!(env["C"], "3");
    }

    #[test]
    fn overrides_reject_unknown_values() {
        let o = Overrides::parse("profile=lite\ngpu=force\nstorage=floppy\n");
        assert_eq!(o.profile, "lite");
        assert_eq!(o.gpu, "force");
        assert_eq!(o.storage, "auto");
        assert_eq!(
            o.helper_args(),
            vec!["profile=lite", "gpu=force", "storage=auto"]
        );
    }
}
