//! Toolkit-independent logic for the Indium OS shell.
//!
//! Everything in here is plain Rust with no GTK or Wayland dependency, so it
//! can be unit-tested anywhere and reused by other Indium tools.

pub mod apps;
pub mod calc;
pub mod config;
pub mod ini;
pub mod notify;
pub mod profile;
pub mod sys;

/// Version of the Indium OS shell, shown in the About page.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
