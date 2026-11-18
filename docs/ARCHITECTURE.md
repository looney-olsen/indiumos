# Architecture

Indium OS is a Debian 13 (trixie) remix built with live-build.

**Boot and login:** GRUB/syslinux boots a live image. `greetd` (with `agreety`) starts `indium-session`.
The live image logs in automatically; installed systems use the greetd login prompt.

**Hardware profiles:** `indium-hwprofile` runs at boot, detects RAM, CPU, GPU and storage and writes
`/etc/indium/profile`. It also sets sysctl values, zram, the I/O scheduler and fstrim.

| Profile | RAM (usable) | Compositor | Effects |
|---|---|---|---|
| Lite | below ~4.5 GB | labwc | none |
| Standard | ~4.5 GB and up, no hardware GPU or few cores | labwc (pixman) or Wayfire | light |
| Pro | ~6.5 GB and up, GPU, 4+ cores | Wayfire | animations, blur, dock magnification |

Overrides (highest priority first): `--set`, kernel cmdline `indium.profile/gpu/storage`, `/etc/indium/override.conf`.
Settings > Performance writes the file through `/usr/lib/indium/indium-override` (pkexec + polkit).

**Session:** `indium-session` loads the profile, starts Wayfire or labwc, and falls back to labwc with
software rendering if Wayfire fails in its first 10 seconds.

**Shell:** one GTK4 process (`indium-shell`, app id `org.indium.Shell`) holds the menu bar, dock,
notification server, control center and Spotlight launcher, to save RAM. It uses layer-shell for placement and
`wlr-foreign-toplevel-management` to track windows. `indium-settings` (`org.indium.Settings`) runs on demand.

**Crates:** `indium-core` is pure Rust (config, profile parsing, app search, calculator, notifications,
system command parsers) with unit tests. `indium-shell` is the GTK UI.
