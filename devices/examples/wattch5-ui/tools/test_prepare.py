"""Asset conversion boundaries against hand-derived synthetic pixels."""
import importlib.util
from pathlib import Path
import struct
import unittest
from PIL import Image
spec=importlib.util.spec_from_file_location('prepare',Path(__file__).with_name('prepare.py'))
prepare=importlib.util.module_from_spec(spec)
spec.loader.exec_module(prepare)
class PrepareTests(unittest.TestCase):
    def test_should_encode_literal_rgb565_and_reject_partial_strips(self):
        image=Image.new('RGB',(2,2));image.putdata([(255,0,0),(0,255,0),(0,0,255),(255,255,255)])
        self.assertEqual(prepare.rgb565(image),bytes.fromhex('f80007e0001fffff'))
        self.assertEqual(prepare.strip(image,2,1,1).size,(2,1))
        for dims in [(1,1,1),(2,3,1),(2,1,3),(2,0,1)]:
            with self.assertRaises(ValueError):prepare.strip(image,*dims)
    def test_should_pack_absolute_ranges_after_a_versioned_header(self):
        image=Image.new('RGB',(1,1),(255,0,0));data=prepare.pack([image]*22)
        self.assertEqual(data[:10],b'W5UI\x01\0\0\0\0\x16')
        self.assertEqual(struct.unpack_from('>HHII',data,10),(1,1,274,2))
        self.assertEqual(data[274:],b'\xf8\0'*22)
