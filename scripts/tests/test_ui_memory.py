"""Evidence parser must classify real linked sections, including zero-file BSS."""
import importlib.util
from pathlib import Path
import struct
import sys
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
import unittest

SPEC = importlib.util.spec_from_file_location('ui_memory', Path(__file__).resolve().parents[1] / 'check_ui_memory.py')
MODULE = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(MODULE)


def elf():
    names = b'\0.text\0.rodata\0.data\0.bss\0.shstrtab\0'
    data = bytearray(52 + len(names) + 9)
    data[:6] = b'\x7fELF\x01\x01'
    table = len(data)
    struct.pack_into('<HHIIIIIHHHHHH', data, 16, 2, 40, 1, 0, 0, table, 0, 52, 0, 0, 40, 6, 5)
    sections = [(0, 0, 0, 0, 0, 0, 0, 0, 0, 0),
                (1, 1, 6, 0, 52 + len(names), 4, 0, 0, 4, 0),
                (7, 1, 2, 0, 56 + len(names), 3, 0, 0, 1, 0),
                (15, 1, 3, 0, 59 + len(names), 2, 0, 0, 1, 0),
                (21, 8, 3, 0, 999999, 16, 0, 0, 4, 0),
                (26, 3, 0, 0, 52, len(names), 0, 0, 1, 0)]
    data[52:52 + len(names)] = names
    for section in sections:
        data.extend(struct.pack('<IIIIIIIIII', *section))
    return bytes(data)


class LinkedEvidence(unittest.TestCase):
    def test_classifies_readonly_initialized_and_bss_without_counting_metadata(self):
        self.assertEqual(MODULE.linked_sections(elf()), {'.text': 4, '.rodata': 3, '.data': 2, '.bss': 16})

    def test_rejects_wrong_machine_or_truncation(self):
        for data in [b'', elf()[:-1], elf()[:18] + b'\x3e\0' + elf()[20:]]:
            with self.assertRaises(ValueError):
                MODULE.linked_sections(data)

    def test_rejects_file_section_outside_the_elf(self):
        data = bytearray(elf())
        table = struct.unpack_from('<I', data, 32)[0]
        struct.pack_into('<I', data, table + 40 + 16, len(data))
        with self.assertRaises(ValueError):
            MODULE.linked_sections(data)
