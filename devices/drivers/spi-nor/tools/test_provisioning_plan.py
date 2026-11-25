"""No device access: pinned fixture hashes exercise the actual offline validator."""
from pathlib import Path
import tempfile
import unittest

from provisioning_plan import Region, verify_image, write_new


IMAGE = b'A' * 4096 + b'B' * 4096
IMAGE_HASH = '54f624253436dcd5fe656688f7ddb3a314b4524453ce553a5b39a62ce0de4ee5'
REGIONS = (
    Region('preserved', 'preserve', 0, 4096,
           '6896d9ea3f73a4434f5832bc65714e7d066f177373f36f34dc8a6f735daa41b1'),
    Region('settings', 'proposed-settings', 4096, 8192,
           '725bcd6c66d02acf6ebeab9c92410e010ea22e336876256aaf05a211f4ce1902'),
)


class ProposalTests(unittest.TestCase):
    def test_should_validate_pinned_source_and_region_hashes(self):
        result = verify_image(IMAGE, len(IMAGE), IMAGE_HASH, REGIONS)
        self.assertEqual([(r['start'], r['end'], r['bytes']) for r in result],
                         [(0, 4096, 4096), (4096, 8192, 4096)])
        self.assertEqual(result[0]['sha256'], REGIONS[0].sha256)

    def test_should_reject_changed_capture_length_and_region_digest(self):
        for changed in (IMAGE[:-1], b'X' + IMAGE[1:]):
            with self.assertRaises(ValueError):
                verify_image(changed, 8192, IMAGE_HASH, REGIONS)
        wrong = (Region('preserved', 'preserve', 0, 4096, '0' * 64), REGIONS[1])
        with self.assertRaises(ValueError):
            verify_image(IMAGE, 8192, IMAGE_HASH, wrong)

    def test_should_reject_overlap_gap_overflow_alignment_and_incomplete_coverage(self):
        for start, end in ((0, 8192), (8192, 12288), (4096, 12288),
                           (4097, 8192), (4096, 4096), (-4096, 8192)):
            changed = (REGIONS[0], Region('bad', 'proposed-settings', start, end, '0' * 64))
            with self.assertRaises(ValueError):
                verify_image(IMAGE, 8192, IMAGE_HASH, changed)
        with self.assertRaises(ValueError):
            verify_image(IMAGE, 8192, IMAGE_HASH, REGIONS[:1])

    def test_should_create_only_new_local_json_and_preserve_existing_bytes(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / 'proposal.json'
            write_new(output, {'qualification': 'unqualified', 'device_io': False})
            original = output.read_bytes()
            with self.assertRaises(FileExistsError):
                write_new(output, {'qualification': 'forbidden replacement'})
            self.assertEqual(output.read_bytes(), original)
            self.assertIn(b'"device_io": false', original)


if __name__ == '__main__':
    unittest.main()
