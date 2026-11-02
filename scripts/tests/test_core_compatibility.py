"""Cost gates reject incomplete evidence rather than silently accepting zero."""
import copy
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
PATH = Path(__file__).resolve().parents[1] / 'check_core_compatibility.py'
MODULE = None
if PATH.exists():
    SPEC = importlib.util.spec_from_file_location('core_compatibility', PATH)
    MODULE = importlib.util.module_from_spec(SPEC)
    SPEC.loader.exec_module(MODULE)


class CompatibilityEvidence(unittest.TestCase):
    def setUp(self):
        self.assertIsNotNone(MODULE, 'core compatibility gate is not implemented')
        self.case = {
            'host_outputs': [17, 31, 47, 59, 61, 79, 83, 97],
            'layout': {'SPI': 8, 'SPI_ALIGN': 4},
            'sections': {'.text': 120, '.rodata': 16, '.data': 0, '.bss': 0},
            'exercise_frame': 32, 'max_local_frame': 40,
        }
        env = {'compiler': 'locked', 'target': 'thumbv6m-none-eabi',
               'profile': 'fixed', 'lock_sha256': 'a', 'probe_sha256': 'b'}
        self.reference = {'format_version': 1, 'environment': env,
                          'cases': {name: copy.deepcopy(self.case) for name in MODULE.LEGACY},
                          'baseline_revision': MODULE.REVISION,
                          'baseline_source_sha256': {path: 'a' * 64 for path in MODULE.BASELINE_SOURCES}}
        self.report = copy.deepcopy(self.reference)

    def test_equal_legacy_workload_has_zero_deltas(self):
        deltas = MODULE.compare_legacy(self.reference, self.report)
        self.assertEqual(deltas['dedicated']['flash_bytes'], 0)
        self.assertEqual(deltas['shared']['exercise_frame'], 0)

    def test_cargo_compiler_aliases_target_flags_and_incremental_overrides_are_rejected(self):
        for key in ['CARGO_BUILD_RUSTC', 'CARGO_BUILD_RUSTC_WRAPPER',
                    'CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER', 'CARGO_INCREMENTAL',
                    'CARGO_BUILD_INCREMENTAL', 'CARGO_TARGET_THUMBV6M_NONE_EABI_RUSTFLAGS',
                    'CARGO_TARGET_AARCH64_APPLE_DARWIN_RUSTFLAGS',
                    'CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS']:
            with self.subTest(key=key), patch.dict(os.environ, {key: 'unrecorded'}, clear=True):
                with self.assertRaisesRegex(ValueError, key):
                    MODULE.fixed_environment()

    def test_missing_or_changed_build_context_is_rejected(self):
        for key in self.reference['environment']:
            for missing in [False, True]:
                report = copy.deepcopy(self.report)
                if missing:
                    del report['environment'][key]
                else:
                    report['environment'][key] = 'changed'
                with self.subTest(key=key, missing=missing), self.assertRaises(ValueError):
                    MODULE.compare_legacy(self.reference, report)

    def test_positive_legacy_flash_ram_frame_or_layout_is_rejected(self):
        for field, key in [('sections', '.text'), ('sections', '.data'),
                           ('sections', '.bss'), ('layout', 'SPI'), ('layout', 'SPI_ALIGN')]:
            report = copy.deepcopy(self.report)
            report['cases']['dedicated'][field][key] += 4
            with self.subTest(field=field, key=key), self.assertRaises(ValueError):
                MODULE.compare_legacy(self.reference, report)
        for field in ['exercise_frame', 'max_local_frame']:
            report = copy.deepcopy(self.report)
            report['cases']['dedicated'][field] += 8
            with self.subTest(field=field), self.assertRaises(ValueError):
                MODULE.compare_legacy(self.reference, report)

    def test_behavior_changes_are_rejected_even_when_smaller(self):
        self.report['cases']['shared']['host_outputs'][0] += 1
        self.report['cases']['shared']['sections']['.text'] = 4
        with self.assertRaises(ValueError):
            MODULE.compare_legacy(self.reference, self.report)

    def test_missing_empty_or_nonfinite_evidence_is_rejected(self):
        mutations = [
            lambda x: x['cases'].pop('shared'),
            lambda x: x['cases']['shared'].pop('layout'),
            lambda x: x['cases']['shared']['layout'].clear(),
            lambda x: x['cases']['shared']['sections'].pop('.bss'),
            lambda x: x['cases']['shared']['sections'].update({'.text': 0}),
            lambda x: x['cases']['shared'].update(exercise_frame=None),
            lambda x: x['cases']['shared'].update(max_local_frame=-1),
            lambda x: x['cases']['shared']['layout'].update(SPI=0),
            lambda x: x['cases']['shared'].update(host_outputs=[]),
            lambda x: x['cases']['shared'].update(host_outputs=[0] * 8),
            lambda x: x.update(format_version=2),
        ]
        for mutate in mutations:
            report = copy.deepcopy(self.report)
            mutate(report)
            with self.subTest(mutate=mutate), self.assertRaises(ValueError):
                MODULE.compare_legacy(self.reference, report)

    def test_corrupt_reference_is_not_a_valid_budget(self):
        self.reference['cases']['dedicated']['sections']['.text'] = -1
        with self.assertRaises(ValueError):
            MODULE.compare_legacy(self.reference, self.report)

    def test_wrappers_cannot_hide_extra_size_or_alignment(self):
        case = copy.deepcopy(self.case)
        case['layout'] = {prefix + name: value
                          for name, value in [('SPI', 8), ('PIN', 12), ('DELAY', 4)]
                          for prefix in ['MF_', 'MF_RAW_', 'MA_', 'MA_RAW_']}
        MODULE.validate_wrappers(case)
        for key in list(case['layout']):
            changed = copy.deepcopy(case)
            changed['layout'][key] += 4
            with self.subTest(key=key), self.assertRaises(ValueError):
                MODULE.validate_wrappers(changed)

    def test_missing_or_corrupt_source_provenance_is_rejected(self):
        for change in [lambda x: x.pop('baseline_source_sha256'),
                       lambda x: x.update(baseline_source_sha256={}),
                       lambda x: x['baseline_source_sha256'].update({'Cargo.toml': '0' * 64}),
                       lambda x: x['baseline_source_sha256'].pop('crates/moteframe-display/Cargo.toml', None),
                       lambda x: x.update(baseline_revision='f' * 40)]:
            reference = copy.deepcopy(self.reference)
            change(reference)
            with self.subTest(change=change), self.assertRaises(ValueError):
                MODULE.compare_legacy(reference, self.report)

    def test_reference_file_cannot_be_silently_rebudgeted(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'reference.json'
            original = json.dumps(self.reference).encode()
            path.write_bytes(original)
            checksum = hashlib.sha256(original).hexdigest()
            self.assertEqual(MODULE.load_reference(path, checksum), self.reference)
            changed = copy.deepcopy(self.reference)
            changed['cases']['dedicated']['sections']['.text'] += 200
            path.write_text(json.dumps(changed))
            with self.assertRaises(ValueError):
                MODULE.load_reference(path, checksum)

    def test_manifest_and_cargo_configuration_changes_change_build_fingerprint(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in MODULE.MANIFESTS:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('[package]\nname="fixture"\n')
            before = MODULE.build_configuration(root)
            (root / 'Cargo.toml').write_text('[profile.release]\nopt-level=0\n')
            manifest_changed = MODULE.build_configuration(root)
            self.assertNotEqual(before, manifest_changed)
            config = root / '.cargo/config.toml'
            config.parent.mkdir()
            config.write_text('[profile.release]\noverflow-checks=true\n')
            self.assertNotEqual(manifest_changed, MODULE.build_configuration(root))
            config.write_text('[source.crates-io]\nreplace-with="fixture"\n[source.fixture]\nregistry="https://example.invalid/index"\n')
            self.assertEqual(manifest_changed, MODULE.build_configuration(root))
            for text in ['build={rustflags=["-Copt-level=0"]}\n',
                         '[env]\nRUSTFLAGS="-Copt-level=0"\n',
                         '[target.thumbv6m-none-eabi]\nrustflags=["-Copt-level=0"]\n']:
                config.write_text(text)
                with self.subTest(config=text):
                    self.assertNotEqual(manifest_changed, MODULE.build_configuration(root))


if __name__ == '__main__':
    unittest.main()
