"""The source adapter must reject guesses and preserve literal source pixels."""
import unittest
from PIL import Image
import factory_pack as f

class FactoryPackTests(unittest.TestCase):
    def test_should_evaluate_only_bounded_integer_layout_expressions(self):
        self.assertEqual(f.coordinate('19+13*3+4'),62)
        for bad in ['__import__("os")','x+1','1/0','2**8','1<<4']:
            with self.assertRaises(ValueError): f.coordinate(bad)

    def test_should_preserve_rgb565_rows_and_deduplicate_without_color_quantization(self):
        red=Image.new('RGB',(128,128),(255,0,0))
        green=Image.new('RGB',(128,128),(0,255,0))
        packed=f.pack({36:red,54:green,2232:red})
        self.assertEqual(f.unpack_row(packed,36,127),b'\xf8\x00'*128)
        self.assertEqual(f.unpack_row(packed,54,0),b'\x07\xe0'*128)
        self.assertLess(len(packed),11000)
        bad=bytearray(packed);bad[32+36*4:36+36*4]=(9377).to_bytes(4,'little')
        with self.assertRaises(ValueError): f.unpack_row(bad,36,0)

if __name__=='__main__':unittest.main()
