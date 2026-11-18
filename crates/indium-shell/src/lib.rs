//! The Indium OS desktop shell.
//!
//! One daemon (`indium-shell`) provides the menu bar, dock, Spotlight-style
//! launcher, notification server and control center in a single process, which
//! keeps memory use low on 2 GB machines. Settings (`indium-settings`) is a
//! separate on-demand program so its memory is returned when it closes.

pub mod bar;
pub mod control_center;
pub mod dock;
pub mod launcher;
pub mod layer;
pub mod notifications;
pub mod settings;
pub mod shell;
pub mod status;
pub mod theme;
pub mod toplevels;
pub mod util;
