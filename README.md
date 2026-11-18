# Indium OS

A Debian-based Linux distribution with a macOS-style desktop, from low-end machines (2 GB RAM) to high-end ones.
Non-commercial, Apache-2.0, built by Gangster Prince Productions.

**Status:** early alpha. The shell builds and its tests pass; the ISO has not been built or booted yet.

## What is in it
- **Hardware profiles** (Lite / Standard / Pro) chosen at boot from RAM, CPU, GPU and storage, with overrides to force GPU and SSD/HDD behavior.
- **Shell** in Rust + GTK4: menu bar, dock, notifications, control center, Spotlight-style launcher, Settings app.
- **Compositors:** labwc (light) and Wayfire (effects), with automatic fallback.
- **Base:** Debian 13, live-build, Calamares, greetd.
- Browser: Epiphany on Lite, Firefox ESR on Standard and Pro.

## Layout
- `crates/` Rust shell and core library
- `config/` live-build package lists, hooks and the files copied into the image
- `auto/` live-build scripts
- `tests/` Python tests for the profile tool and override helper
- `docs/` architecture, building, roadmap

See `docs/BUILDING.md` to build and run.
