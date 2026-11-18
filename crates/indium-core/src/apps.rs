//! App metadata, launcher search ranking, and matching running windows to apps.
//!
//! The GTK side builds `AppMeta` values from the system's .desktop files; the
//! ranking and matching logic lives here so it can be tested without GTK.

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AppMeta {
    /// Desktop file ID, e.g. `org.gnome.Epiphany.desktop`.
    pub id: String,
    pub name: String,
    pub generic_name: String,
    pub comment: String,
    pub keywords: Vec<String>,
    pub icon: String,
    /// `StartupWMClass` from the .desktop file, if any.
    pub wm_class: String,
    pub exec: String,
    /// Extra window app IDs this entry stands for (`X-Indium-AppIds`), e.g. a
    /// launcher that opens Epiphany or Firefox depending on the machine.
    pub alt_ids: Vec<String>,
}

/// Score how well `app` matches `query` (case-insensitive). 0 means no match.
pub fn score(app: &AppMeta, query: &str) -> u32 {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return 0;
    }
    let name = app.name.to_lowercase();

    if name == q {
        return 1000;
    }
    if name.starts_with(&q) {
        return 800;
    }
    if name
        .split(|c: char| !c.is_alphanumeric())
        .any(|word| !word.is_empty() && word.starts_with(&q))
    {
        return 600;
    }
    if name.contains(&q) {
        return 400;
    }
    let in_keywords = app.keywords.iter().any(|k| k.to_lowercase().contains(&q));
    if in_keywords || app.generic_name.to_lowercase().contains(&q) {
        return 250;
    }
    if app.comment.to_lowercase().contains(&q) {
        return 100;
    }
    if is_subsequence(&q, &name) {
        return 50;
    }
    0
}

fn is_subsequence(needle: &str, haystack: &str) -> bool {
    let mut hay = haystack.chars();
    needle.chars().all(|n| hay.any(|h| h == n))
}

/// Indices of matching apps, best first (ties broken alphabetically), at most `limit`.
pub fn rank(apps: &[AppMeta], query: &str, limit: usize) -> Vec<usize> {
    let mut scored: Vec<(u32, usize)> = apps
        .iter()
        .enumerate()
        .map(|(i, app)| (score(app, query), i))
        .filter(|(s, _)| *s > 0)
        .collect();
    scored.sort_by(|a, b| {
        b.0.cmp(&a.0).then_with(|| {
            apps[a.1]
                .name
                .to_lowercase()
                .cmp(&apps[b.1].name.to_lowercase())
        })
    });
    scored.into_iter().take(limit).map(|(_, i)| i).collect()
}

fn normalize_id(id: &str) -> String {
    id.trim().trim_end_matches(".desktop").to_lowercase()
}

fn exec_basename(exec: &str) -> String {
    exec.split_whitespace()
        .next()
        .and_then(|cmd| cmd.rsplit('/').next())
        .unwrap_or("")
        .to_lowercase()
}

/// Find the app a running window belongs to, given its Wayland `app_id`.
///
/// Tries, in order of reliability: desktop ID, `StartupWMClass`, executable
/// name, then the display name.
impl AppMeta {
    /// True if a window with this app ID belongs to this entry via `alt_ids`.
    pub fn alt_matches(&self, app_id: &str) -> bool {
        let wanted = normalize_id(app_id);
        !wanted.is_empty() && self.alt_ids.iter().any(|a| normalize_id(a) == wanted)
    }
}

pub fn match_app_id(apps: &[AppMeta], app_id: &str) -> Option<usize> {
    let wanted = normalize_id(app_id);
    if wanted.is_empty() {
        return None;
    }
    let tiers: [&dyn Fn(&AppMeta) -> bool; 4] = [
        &|a| normalize_id(&a.id) == wanted,
        &|a| !a.wm_class.is_empty() && a.wm_class.to_lowercase() == wanted,
        &|a| exec_basename(&a.exec) == wanted,
        &|a| a.name.to_lowercase() == wanted,
    ];
    tiers.iter().find_map(|tier| apps.iter().position(tier))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(id: &str, name: &str) -> AppMeta {
        AppMeta {
            id: id.into(),
            name: name.into(),
            ..AppMeta::default()
        }
    }

    fn sample() -> Vec<AppMeta> {
        let mut firefox = app("firefox-esr.desktop", "Firefox ESR");
        firefox.generic_name = "Web Browser".into();
        firefox.keywords = vec!["internet".into(), "www".into()];
        firefox.exec = "/usr/lib/firefox-esr/firefox-esr %u".into();
        let mut files = app("thunar.desktop", "Files");
        files.comment = "Browse the file system".into();
        files.exec = "thunar %U".into();
        let mut term = app("foot.desktop", "Foot");
        term.comment = "A fast terminal".into();
        term.exec = "foot".into();
        let mut calc = app("org.gnome.Calculator.desktop", "Calculator");
        calc.wm_class = "gnome-calculator".into();
        vec![firefox, files, term, calc]
    }

    #[test]
    fn alt_ids_match_windows() {
        let mut browser = app("indium-browser.desktop", "Web Browser");
        browser.alt_ids = vec!["org.gnome.Epiphany".into(), "firefox-esr".into()];
        assert!(browser.alt_matches("firefox-esr"));
        assert!(browser.alt_matches("org.gnome.Epiphany"));
        assert!(!browser.alt_matches("foot"));
        assert!(!browser.alt_matches(""));
    }

    #[test]
    fn name_prefix_beats_everything_else() {
        let apps = sample();
        assert_eq!(rank(&apps, "fire", 8)[0], 0); // Firefox ESR, prefix
        assert_eq!(score(&apps[0], "firefox esr"), 1000);
        // Two prefix matches tie, so alphabetical order decides: Files, then Firefox.
        assert_eq!(rank(&apps, "fi", 8)[..2], [1, 0]);
    }

    #[test]
    fn matches_generic_name_keywords_and_comments() {
        let apps = sample();
        assert!(rank(&apps, "browser", 8).contains(&0)); // generic name
        assert!(rank(&apps, "internet", 8).contains(&0)); // keyword
        assert_eq!(rank(&apps, "terminal", 8), vec![2]); // comment
    }

    #[test]
    fn word_start_ranks_above_plain_substring() {
        let apps = sample();
        assert_eq!(score(&apps[0], "esr"), 600);
        assert_eq!(score(&apps[0], "refo"), 400);
    }

    #[test]
    fn fuzzy_subsequence_is_the_last_resort() {
        let apps = sample();
        assert_eq!(score(&apps[3], "clc"), 50);
        assert_eq!(score(&apps[3], "zzz"), 0);
    }

    #[test]
    fn empty_query_and_limit() {
        let apps = sample();
        assert!(rank(&apps, "   ", 8).is_empty());
        assert_eq!(rank(&apps, "f", 2).len(), 2);
    }

    #[test]
    fn running_windows_match_by_id_class_exec_then_name() {
        let apps = sample();
        assert_eq!(match_app_id(&apps, "foot"), Some(2)); // desktop id
        assert_eq!(match_app_id(&apps, "Thunar"), Some(1)); // case-insensitive
        assert_eq!(match_app_id(&apps, "gnome-calculator"), Some(3)); // wm class
        assert_eq!(match_app_id(&apps, "firefox-esr"), Some(0)); // id and exec
        assert_eq!(match_app_id(&apps, "Calculator"), Some(3)); // name
        assert_eq!(match_app_id(&apps, "unknown-app"), None);
        assert_eq!(match_app_id(&apps, ""), None);
    }
}
