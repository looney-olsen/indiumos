//! Notification data model and history, independent of D-Bus and GTK.

use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Urgency {
    Low,
    Normal,
    Critical,
}

impl Urgency {
    /// From the `urgency` hint byte of the Desktop Notifications spec.
    pub fn from_hint(byte: u8) -> Self {
        match byte {
            0 => Urgency::Low,
            2 => Urgency::Critical,
            _ => Urgency::Normal,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notification {
    pub id: u32,
    pub app_name: String,
    pub summary: String,
    pub body: String,
    /// Icon name or absolute file path; may be empty.
    pub icon: String,
    pub urgency: Urgency,
    /// Spec value: -1 = server default, 0 = never expires, >0 = milliseconds.
    pub expire_timeout: i32,
    /// (action key, label) pairs. The `default` key is the click action.
    pub actions: Vec<(String, String)>,
}

impl Notification {
    pub fn has_default_action(&self) -> bool {
        self.actions.iter().any(|(key, _)| key == "default")
    }

    /// How long the popup should stay on screen, or `None` if it must be dismissed manually.
    pub fn popup_millis(&self, default_ms: u32) -> Option<u32> {
        if self.urgency == Urgency::Critical {
            return None;
        }
        match self.expire_timeout {
            0 => None,
            t if t < 0 => Some(default_ms),
            t => Some(t as u32),
        }
    }
}

/// Reduce notification body markup (`<b>`, `<i>`, `<a href=...>`) to plain text.
///
/// The spec lets apps send a small HTML subset. Showing it as plain text avoids
/// invalid-markup errors and clickable links in popups.
pub fn plain_text(markup: &str) -> String {
    let mut out = String::with_capacity(markup.len());
    let mut in_tag = false;
    for c in markup.chars() {
        match c {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            _ if in_tag => {}
            _ => out.push(c),
        }
    }
    out.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// Split the flat `[key, label, key, label, ...]` array D-Bus delivers.
pub fn pair_actions(flat: &[String]) -> Vec<(String, String)> {
    (0..flat.len() / 2)
        .map(|i| (flat[2 * i].clone(), flat[2 * i + 1].clone()))
        .collect()
}

/// History shown in the control center, plus Do Not Disturb state.
#[derive(Debug)]
pub struct NotificationStore {
    history: VecDeque<Notification>,
    capacity: usize,
    pub do_not_disturb: bool,
}

impl NotificationStore {
    pub fn new(capacity: usize) -> Self {
        NotificationStore {
            history: VecDeque::new(),
            capacity,
            do_not_disturb: false,
        }
    }

    /// Record a notification (replacing one with the same id) and return whether
    /// it should pop up on screen.
    pub fn push(&mut self, n: Notification) -> bool {
        let show_popup = !self.do_not_disturb || n.urgency == Urgency::Critical;
        self.history.retain(|old| old.id != n.id);
        self.history.push_front(n);
        while self.history.len() > self.capacity {
            self.history.pop_back();
        }
        show_popup
    }

    pub fn remove(&mut self, id: u32) -> bool {
        let before = self.history.len();
        self.history.retain(|n| n.id != id);
        self.history.len() != before
    }

    pub fn clear(&mut self) {
        self.history.clear();
    }

    /// Newest first.
    pub fn history(&self) -> impl Iterator<Item = &Notification> {
        self.history.iter()
    }

    pub fn len(&self) -> usize {
        self.history.len()
    }

    pub fn is_empty(&self) -> bool {
        self.history.is_empty()
    }
}

/// Why a notification was closed (values from the spec's `NotificationClosed` signal).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    Expired = 1,
    Dismissed = 2,
    ClosedByCall = 3,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn note(id: u32, urgency: Urgency) -> Notification {
        Notification {
            id,
            app_name: "app".into(),
            summary: format!("n{id}"),
            body: String::new(),
            icon: String::new(),
            urgency,
            expire_timeout: -1,
            actions: vec![],
        }
    }

    #[test]
    fn timeouts_follow_the_spec() {
        let mut n = note(1, Urgency::Normal);
        assert_eq!(n.popup_millis(5000), Some(5000)); // -1 = default
        n.expire_timeout = 0;
        assert_eq!(n.popup_millis(5000), None); // never expires
        n.expire_timeout = 2500;
        assert_eq!(n.popup_millis(5000), Some(2500));
        n.urgency = Urgency::Critical;
        assert_eq!(n.popup_millis(5000), None); // critical sticks
    }

    #[test]
    fn urgency_hint_mapping() {
        assert_eq!(Urgency::from_hint(0), Urgency::Low);
        assert_eq!(Urgency::from_hint(1), Urgency::Normal);
        assert_eq!(Urgency::from_hint(2), Urgency::Critical);
        assert_eq!(Urgency::from_hint(9), Urgency::Normal);
    }

    #[test]
    fn markup_is_reduced_to_plain_text() {
        assert_eq!(plain_text("Hello <b>world</b>"), "Hello world");
        assert_eq!(
            plain_text("<a href=\"https://x\">link</a> &amp; more"),
            "link & more"
        );
        assert_eq!(plain_text("1 &lt; 2 &gt; 0"), "1 < 2 > 0");
        assert_eq!(plain_text("plain"), "plain");
        assert_eq!(plain_text("unterminated <b"), "unterminated ");
        assert_eq!(plain_text(""), "");
    }

    #[test]
    fn actions_are_paired_and_odd_tails_dropped() {
        let flat: Vec<String> = ["default", "Open", "reply", "Reply", "orphan"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let pairs = pair_actions(&flat);
        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0], ("default".into(), "Open".into()));
        let mut n = note(1, Urgency::Normal);
        n.actions = pairs;
        assert!(n.has_default_action());
    }

    #[test]
    fn history_is_newest_first_and_bounded() {
        let mut store = NotificationStore::new(3);
        for id in 1..=5 {
            store.push(note(id, Urgency::Normal));
        }
        let ids: Vec<u32> = store.history().map(|n| n.id).collect();
        assert_eq!(ids, vec![5, 4, 3]);
        assert_eq!(store.len(), 3);
    }

    #[test]
    fn replacing_an_id_keeps_one_entry() {
        let mut store = NotificationStore::new(10);
        store.push(note(7, Urgency::Normal));
        let mut updated = note(7, Urgency::Normal);
        updated.summary = "updated".into();
        store.push(updated);
        assert_eq!(store.len(), 1);
        assert_eq!(store.history().next().unwrap().summary, "updated");
    }

    #[test]
    fn do_not_disturb_hides_popups_except_critical() {
        let mut store = NotificationStore::new(10);
        store.do_not_disturb = true;
        assert!(!store.push(note(1, Urgency::Normal)));
        assert!(store.push(note(2, Urgency::Critical)));
        assert_eq!(store.len(), 2); // still recorded in history
    }

    #[test]
    fn remove_and_clear() {
        let mut store = NotificationStore::new(10);
        store.push(note(1, Urgency::Normal));
        store.push(note(2, Urgency::Normal));
        assert!(store.remove(1));
        assert!(!store.remove(1));
        store.clear();
        assert!(store.is_empty());
    }
}
