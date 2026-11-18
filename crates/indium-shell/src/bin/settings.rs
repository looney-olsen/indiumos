//! `indium-settings`: the System Settings app.
//!
//!   indium-settings            open Settings
//!   indium-settings about      open a specific page
//!                              (appearance, performance, display, network, sound, about)

fn main() -> gtk::glib::ExitCode {
    indium_shell::settings::run()
}
