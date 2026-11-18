# Building

## The shell (any Debian/Ubuntu-like machine, including Kali)
```
sudo apt install build-essential pkg-config libgtk-4-dev libglib2.0-dev
cargo test --no-default-features
cargo run --no-default-features --bin indium-settings
```
`--no-default-features` skips layer-shell, so the UI runs as ordinary windows inside your current desktop.
For the real thing install `libgtk4-layer-shell-dev` (Debian 13) and build without that flag.

## The ISO
Use Debian 13 (a VM or container, privileged) with `live-build` installed, or run the **Build ISO** GitHub Action.
```
cargo build --release
install -Dm755 target/release/indium-shell config/includes.chroot/usr/bin/indium-shell
install -Dm755 target/release/indium-settings config/includes.chroot/usr/bin/indium-settings
sudo ./auto/config && sudo ./auto/build
```
The ISO is not yet tested; expect to fix package names and hooks on the first runs. Test it in a VM first.

## Tests
`cd tests && python3 -m unittest discover -v` and `cargo test`.
