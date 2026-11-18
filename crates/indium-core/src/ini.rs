//! A tiny INI reader/writer used for the shell configuration.
//!
//! Format: `[section]` headers, `key=value` lines, `#` or `;` comments.
//! Lists are stored GKeyFile-style as `a;b;c;`.

use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Ini {
    sections: Vec<Section>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Section {
    name: String,
    entries: Vec<(String, String)>,
}

impl Ini {
    pub fn parse(text: &str) -> Ini {
        let mut ini = Ini::default();
        let mut current = String::new();
        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if let Some(rest) = line.strip_prefix('[') {
                if let Some(name) = rest.strip_suffix(']') {
                    current = name.trim().to_string();
                    ini.section_mut(&current);
                    continue;
                }
            }
            if let Some((key, value)) = line.split_once('=') {
                ini.set(&current, key.trim(), value.trim());
            }
        }
        ini
    }

    /// Load a file; a missing or unreadable file yields an empty `Ini`.
    pub fn load(path: &Path) -> Ini {
        fs::read_to_string(path)
            .map(|text| Ini::parse(&text))
            .unwrap_or_default()
    }

    /// Save atomically (write to a temporary file, then rename).
    pub fn save(&self, path: &Path) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let tmp = path.with_extension("ini.tmp");
        fs::write(&tmp, self.to_string())?;
        fs::rename(&tmp, path)
    }

    fn section_mut(&mut self, name: &str) -> &mut Section {
        if let Some(i) = self.sections.iter().position(|s| s.name == name) {
            return &mut self.sections[i];
        }
        self.sections.push(Section {
            name: name.to_string(),
            entries: Vec::new(),
        });
        self.sections.last_mut().expect("just pushed")
    }

    pub fn get(&self, section: &str, key: &str) -> Option<&str> {
        self.sections
            .iter()
            .find(|s| s.name == section)?
            .entries
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn set(&mut self, section: &str, key: &str, value: &str) {
        let sec = self.section_mut(section);
        match sec.entries.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = value.to_string(),
            None => sec.entries.push((key.to_string(), value.to_string())),
        }
    }

    pub fn get_bool(&self, section: &str, key: &str) -> Option<bool> {
        match self.get(section, key)?.to_ascii_lowercase().as_str() {
            "true" | "yes" | "1" | "on" => Some(true),
            "false" | "no" | "0" | "off" => Some(false),
            _ => None,
        }
    }

    pub fn set_bool(&mut self, section: &str, key: &str, value: bool) {
        self.set(section, key, if value { "true" } else { "false" });
    }

    pub fn get_u32(&self, section: &str, key: &str) -> Option<u32> {
        self.get(section, key)?.parse().ok()
    }

    pub fn get_list(&self, section: &str, key: &str) -> Option<Vec<String>> {
        let value = self.get(section, key)?;
        Some(
            value
                .split(';')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
                .collect(),
        )
    }

    pub fn set_list<S: AsRef<str>>(&mut self, section: &str, key: &str, items: &[S]) {
        let mut joined = String::new();
        for item in items {
            joined.push_str(item.as_ref());
            joined.push(';');
        }
        self.set(section, key, &joined);
    }

    /// Copy every key from `other` over this one (other wins).
    pub fn merge(&mut self, other: &Ini) {
        for section in &other.sections {
            for (key, value) in &section.entries {
                self.set(&section.name, key, value);
            }
        }
    }
}

impl fmt::Display for Ini {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for section in &self.sections {
            if section.entries.is_empty() && section.name.is_empty() {
                continue;
            }
            if !first {
                writeln!(f)?;
            }
            first = false;
            if !section.name.is_empty() {
                writeln!(f, "[{}]", section.name)?;
            }
            for (key, value) in &section.entries {
                writeln!(f, "{key}={value}")?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sections_comments_and_whitespace() {
        let ini = Ini::parse(
            "# c\n[dock]\n icon_size = 52 \n; x\nfavorites=a;b;\n\n[bar]\nclock_24h=no\n",
        );
        assert_eq!(ini.get("dock", "icon_size"), Some("52"));
        assert_eq!(ini.get_u32("dock", "icon_size"), Some(52));
        assert_eq!(ini.get_bool("bar", "clock_24h"), Some(false));
        assert_eq!(ini.get("dock", "missing"), None);
        assert_eq!(ini.get("nope", "x"), None);
    }

    #[test]
    fn lists_round_trip() {
        let mut ini = Ini::default();
        ini.set_list("dock", "favorites", &["a.desktop", "b.desktop"]);
        assert_eq!(ini.get("dock", "favorites"), Some("a.desktop;b.desktop;"));
        assert_eq!(
            ini.get_list("dock", "favorites"),
            Some(vec!["a.desktop".to_string(), "b.desktop".to_string()])
        );
        ini.set_list::<&str>("dock", "favorites", &[]);
        assert_eq!(ini.get_list("dock", "favorites"), Some(vec![]));
    }

    #[test]
    fn set_overwrites_and_merge_prefers_other() {
        let mut a = Ini::parse("[x]\nk=1\nj=2\n");
        let b = Ini::parse("[x]\nk=9\n[y]\nz=3\n");
        a.merge(&b);
        assert_eq!(a.get("x", "k"), Some("9"));
        assert_eq!(a.get("x", "j"), Some("2"));
        assert_eq!(a.get("y", "z"), Some("3"));
    }

    #[test]
    fn display_then_parse_is_stable() {
        let mut ini = Ini::default();
        ini.set("appearance", "mode", "dark");
        ini.set("dock", "icon_size", "48");
        let again = Ini::parse(&ini.to_string());
        assert_eq!(ini, again);
    }

    #[test]
    fn save_and_load_use_the_filesystem() {
        let dir = std::env::temp_dir().join(format!("indium-ini-test-{}", std::process::id()));
        let path = dir.join("nested").join("shell.ini");
        let mut ini = Ini::default();
        ini.set("a", "b", "c");
        ini.save(&path).unwrap();
        assert_eq!(Ini::load(&path).get("a", "b"), Some("c"));
        assert_eq!(Ini::load(&dir.join("absent.ini")), Ini::default());
        let _ = fs::remove_dir_all(dir);
    }
}
