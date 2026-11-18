//! The notification server (`org.freedesktop.Notifications`), the popups it
//! shows, and the history shown in the control center.
//!
//! The D-Bus service uses GIO's own bus support, so it runs on the GTK main
//! loop: no extra thread, runtime or dependency.

use crate::layer::{self, Keyboard, Layer, Spec, Zone};
use crate::util::Listeners;
use gtk::prelude::*;
use gtk::{gio, glib};
use indium_core::notify::{
    pair_actions, plain_text, CloseReason, Notification, NotificationStore, Urgency,
};
use indium_core::profile::HardwareProfile;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::time::Duration;

const DEFAULT_POPUP_MS: u32 = 5000;
const HISTORY_SIZE: usize = 30;
const OBJECT_PATH: &str = "/org/freedesktop/Notifications";
const INTERFACE: &str = "org.freedesktop.Notifications";

const INTROSPECTION: &str = r#"
<node>
  <interface name="org.freedesktop.Notifications">
    <method name="GetCapabilities">
      <arg direction="out" name="capabilities" type="as"/>
    </method>
    <method name="Notify">
      <arg direction="in" name="app_name" type="s"/>
      <arg direction="in" name="replaces_id" type="u"/>
      <arg direction="in" name="app_icon" type="s"/>
      <arg direction="in" name="summary" type="s"/>
      <arg direction="in" name="body" type="s"/>
      <arg direction="in" name="actions" type="as"/>
      <arg direction="in" name="hints" type="a{sv}"/>
      <arg direction="in" name="expire_timeout" type="i"/>
      <arg direction="out" name="id" type="u"/>
    </method>
    <method name="CloseNotification">
      <arg direction="in" name="id" type="u"/>
    </method>
    <method name="GetServerInformation">
      <arg direction="out" name="name" type="s"/>
      <arg direction="out" name="vendor" type="s"/>
      <arg direction="out" name="version" type="s"/>
      <arg direction="out" name="spec_version" type="s"/>
    </method>
    <signal name="NotificationClosed">
      <arg name="id" type="u"/>
      <arg name="reason" type="u"/>
    </signal>
    <signal name="ActionInvoked">
      <arg name="id" type="u"/>
      <arg name="action_key" type="s"/>
    </signal>
  </interface>
</node>"#;

type NotifyArgs = (
    String,
    u32,
    String,
    String,
    String,
    Vec<String>,
    HashMap<String, glib::Variant>,
    i32,
);

struct Popup {
    card: gtk::Widget,
    generation: u64,
}

pub struct Notifications {
    store: RefCell<NotificationStore>,
    conn: RefCell<Option<gio::DBusConnection>>,
    next_id: Cell<u32>,
    window: gtk::Window,
    column: gtk::Box,
    popups: RefCell<HashMap<u32, Popup>>,
    generation: Cell<u64>,
    history_changed: Listeners<()>,
}

impl Notifications {
    pub fn new(app: &gtk::Application, profile: &HardwareProfile) -> Rc<Self> {
        let column = gtk::Box::new(gtk::Orientation::Vertical, 0);
        let window = gtk::Window::builder()
            .application(app)
            .decorated(false)
            .resizable(false)
            .default_width(360)
            .child(&column)
            .build();
        window.add_css_class("indium-notifications");
        window.add_css_class(profile.effects_class());
        layer::apply(
            &window,
            &Spec {
                top: true,
                right: true,
                margin: (36, 12, 0, 0),
                zone: Zone::None,
                keyboard: Keyboard::None,
                ..Spec::new("indium-notifications", Layer::Overlay)
            },
        );

        let center = Rc::new(Notifications {
            store: RefCell::new(NotificationStore::new(HISTORY_SIZE)),
            conn: RefCell::new(None),
            next_id: Cell::new(1),
            window,
            column,
            popups: RefCell::new(HashMap::new()),
            generation: Cell::new(0),
            history_changed: Listeners::default(),
        });
        center.own_bus_name();
        center
    }

    // ------------------------------------------------------------- D-Bus

    /// Claim the notification name. If another daemon already owns it, ours
    /// stays idle and notifications keep going to that one.
    fn own_bus_name(self: &Rc<Self>) {
        let node = gio::DBusNodeInfo::for_xml(INTROSPECTION).expect("introspection XML is valid");
        let interface = node
            .lookup_interface(INTERFACE)
            .expect("interface is declared");
        let weak = Rc::downgrade(self);
        let _owner = gio::bus_own_name(
            gio::BusType::Session,
            INTERFACE,
            gio::BusNameOwnerFlags::NONE,
            move |connection, _| {
                if let Some(this) = weak.upgrade() {
                    this.register(connection, &interface);
                }
            },
            |_, _| {},
            |_, name| {
                eprintln!("indium: could not own {name}; another notification daemon is running")
            },
        );
    }

    fn register(
        self: &Rc<Self>,
        connection: gio::DBusConnection,
        interface: &gio::DBusInterfaceInfo,
    ) {
        let weak: Weak<Notifications> = Rc::downgrade(self);
        let registered = connection
            .register_object(OBJECT_PATH, interface)
            .method_call(move |_, _, _, _, method, params, invocation| {
                if let Some(this) = weak.upgrade() {
                    this.handle_call(method, &params, invocation);
                }
            })
            .build();
        match registered {
            Ok(_) => *self.conn.borrow_mut() = Some(connection),
            Err(err) => eprintln!("indium: could not publish notification service: {err}"),
        }
    }

    fn handle_call(
        self: &Rc<Self>,
        method: &str,
        params: &glib::Variant,
        invocation: gio::DBusMethodInvocation,
    ) {
        match method {
            "GetCapabilities" => {
                let caps = vec!["body", "actions", "icon-static", "persistence"];
                invocation.return_value(Some(&(caps,).to_variant()));
            }
            "GetServerInformation" => {
                let info = ("Indium", "Indium OS", indium_core::VERSION, "1.2");
                invocation.return_value(Some(&info.to_variant()));
            }
            "CloseNotification" => {
                if let Some((id,)) = params.get::<(u32,)>() {
                    self.close_by_call(id);
                }
                invocation.return_value(None);
            }
            "Notify" => match params.get::<NotifyArgs>() {
                Some(args) => {
                    let id = self.notify(args);
                    invocation.return_value(Some(&(id,).to_variant()));
                }
                None => invocation.return_dbus_error(
                    "org.freedesktop.DBus.Error.InvalidArgs",
                    "Notify arguments have the wrong types",
                ),
            },
            other => invocation.return_dbus_error(
                "org.freedesktop.DBus.Error.UnknownMethod",
                &format!("No such method: {other}"),
            ),
        }
    }

    fn notify(self: &Rc<Self>, args: NotifyArgs) -> u32 {
        let (app_name, replaces_id, icon, summary, body, actions, hints, expire_timeout) = args;
        let id = if replaces_id != 0 {
            replaces_id
        } else {
            let id = self.next_id.get();
            self.next_id.set(id.wrapping_add(1).max(1));
            id
        };
        let urgency = hints
            .get("urgency")
            .and_then(|v| v.get::<u8>())
            .map(Urgency::from_hint)
            .unwrap_or(Urgency::Normal);
        self.receive(Notification {
            id,
            app_name,
            summary,
            body: plain_text(&body),
            icon,
            urgency,
            expire_timeout,
            actions: pair_actions(&actions),
        });
        id
    }

    fn emit_signal(&self, name: &str, body: glib::Variant) {
        if let Some(conn) = self.conn.borrow().as_ref() {
            if let Err(err) = conn.emit_signal(None, OBJECT_PATH, INTERFACE, name, Some(&body)) {
                eprintln!("indium: could not send {name}: {err}");
            }
        }
    }

    fn emit_closed(&self, id: u32, reason: CloseReason) {
        self.emit_signal("NotificationClosed", (id, reason as u32).to_variant());
    }

    fn emit_action(&self, id: u32, key: &str) {
        self.emit_signal("ActionInvoked", (id, key).to_variant());
    }

    // ------------------------------------------------------------ history

    /// Newest first.
    pub fn history(&self) -> Vec<Notification> {
        self.store.borrow().history().cloned().collect()
    }

    pub fn clear_history(&self) {
        self.store.borrow_mut().clear();
        self.history_changed.emit(&());
    }

    pub fn remove_from_history(&self, id: u32) {
        let removed = self.store.borrow_mut().remove(id);
        if removed {
            self.history_changed.emit(&());
        }
    }

    pub fn do_not_disturb(&self) -> bool {
        self.store.borrow().do_not_disturb
    }

    pub fn set_do_not_disturb(&self, on: bool) {
        self.store.borrow_mut().do_not_disturb = on;
    }

    pub fn on_history_change(&self, callback: impl Fn() + 'static) {
        self.history_changed.add(move |_| callback());
    }

    // ------------------------------------------------------------- popups

    fn receive(self: &Rc<Self>, n: Notification) {
        let popup = self.store.borrow_mut().push(n.clone());
        self.history_changed.emit(&());
        if popup {
            self.show_popup(&n);
        }
    }

    fn close_by_call(self: &Rc<Self>, id: u32) {
        self.remove_popup(id);
        self.remove_from_history(id);
        self.emit_closed(id, CloseReason::ClosedByCall);
    }

    fn show_popup(self: &Rc<Self>, n: &Notification) {
        self.remove_popup(n.id); // a replacement takes the old popup's place
        let card = self.make_card(n);
        self.column.prepend(&card);

        let generation = self.generation.get() + 1;
        self.generation.set(generation);
        self.popups.borrow_mut().insert(
            n.id,
            Popup {
                card: card.clone().upcast(),
                generation,
            },
        );
        self.window.set_visible(true);

        if let Some(ms) = n.popup_millis(DEFAULT_POPUP_MS) {
            let this = Rc::clone(self);
            let id = n.id;
            glib::timeout_add_local_once(Duration::from_millis(ms.into()), move || {
                let still_current = this
                    .popups
                    .borrow()
                    .get(&id)
                    .map(|p| p.generation == generation)
                    .unwrap_or(false);
                if still_current {
                    this.dismiss(id, CloseReason::Expired);
                }
            });
        }
    }

    /// Remove the popup card but keep the history entry.
    fn dismiss(&self, id: u32, reason: CloseReason) {
        if self.remove_popup(id) {
            self.emit_closed(id, reason);
        }
    }

    fn remove_popup(&self, id: u32) -> bool {
        let popup = self.popups.borrow_mut().remove(&id);
        match popup {
            Some(p) => {
                self.column.remove(&p.card);
                if self.popups.borrow().is_empty() {
                    self.window.set_visible(false);
                }
                true
            }
            None => false,
        }
    }

    fn make_card(self: &Rc<Self>, n: &Notification) -> gtk::Box {
        let card = gtk::Box::new(gtk::Orientation::Horizontal, 10);
        card.add_css_class("notif-card");
        if n.urgency == Urgency::Critical {
            card.add_css_class("critical");
        }

        let icon = if n.icon.starts_with('/') {
            gtk::Image::from_file(&n.icon)
        } else if !n.icon.is_empty() {
            gtk::Image::from_icon_name(&n.icon)
        } else {
            gtk::Image::from_icon_name("dialog-information-symbolic")
        };
        icon.set_pixel_size(36);
        icon.set_valign(gtk::Align::Start);
        card.append(&icon);

        let text = gtk::Box::new(gtk::Orientation::Vertical, 2);
        text.set_hexpand(true);
        text.append(
            &gtk::Label::builder()
                .label(&n.app_name)
                .xalign(0.0)
                .css_classes(["app"])
                .build(),
        );
        text.append(
            &gtk::Label::builder()
                .label(&n.summary)
                .xalign(0.0)
                .wrap(true)
                .max_width_chars(36)
                .css_classes(["summary"])
                .build(),
        );
        if !n.body.is_empty() {
            text.append(
                &gtk::Label::builder()
                    .label(&n.body)
                    .xalign(0.0)
                    .wrap(true)
                    .lines(3)
                    .ellipsize(gtk::pango::EllipsizeMode::End)
                    .max_width_chars(36)
                    .css_classes(["body"])
                    .build(),
            );
        }

        let extra: Vec<&(String, String)> = n
            .actions
            .iter()
            .filter(|(key, _)| key != "default")
            .take(3)
            .collect();
        if !extra.is_empty() {
            let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            row.set_margin_top(6);
            for (key, label) in extra {
                let button = gtk::Button::with_label(label);
                let this = Rc::clone(self);
                let (id, key) = (n.id, key.clone());
                button.connect_clicked(move |_| {
                    this.emit_action(id, &key);
                    this.dismiss(id, CloseReason::Dismissed);
                });
                row.append(&button);
            }
            text.append(&row);
        }
        card.append(&text);

        let close = gtk::Button::from_icon_name("window-close-symbolic");
        close.add_css_class("close");
        close.set_valign(gtk::Align::Start);
        {
            let this = Rc::clone(self);
            let id = n.id;
            close.connect_clicked(move |_| this.dismiss(id, CloseReason::Dismissed));
        }
        card.append(&close);

        // Clicking the card itself runs the notification's default action.
        let click = gtk::GestureClick::new();
        {
            let this = Rc::clone(self);
            let (id, has_default) = (n.id, n.has_default_action());
            click.connect_released(move |_, _, _, _| {
                if has_default {
                    this.emit_action(id, "default");
                }
                this.dismiss(id, CloseReason::Dismissed);
            });
        }
        card.add_controller(click);
        card
    }
}
