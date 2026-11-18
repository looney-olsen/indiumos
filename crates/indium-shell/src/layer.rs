//! Wayland layer-shell setup for the shell's surfaces.
//!
//! With the `layer-shell` feature (always on in the ISO) windows become real
//! desktop surfaces: the menu bar, the dock, popups and the launcher overlay.
//! Without it (development on a machine lacking the library) they stay ordinary
//! windows so the UI can still be run and inspected.

use gtk::prelude::*;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    Top,
    Overlay,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Keyboard {
    None,
    OnDemand,
    Exclusive,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    /// Do not reserve screen space.
    None,
    /// Reserve exactly the surface size (plus margin) on its anchored edge.
    Auto,
    /// Reserve this many pixels.
    Fixed(i32),
    /// Ignore other surfaces' zones and cover the whole output (launcher backdrop).
    Ignore,
}

#[derive(Clone, Copy)]
pub struct Spec {
    pub namespace: &'static str,
    pub layer: Layer,
    pub top: bool,
    pub bottom: bool,
    pub left: bool,
    pub right: bool,
    /// Margins in pixels: (top, right, bottom, left).
    pub margin: (i32, i32, i32, i32),
    pub zone: Zone,
    pub keyboard: Keyboard,
}

impl Spec {
    pub const fn new(namespace: &'static str, layer: Layer) -> Self {
        Spec {
            namespace,
            layer,
            top: false,
            bottom: false,
            left: false,
            right: false,
            margin: (0, 0, 0, 0),
            zone: Zone::None,
            keyboard: Keyboard::None,
        }
    }
}

/// Turn `window` into a layer-shell surface. Must be called before it is shown.
/// Returns `false` when layer-shell is unavailable (the window stays a normal one).
#[cfg(feature = "layer-shell")]
pub fn apply(window: &impl IsA<gtk::Window>, spec: &Spec) -> bool {
    use gtk4_layer_shell as ls;
    use gtk4_layer_shell::LayerShell;

    if !ls::is_supported() {
        return false;
    }
    let w = window.as_ref();
    w.init_layer_shell();
    w.set_namespace(Some(spec.namespace));
    w.set_layer(match spec.layer {
        Layer::Top => ls::Layer::Top,
        Layer::Overlay => ls::Layer::Overlay,
    });
    w.set_anchor(ls::Edge::Top, spec.top);
    w.set_anchor(ls::Edge::Bottom, spec.bottom);
    w.set_anchor(ls::Edge::Left, spec.left);
    w.set_anchor(ls::Edge::Right, spec.right);
    w.set_margin(ls::Edge::Top, spec.margin.0);
    w.set_margin(ls::Edge::Right, spec.margin.1);
    w.set_margin(ls::Edge::Bottom, spec.margin.2);
    w.set_margin(ls::Edge::Left, spec.margin.3);
    match spec.zone {
        Zone::None => w.set_exclusive_zone(0),
        Zone::Auto => w.auto_exclusive_zone_enable(),
        Zone::Fixed(px) => w.set_exclusive_zone(px),
        Zone::Ignore => w.set_exclusive_zone(-1),
    }
    w.set_keyboard_mode(match spec.keyboard {
        Keyboard::None => ls::KeyboardMode::None,
        Keyboard::OnDemand => ls::KeyboardMode::OnDemand,
        Keyboard::Exclusive => ls::KeyboardMode::Exclusive,
    });
    true
}

#[cfg(not(feature = "layer-shell"))]
pub fn apply(_window: &impl IsA<gtk::Window>, _spec: &Spec) -> bool {
    false
}

/// Change only the exclusive zone of an existing layer surface.
#[cfg(feature = "layer-shell")]
pub fn set_zone(window: &impl IsA<gtk::Window>, pixels: i32) {
    use gtk4_layer_shell::LayerShell;
    let w = window.as_ref();
    if w.is_layer_window() {
        w.set_exclusive_zone(pixels);
    }
}

#[cfg(not(feature = "layer-shell"))]
pub fn set_zone(_window: &impl IsA<gtk::Window>, _pixels: i32) {}

/// Is this process really running as a layer-shell client?
#[cfg(feature = "layer-shell")]
pub fn available() -> bool {
    gtk4_layer_shell::is_supported()
}

#[cfg(not(feature = "layer-shell"))]
pub fn available() -> bool {
    false
}
