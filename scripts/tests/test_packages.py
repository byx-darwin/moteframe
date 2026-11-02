"""Cargo publication candidates must contain portable reviewed files only."""
from pathlib import Path
import importlib.util
import unittest

SCRIPT = Path(__file__).parents[1] / "check_packages.py"
SPEC = importlib.util.spec_from_file_location("packages", SCRIPT) if SCRIPT.exists() else None
MODULE = importlib.util.module_from_spec(SPEC) if SPEC is not None else None
if SPEC is not None:
    SPEC.loader.exec_module(MODULE)


class PackageTests(unittest.TestCase):
    def setUp(self):
        if MODULE is None:
            self.fail("package candidate validator is not implemented")
        self.files = ["Cargo.toml", "Cargo.toml.orig", "Cargo.lock", "README.md", "LICENSE", "NOTICE", "src/lib.rs", "src/read.rs", "src/spi.rs", "src/hal.rs", "tests/hal.rs", "tests/recovery.rs", "tests/shared_spi.rs", "tests/faults.rs"]

    def test_should_accept_a_complete_core_package(self):
        self.assertEqual(MODULE.validate_package("moteframe-core", self.files), [])

    def test_should_accept_i2c_extensions_and_maintained_fault_tests(self):
        files = ["Cargo.toml", "Cargo.lock", "README.md", "LICENSE", "NOTICE", "src/lib.rs", "src/resources.rs", "src/checked.rs", "tests/read.rs", "tests/extensions.rs", "tests/wire.rs"]
        self.assertEqual(MODULE.validate_package("moteframe-i2c", files), [])

    def test_should_accept_portable_storage_and_ble_packages(self):
        for package, test in [("moteframe-storage", "read"), ("moteframe-ble", "values")]:
            files = ["Cargo.toml", "Cargo.lock", "README.md", "LICENSE", "NOTICE", "src/lib.rs", f"tests/{test}.rs", "tests/debug.rs"]
            with self.subTest(package=package):
                self.assertEqual(MODULE.validate_package(package, files), [])
                self.assertTrue(MODULE.validate_package(package, files + ["src/vendor.rs"]))

    def test_should_require_assets_input_behavior_tests_and_reject_private_resources(self):
        for package,source in [
            ("moteframe-assets",["src/lib.rs","src/formats.rs","src/raster.rs","tests/raster.rs","tests/ui.rs"]),
            ("moteframe-input",["src/lib.rs","src/queue.rs","src/button.rs","src/touch.rs","tests/input.rs"]),
        ]:
            files=["Cargo.toml","README.md","LICENSE","NOTICE",*source]
            with self.subTest(package=package):
                self.assertEqual(MODULE.validate_package(package,files),[])
                self.assertTrue(MODULE.validate_package(package,files+["resources/original.bin"]))
                self.assertTrue(MODULE.validate_package(package,[f for f in files if f!=source[-1]]))

    def test_should_reject_missing_license_texts(self):
        self.assertTrue(MODULE.validate_package("moteframe-core", [f for f in self.files if f != "LICENSE"]))

    def test_should_reject_missing_attribution_notice(self):
        self.assertTrue(MODULE.validate_package("moteframe-core", [f for f in self.files if f != "NOTICE"]))

    def test_should_reject_obsolete_alternative_license_files(self):
        self.assertTrue(MODULE.validate_package("moteframe-core", self.files + ["LICENSE-MIT"]))

    def test_should_reject_binary_and_board_profile_candidates(self):
        for extra in ["src/profile.rs", "resources.bin", "../private.rs", "/private.rs"]:
            with self.subTest(extra=extra):
                self.assertTrue(MODULE.validate_package("moteframe-core", self.files + [extra]))

    def test_should_reject_tests_or_cargo_files_outside_the_reviewed_set(self):
        self.assertTrue(MODULE.validate_package("moteframe-core", self.files + ["tests/private.rs"]))


if __name__ == "__main__":
    unittest.main()
