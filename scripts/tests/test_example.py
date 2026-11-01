"""The host example must produce a complete, nonblank RGB image."""
from pathlib import Path
import importlib.util
import unittest

SCRIPT = Path(__file__).parents[1] / "check_example.py"
SPEC = importlib.util.spec_from_file_location("example_check", SCRIPT) if SCRIPT.exists() else None
MODULE = importlib.util.module_from_spec(SPEC) if SPEC is not None else None
if SPEC is not None:
    SPEC.loader.exec_module(MODULE)


class ExampleTests(unittest.TestCase):
    def setUp(self):
        if MODULE is None:
            self.fail("render-output validator is not implemented")
        self.header = b"P6\n64 40\n255\n"
        self.pixels = bytes.fromhex("ff000000ff000000ffffffff") + bytes(7668)

    def test_should_accept_a_complete_rgb_image_with_expected_primitive_colors(self):
        self.assertEqual(MODULE.validate(self.header + self.pixels), [])

    def test_should_reject_truncated_or_oversized_pixel_payload(self):
        for pixels in [self.pixels[:-1], self.pixels + b"\0"]:
            with self.subTest(length=len(pixels)):
                self.assertTrue(MODULE.validate(self.header + pixels))

    def test_should_reject_wrong_geometry_or_format(self):
        for header in [b"P3\n64 40\n255\n", b"P6\n40 64\n255\n", b"P6\n64 40\n65535\n"]:
            with self.subTest(header=header):
                self.assertTrue(MODULE.validate(header + self.pixels))

    def test_should_reject_a_complete_but_blank_frame(self):
        self.assertTrue(MODULE.validate(self.header + bytes(7680)))


if __name__ == "__main__":
    unittest.main()
