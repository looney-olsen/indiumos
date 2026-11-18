import os
import tempfile
import unittest

from helpers import load_script

ov = load_script("indium-override")


class Parse(unittest.TestCase):
    def test_defaults_to_auto(self):
        self.assertEqual(ov.parse_args([]), {"profile": "auto", "gpu": "auto", "storage": "auto"})

    def test_valid(self):
        v = ov.parse_args(["profile=lite", "gpu=force", "storage=ssd"])
        self.assertEqual(v, {"profile": "lite", "gpu": "force", "storage": "ssd"})

    def test_rejects_bad_input(self):
        for bad in (["profile=huge"], ["colour=red"], ["profile"], ["gpu=on", "gpu=off"],
                    ["profile=lite\nstorage=hdd"], ["profile=lite;rm"], ["=x"]):
            with self.assertRaises(ov.UsageError, msg=bad):
                ov.parse_args(bad)

    def test_render_contains_all_keys(self):
        text = ov.render(ov.parse_args(["gpu=off"]))
        self.assertIn("gpu=off\n", text)
        self.assertIn("profile=auto\n", text)
        self.assertIn("storage=auto\n", text)


class Write(unittest.TestCase):
    def test_atomic_write_replaces_and_cleans_temp(self):
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "etc", "override.conf")
            ov.write_atomically(path, "a\n")
            ov.write_atomically(path, "b\n")
            self.assertEqual(open(path).read(), "b\n")
            self.assertEqual(os.listdir(os.path.dirname(path)), ["override.conf"])

    def test_planted_symlink_is_not_followed(self):
        with tempfile.TemporaryDirectory() as d:
            victim = os.path.join(d, "victim")
            open(victim, "w").write("keep")
            os.symlink(victim, os.path.join(d, ".override.conf.tmp"))
            ov.write_atomically(os.path.join(d, "override.conf"), "x\n")
            self.assertEqual(open(victim).read(), "keep")

    def test_main_rejects_non_root_after_validation(self):
        if os.geteuid() == 0:
            self.skipTest("running as root")
        self.assertEqual(ov.main(["profile=lite"]), 1)

    def test_main_usage_error(self):
        self.assertEqual(ov.main(["bogus=1"]), 2)


if __name__ == "__main__":
    unittest.main()
