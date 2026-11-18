import tempfile
import unittest

from helpers import fake_machine, load_script

hw = load_script("indium-hwprofile")


class Decide(unittest.TestCase):
    def run_machine(self, **kwargs):
        with tempfile.TemporaryDirectory() as root:
            fake_machine(root, **kwargs)
            hw.SYSROOT = root
            return hw.decide(hw.parse_overrides([]))

    def test_two_gb_no_gpu_is_lite_lowmem(self):
        r = self.run_machine(ram_mb=1900, cpus=2, disk="hdd")
        self.assertEqual(r["profile"], "lite")
        self.assertTrue(r["lowmem"])
        self.assertEqual(r["compositor"], "labwc")
        self.assertEqual(r["renderer"], "pixman")
        self.assertEqual(r["storage"], "hdd")

    def test_dev_machine_4gb_amd_nvme_is_lite_but_accelerated(self):
        r = self.run_machine(ram_mb=3700, cpus=6, gpu=("0x1002", "amdgpu"), disk="nvme")
        self.assertEqual(r["profile"], "lite")
        self.assertFalse(r["lowmem"])
        self.assertEqual(r["renderer"], "gles2")
        self.assertEqual(r["compositor"], "labwc")
        self.assertEqual(r["storage"], "nvme")

    def test_16gb_amd_ssd_is_pro_wayfire(self):
        r = self.run_machine(ram_mb=15800, cpus=8, gpu=("0x1002", "amdgpu"), disk="ssd")
        self.assertEqual(r["profile"], "pro")
        self.assertEqual(r["compositor"], "wayfire")
        self.assertEqual(r["blur"], "on")

    def test_16gb_virtio_is_standard_software(self):
        r = self.run_machine(ram_mb=15800, cpus=8, gpu=("0x1af4", "virtio-pci"), disk="ssd")
        self.assertEqual(r["profile"], "standard")
        self.assertEqual(r["renderer"], "pixman")
        self.assertEqual(r["compositor"], "labwc")


class Overrides(unittest.TestCase):
    def run_machine(self, **kwargs):
        with tempfile.TemporaryDirectory() as root:
            fake_machine(root, **kwargs)
            hw.SYSROOT = root
            return hw.decide(hw.parse_overrides([]))

    def test_force_gpu_on_vm(self):
        r = self.run_machine(ram_mb=15800, cpus=8, disk="ssd", override="gpu=force\n")
        self.assertTrue(r["gpu"]["accelerated"])
        self.assertEqual(r["profile"], "pro")

    def test_gpu_off_downgrades_pro(self):
        r = self.run_machine(ram_mb=15800, cpus=8, gpu=("0x1002", "amdgpu"),
                             disk="ssd", override="gpu=off\n")
        self.assertEqual(r["profile"], "standard")
        self.assertEqual(r["compositor"], "labwc")

    def test_force_storage_ssd(self):
        r = self.run_machine(ram_mb=3700, cpus=4, disk="hdd", override="storage=ssd\n")
        self.assertEqual(r["storage"], "ssd")

    def test_cmdline_beats_file(self):
        r = self.run_machine(ram_mb=15800, cpus=8, gpu=("0x1002", "amdgpu"), disk="ssd",
                             cmdline="quiet indium.profile=lite", override="profile=pro\n")
        self.assertEqual(r["profile"], "lite")

    def test_forced_profile_on_small_machine(self):
        r = self.run_machine(ram_mb=1900, cpus=2, disk="hdd", override="profile=pro\n")
        self.assertEqual(r["profile"], "pro")


if __name__ == "__main__":
    unittest.main()
