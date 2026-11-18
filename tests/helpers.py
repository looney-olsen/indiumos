"""Shared helpers: load the extension-less scripts and build fake /proc and /sys trees."""

import importlib.machinery
import importlib.util
import os
import pathlib

ROOT = pathlib.Path(__file__).resolve().parent.parent
LIB = ROOT / "config" / "includes.chroot" / "usr" / "lib" / "indium"


def load_script(name):
    path = str(LIB / name)
    loader = importlib.machinery.SourceFileLoader(name.replace("-", "_"), path)
    spec = importlib.util.spec_from_loader(loader.name, loader)
    module = importlib.util.module_from_spec(spec)
    loader.exec_module(module)
    return module


def write(root, rel, text):
    path = pathlib.Path(root) / rel.lstrip("/")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


def fake_machine(root, ram_mb, cpus, gpu=None, disk=None, cmdline="", override=""):
    """gpu: (vendor_id, driver) or None. disk: 'nvme' | 'ssd' | 'hdd' or None."""
    write(root, "/proc/meminfo", f"MemTotal:       {ram_mb * 1024} kB\n")
    write(root, "/proc/cpuinfo", "".join(f"processor\t: {i}\n" for i in range(cpus)))
    write(root, "/proc/cmdline", cmdline + "\n")
    if override:
        write(root, "/etc/indium/override.conf", override)
    if gpu:
        vendor, driver = gpu
        write(root, "/sys/class/drm/card0/device/vendor", vendor + "\n")
        drivers = pathlib.Path(root) / "sys/bus/pci/drivers" / driver
        drivers.mkdir(parents=True, exist_ok=True)
        link = pathlib.Path(root) / "sys/class/drm/card0/device/driver"
        os.symlink(drivers, link)
        write(root, "/dev/dri/renderD128", "")
    if disk:
        name = "nvme0n1" if disk == "nvme" else "sda"
        write(root, f"/sys/block/{name}/queue/rotational", "1\n" if disk == "hdd" else "0\n")
        write(root, f"/sys/block/{name}/removable", "0\n")
