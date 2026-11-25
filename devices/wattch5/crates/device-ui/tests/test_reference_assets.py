"""Pixel conversion and crop validation for the host-only preparation tool."""
import importlib.util
from pathlib import Path
import unittest

from PIL import Image

SPEC = importlib.util.spec_from_file_location(
    "prepare_reference", Path(__file__).parents[1] / "examples/prepare_reference.py"
)
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


class ReferenceAssetsTests(unittest.TestCase):
    def test_should_encode_rgb565_msb_first_with_channel_truncation(self):
        image = Image.new("RGB", (5, 1))
        image.putdata([(255, 0, 0), (0, 255, 0), (0, 0, 255), (255, 255, 255), (128, 129, 130)])
        self.assertEqual(MODULE.pack_rgb565(image), bytes.fromhex("f80007e0001fffff8410"))

    def test_should_select_a_vertical_strip_frame_without_resampling(self):
        image = Image.new("RGB", (2, 4))
        image.putdata([(255, 0, 0)] * 4 + [(0, 255, 0)] * 4)
        cropped = MODULE.frame(image, 2, 2, 1)
        self.assertEqual(cropped.size, (2, 2))
        self.assertEqual(list(cropped.getdata()), [(0, 255, 0)] * 4)

    def test_should_reject_invalid_frame_bounds_and_ambiguous_strip_width(self):
        for width, height, index in [(2, 2, -1), (2, 2, 2), (0, 2, 0), (1, 2, 0), (2, 3, 1)]:
            with self.subTest(width=width, height=height, index=index):
                with self.assertRaises(ValueError):
                    MODULE.frame(Image.new("RGB", (2, 4)), width, height, index)


if __name__ == "__main__":
    unittest.main()
