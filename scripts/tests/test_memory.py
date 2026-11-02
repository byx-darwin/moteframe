"""Compiler evidence must not silently accept missing or malformed layouts."""
from pathlib import Path
import importlib.util
import struct
import unittest

SCRIPT = Path(__file__).parents[1] / "check_memory.py"
SPEC = importlib.util.spec_from_file_location("memory", SCRIPT) if SCRIPT.exists() else None
MODULE = importlib.util.module_from_spec(SPEC) if SPEC else None
if SPEC:
    SPEC.loader.exec_module(MODULE)


def arm_object(stack=b"\x98\x01", link=2):
    """Small ELF fixture: eight code bytes and a 152-byte compiler frame."""
    names = b"\0.strtab\0.text.probe\0.stack_sizes\0"
    payload = names + bytes(8) + bytes(4) + stack
    offset = 52 + len(payload)
    header = b"\x7fELF\x01\x01\x01" + bytes(9)
    header += struct.pack("<HHIIIIIHHHHHH", 1, 40, 1, 0, 0, offset, 0, 52, 0, 0, 40, 4, 1)
    sections = [(0,) * 10,
                (1, 3, 0, 0, 52, len(names), 0, 0, 1, 0),
                (9, 1, 4, 0, 52 + len(names), 8, 0, 0, 2, 0),
                (21, 1, 128, 0, 60 + len(names), 4 + len(stack), link, 0, 1, 0)]
    return header + payload + b"".join(struct.pack("<IIIIIIIIII", *section) for section in sections)


class MemoryTests(unittest.TestCase):
    def setUp(self):
        if MODULE is None:
            self.fail("memory evidence parser not implemented")

    def test_should_read_target_sizes_from_compiler_globals(self):
        ir = '@_R42MF_PIXEL_RGB565 = constant [2 x i8] zeroinitializer, align 1\n'
        self.assertEqual(MODULE.read_layout(ir, {"MF_PIXEL_RGB565"}), {"MF_PIXEL_RGB565": 2})

    def test_should_distinguish_exact_marker_from_longer_prefixed_marker(self):
        ir='@MODULE_TOUCH = constant [56 x i8] zeroinitializer\n@MODULE_TOUCH_QUEUE = constant [16 x i8] zeroinitializer\n'
        self.assertEqual(MODULE.read_layout(ir,{'MODULE_TOUCH','MODULE_TOUCH_QUEUE'}),{'MODULE_TOUCH':56,'MODULE_TOUCH_QUEUE':16})

    def test_should_reject_missing_or_duplicate_layout_globals(self):
        with self.assertRaises(ValueError):
            MODULE.read_layout("", {"MF_PIXEL_RGB565"})
        row = '@_R42MF_PIXEL_RGB565 = constant [2 x i8] zeroinitializer, align 1\n'
        with self.assertRaises(ValueError):
            MODULE.read_layout(row + row, {"MF_PIXEL_RGB565"})

    def test_should_reject_non_arm_or_truncated_object_evidence(self):
        for data in [b"", b"\x7fELF\x02\x01" + bytes(60), b"\x7fELF\x01\x01" + bytes(46)]:
            with self.subTest(data=data):
                with self.assertRaises(ValueError):
                    MODULE.read_object(data)

    def test_should_read_executable_bytes_and_multibyte_frame(self):
        self.assertEqual(MODULE.read_object(arm_object()), {
            "executable_section_bytes": 8, "function_frames": {".text.probe": 152}})

    def test_should_read_a_zero_byte_leaf_frame(self):
        self.assertEqual(MODULE.read_object(arm_object(b"\0"))["function_frames"], {".text.probe": 0})

    def test_should_reject_truncated_stack_values_and_missing_function_link(self):
        for data in [arm_object(b"\x98"), arm_object(link=0)]:
            with self.subTest(data=data):
                with self.assertRaises(ValueError):
                    MODULE.read_object(data)


if __name__ == "__main__":
    unittest.main()
