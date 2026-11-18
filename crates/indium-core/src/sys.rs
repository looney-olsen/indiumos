//! Readers and controllers for volume, brightness, battery, network and system info.
//!
//! Each reader is a thin command runner around a pure parser, so the parsing
//! is unit-tested without needing the real tools. These functions block; the UI
//! calls them from a worker thread.

use std::fs;
use std::process::{Command, Stdio};

fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

// ---------------------------------------------------------------- volume

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Volume {
    pub percent: u8,
    pub muted: bool,
}

/// Parse `wpctl get-volume` output such as `Volume: 0.45` or `Volume: 0.45 [MUTED]`.
pub fn parse_wpctl_volume(text: &str) -> Option<Volume> {
    let rest = text.trim().strip_prefix("Volume:")?.trim();
    let value: f64 = rest.split_whitespace().next()?.parse().ok()?;
    Some(Volume {
        percent: (value * 100.0).round().clamp(0.0, 150.0) as u8,
        muted: rest.contains("[MUTED]"),
    })
}

pub fn volume() -> Option<Volume> {
    parse_wpctl_volume(&run("wpctl", &["get-volume", "@DEFAULT_AUDIO_SINK@"])?)
}

pub fn set_volume(percent: u8) {
    let arg = format!("{}%", percent.min(100));
    run("wpctl", &["set-volume", "@DEFAULT_AUDIO_SINK@", &arg]);
}

pub fn toggle_mute() {
    run("wpctl", &["set-mute", "@DEFAULT_AUDIO_SINK@", "toggle"]);
}

// ------------------------------------------------------------ brightness

/// Parse `brightnessctl -m` output: `device,class,current,percent%,max`.
pub fn parse_brightnessctl(text: &str) -> Option<u8> {
    let line = text.lines().next()?;
    let percent = line.split(',').nth(3)?.trim().trim_end_matches('%');
    percent.parse::<u8>().ok().map(|p| p.min(100))
}

pub fn brightness() -> Option<u8> {
    parse_brightnessctl(&run("brightnessctl", &["-m"])?)
}

pub fn set_brightness(percent: u8) {
    // Never go fully black: at 0% some panels are unreadable.
    let arg = format!("{}%", percent.clamp(1, 100));
    run("brightnessctl", &["set", &arg]);
}

// --------------------------------------------------------------- battery

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Battery {
    pub percent: u8,
    /// On AC power (charging or full).
    pub plugged: bool,
}

pub fn parse_battery(capacity: &str, status: &str) -> Option<Battery> {
    let percent: u8 = capacity.trim().parse().ok()?;
    let plugged = matches!(status.trim(), "Charging" | "Full" | "Not charging");
    Some(Battery {
        percent: percent.min(100),
        plugged,
    })
}

pub fn battery() -> Option<Battery> {
    let mut names: Vec<String> = fs::read_dir("/sys/class/power_supply")
        .ok()?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("BAT"))
        .collect();
    names.sort();
    let name = names.first()?;
    let base = format!("/sys/class/power_supply/{name}");
    parse_battery(
        &fs::read_to_string(format!("{base}/capacity")).ok()?,
        &fs::read_to_string(format!("{base}/status")).unwrap_or_default(),
    )
}

/// Symbolic icon name for a battery reading.
pub fn battery_icon(b: Battery) -> String {
    let level = (b.percent / 10 * 10).min(100);
    if b.plugged {
        format!("battery-level-{level}-charging-symbolic")
    } else {
        format!("battery-level-{level}-symbolic")
    }
}

// --------------------------------------------------------------- network

/// Split a `nmcli -t` line on unescaped colons, undoing `\:` and `\\` escapes.
pub fn split_terse(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(next) = chars.next() {
                    fields.last_mut().expect("never empty").push(next);
                }
            }
            ':' => fields.push(String::new()),
            other => fields.last_mut().expect("never empty").push(other),
        }
    }
    fields
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Network {
    pub wifi_enabled: bool,
    pub wifi_ssid: Option<String>,
    pub ethernet: bool,
}

/// Parse `nmcli -t -f TYPE,STATE,CONNECTION device` output.
pub fn parse_devices(text: &str) -> (Option<String>, bool) {
    let mut ssid = None;
    let mut ethernet = false;
    for line in text.lines() {
        let f = split_terse(line);
        if f.len() < 2 || f[1] != "connected" {
            continue;
        }
        match f[0].as_str() {
            "wifi" => ssid = f.get(2).filter(|s| !s.is_empty()).cloned(),
            "ethernet" => ethernet = true,
            _ => {}
        }
    }
    (ssid, ethernet)
}

pub fn network() -> Network {
    let wifi_enabled = run("nmcli", &["radio", "wifi"])
        .map(|s| s.trim() == "enabled")
        .unwrap_or(false);
    let (wifi_ssid, ethernet) = run("nmcli", &["-t", "-f", "TYPE,STATE,CONNECTION", "device"])
        .map(|s| parse_devices(&s))
        .unwrap_or((None, false));
    Network {
        wifi_enabled,
        wifi_ssid,
        ethernet,
    }
}

pub fn set_wifi(on: bool) {
    run("nmcli", &["radio", "wifi", if on { "on" } else { "off" }]);
}

pub fn network_icon(n: &Network) -> &'static str {
    if n.wifi_ssid.is_some() {
        "network-wireless-signal-good-symbolic"
    } else if n.ethernet {
        "network-wired-symbolic"
    } else if n.wifi_enabled {
        "network-wireless-offline-symbolic"
    } else {
        "network-wireless-disabled-symbolic"
    }
}

// ------------------------------------------------------------- bluetooth

pub fn parse_bluetooth_powered(text: &str) -> bool {
    text.lines()
        .any(|l| l.trim().eq_ignore_ascii_case("Powered: yes"))
}

pub fn bluetooth_powered() -> bool {
    run("bluetoothctl", &["show"])
        .map(|s| parse_bluetooth_powered(&s))
        .unwrap_or(false)
}

pub fn set_bluetooth(on: bool) {
    run("bluetoothctl", &["power", if on { "on" } else { "off" }]);
}

// ----------------------------------------------------------- system info

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemInfo {
    pub total_mb: u64,
    pub available_mb: u64,
}

pub fn parse_meminfo(text: &str) -> Option<MemInfo> {
    let field = |name: &str| -> Option<u64> {
        text.lines()
            .find_map(|l| l.strip_prefix(name))?
            .trim_start_matches(':')
            .split_whitespace()
            .next()?
            .parse::<u64>()
            .ok()
    };
    Some(MemInfo {
        total_mb: field("MemTotal")? / 1024,
        available_mb: field("MemAvailable")? / 1024,
    })
}

pub fn meminfo() -> Option<MemInfo> {
    parse_meminfo(&fs::read_to_string("/proc/meminfo").ok()?)
}

pub fn parse_cpu_model(text: &str) -> Option<String> {
    text.lines()
        .find(|l| l.starts_with("model name"))
        .and_then(|l| l.split_once(':'))
        .map(|(_, v)| v.split_whitespace().collect::<Vec<_>>().join(" "))
}

pub fn cpu_model() -> Option<String> {
    parse_cpu_model(&fs::read_to_string("/proc/cpuinfo").ok()?)
}

/// Read a key such as `PRETTY_NAME` from os-release text.
pub fn parse_os_release(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
    })
}

pub fn os_release(key: &str) -> Option<String> {
    let text = fs::read_to_string("/etc/os-release").ok()?;
    parse_os_release(&text, key)
}

pub fn kernel_version() -> Option<String> {
    fs::read_to_string("/proc/sys/kernel/osrelease")
        .ok()
        .map(|s| s.trim().to_string())
}

pub fn hostname() -> Option<String> {
    fs::read_to_string("/etc/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_parsing() {
        assert_eq!(
            parse_wpctl_volume("Volume: 0.45\n"),
            Some(Volume {
                percent: 45,
                muted: false
            })
        );
        assert_eq!(
            parse_wpctl_volume("Volume: 1.00 [MUTED]"),
            Some(Volume {
                percent: 100,
                muted: true
            })
        );
        assert_eq!(parse_wpctl_volume("Volume: 2.5").unwrap().percent, 150);
        assert_eq!(parse_wpctl_volume("nonsense"), None);
        assert_eq!(parse_wpctl_volume(""), None);
    }

    #[test]
    fn brightness_parsing() {
        assert_eq!(
            parse_brightnessctl("amdgpu_bl1,backlight,1500,50%,3000\n"),
            Some(50)
        );
        assert_eq!(parse_brightnessctl("x,backlight,1,100%,1"), Some(100));
        assert_eq!(parse_brightnessctl("x,backlight,1"), None);
        assert_eq!(parse_brightnessctl(""), None);
    }

    #[test]
    fn battery_parsing_and_icons() {
        let b = parse_battery("87\n", "Discharging\n").unwrap();
        assert_eq!(
            b,
            Battery {
                percent: 87,
                plugged: false
            }
        );
        assert_eq!(battery_icon(b), "battery-level-80-symbolic");
        let c = parse_battery("100", "Full").unwrap();
        assert!(c.plugged);
        assert_eq!(battery_icon(c), "battery-level-100-charging-symbolic");
        assert_eq!(parse_battery("abc", "Full"), None);
        assert_eq!(
            battery_icon(Battery {
                percent: 5,
                plugged: false
            }),
            "battery-level-0-symbolic"
        );
    }

    #[test]
    fn nmcli_terse_escaping() {
        assert_eq!(
            split_terse("wifi:connected:Cafe"),
            vec!["wifi", "connected", "Cafe"]
        );
        assert_eq!(
            split_terse(r"wifi:connected:My\:Network"),
            vec!["wifi", "connected", "My:Network"]
        );
        assert_eq!(split_terse(r"a\\b:c"), vec![r"a\b", "c"]);
        assert_eq!(split_terse(""), vec![""]);
    }

    #[test]
    fn device_listing() {
        let text =
            "ethernet:unavailable:\nwifi:connected:Home WiFi\nloopback:connected (externally):lo\n";
        assert_eq!(parse_devices(text), (Some("Home WiFi".to_string()), false));
        let wired = "ethernet:connected:Wired connection 1\nwifi:disconnected:\n";
        assert_eq!(parse_devices(wired), (None, true));
        assert_eq!(parse_devices(""), (None, false));
    }

    #[test]
    fn network_icon_priority() {
        let mut n = Network {
            wifi_enabled: true,
            wifi_ssid: Some("x".into()),
            ethernet: true,
        };
        assert_eq!(network_icon(&n), "network-wireless-signal-good-symbolic");
        n.wifi_ssid = None;
        assert_eq!(network_icon(&n), "network-wired-symbolic");
        n.ethernet = false;
        assert_eq!(network_icon(&n), "network-wireless-offline-symbolic");
        n.wifi_enabled = false;
        assert_eq!(network_icon(&n), "network-wireless-disabled-symbolic");
    }

    #[test]
    fn bluetooth_parsing() {
        assert!(parse_bluetooth_powered(
            "Controller AA:BB\n\tPowered: yes\n\tAlias: x\n"
        ));
        assert!(!parse_bluetooth_powered(
            "Controller AA:BB\n\tPowered: no\n"
        ));
        assert!(!parse_bluetooth_powered(""));
    }

    #[test]
    fn meminfo_parsing() {
        let text = "MemTotal:        3700000 kB\nMemFree:  100 kB\nMemAvailable:    1800000 kB\n";
        let m = parse_meminfo(text).unwrap();
        assert_eq!(m.total_mb, 3613);
        assert_eq!(m.available_mb, 1757);
        assert_eq!(parse_meminfo("MemTotal: 1 kB"), None);
    }

    #[test]
    fn cpu_and_os_release_parsing() {
        assert_eq!(
            parse_cpu_model("processor : 0\nmodel name\t: AMD Ryzen 3  7320U  with Radeon\n"),
            Some("AMD Ryzen 3 7320U with Radeon".into())
        );
        let osr = "NAME=\"Indium OS\"\nPRETTY_NAME=\"Indium OS 0.1\"\nID=indium\n";
        assert_eq!(
            parse_os_release(osr, "PRETTY_NAME"),
            Some("Indium OS 0.1".into())
        );
        assert_eq!(parse_os_release(osr, "ID"), Some("indium".into()));
        assert_eq!(parse_os_release(osr, "MISSING"), None);
    }
}
