"""Legacy I2C consumers must retain behavior, layout and fixed resource budgets."""
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

PATH = Path(__file__).parents[1] / 'check_i2c_compatibility.py'


class I2cCompatibilityTests(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location('i2c_compat', PATH)
        self.module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.module)
        self.reference = {'environment': {'compiler': 'fixed'}, 'cases': {'legacy': {
            'host_outputs': [1, 2], 'layout': {'I2C_OWNER': 24},
            'sections': {'.text': 100, '.rodata': 8, '.data': 0, '.bss': 0},
            'exercise_frame': 32, 'max_local_frame': 48}}}
        self.report = copy.deepcopy(self.reference)

    def test_unchanged_consumer_is_accepted(self):
        self.assertEqual(self.module.compare(self.reference, self.report),
                         {'flash': 0, 'static_ram': 0, 'exercise_frame': 0, 'max_local_frame': 0})

    def test_smaller_consumer_is_accepted(self):
        self.report['cases']['legacy']['sections']['.text'] = 99
        self.assertEqual(self.module.compare(self.reference, self.report)['flash'], -1)

    def test_behavior_layout_and_context_drift_are_rejected(self):
        for key in ['host_outputs', 'layout']:
            report = copy.deepcopy(self.report)
            report['cases']['legacy'][key] = []
            with self.subTest(key=key), self.assertRaises(ValueError):
                self.module.compare(self.reference, report)
        self.report['environment']['compiler'] = 'other'
        with self.assertRaises(ValueError):
            self.module.compare(self.reference, self.report)

    def test_each_positive_resource_delta_is_rejected(self):
        for field in ['.text', '.rodata', '.data', '.bss', 'exercise_frame', 'max_local_frame']:
            report = copy.deepcopy(self.report)
            case = report['cases']['legacy']
            target = case['sections'] if field.startswith('.') else case
            target[field] += 1
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.module.compare(self.reference, report)

    def test_missing_or_non_numeric_evidence_is_rejected(self):
        for value in [None, -1, True, '32']:
            report = copy.deepcopy(self.report)
            report['cases']['legacy']['exercise_frame'] = value
            with self.subTest(value=value), self.assertRaises(ValueError):
                self.module.compare(self.reference, report)

    def test_frozen_reference_has_exact_source_provenance(self):
        reference = self.module.load_reference()
        self.assertEqual(reference['baseline_source_sha256'], self.module.SOURCE_SHA256)

    def test_modified_reference_or_source_provenance_is_rejected(self):
        reference = self.module.load_reference()
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'reference.json'
            reference['baseline_source_sha256'] = '0' * 64
            data = json.dumps(reference).encode()
            path.write_bytes(data)
            with self.assertRaises(ValueError):
                self.module.load_reference(path)
            with self.assertRaises(ValueError):
                self.module.load_reference(path, hashlib.sha256(data).hexdigest())

    def test_cli_rejects_unrecorded_cargo_wrapper_before_compiling(self):
        key = 'CARGO_BUILD_RUSTC_WRAPPER'
        env = {**os.environ, key: 'unrecorded-compiler-wrapper'}
        result = subprocess.run([sys.executable, str(PATH)], env=env,
                                capture_output=True, text=True, check=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('unset: ' + key, result.stderr)


if __name__ == '__main__':
    unittest.main()
