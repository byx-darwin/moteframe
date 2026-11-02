"""Only explicitly reviewed workspace evolution may cross a frozen cost context."""
import copy
import hashlib
import importlib.util
import json
from pathlib import Path
import sys
import tempfile
import unittest

sys.path.insert(0, str(Path(__file__).parents[1]))
PATH = Path(__file__).parents[1] / 'check_build_context.py'


class ContextMigrationTests(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location('context_migration', PATH)
        self.module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.module)
        self.before = {'compiler': 'fixed', 'profile': 's', 'probe_sha256': 'probe',
                       'linker_sha256': 'linker', 'target': 'thumbv6m-none-eabi',
                       'lock_sha256': 'old-lock', 'cargo_configuration': {
                           'manifests': {'Cargo.toml': 'old', 'core.toml': 'same'},
                           'nontransport_configs_sha256': []}}
        self.after = copy.deepcopy(self.before)
        self.after['lock_sha256'] = 'new-lock'
        self.after['cargo_configuration']['manifests'].update({'Cargo.toml': 'new', 'storage.toml': 'storage'})
        self.policy = {'format_version': 1, 'transitions': [{
            'name': 'new-independent-modules', 'reference_sha256': 'a' * 64,
            'before': self.module.cargo_graph(self.before), 'after': self.module.cargo_graph(self.after)}]}

    def test_exact_reviewed_graph_transition_is_accepted(self):
        self.assertEqual(self.module.validate_transition(self.before, self.after, 'a' * 64, self.policy),
                         'new-independent-modules')

    def test_unchanged_context_does_not_need_a_migration(self):
        self.assertIsNone(self.module.validate_transition(self.before, self.before, 'unknown', {}))

    def test_all_non_graph_context_must_remain_exact(self):
        for field in ['compiler', 'profile', 'probe_sha256', 'linker_sha256', 'target']:
            changed = copy.deepcopy(self.after)
            changed[field] = 'unreviewed'
            with self.subTest(field=field), self.assertRaises(ValueError):
                self.module.validate_transition(self.before, changed, 'a' * 64, self.policy)
        changed = copy.deepcopy(self.after)
        changed['cargo_configuration']['nontransport_configs_sha256'] = ['new-config']
        with self.assertRaises(ValueError):
            self.module.validate_transition(self.before, changed, 'a' * 64, self.policy)

    def test_unreviewed_graph_or_reference_is_rejected(self):
        changes = [lambda x: x.update(lock_sha256='unknown'),
                   lambda x: x['cargo_configuration']['manifests'].update({'core.toml': 'changed'}),
                   lambda x: x['cargo_configuration']['manifests'].update({'ble.toml': 'extra'}),
                   lambda x: x['cargo_configuration']['manifests'].pop('storage.toml')]
        for mutate in changes:
            changed = copy.deepcopy(self.after)
            mutate(changed)
            with self.subTest(mutate=mutate), self.assertRaises(ValueError):
                self.module.validate_transition(self.before, changed, 'a' * 64, self.policy)
        with self.assertRaises(ValueError):
            self.module.validate_transition(self.before, self.after, 'b' * 64, self.policy)

    def test_migration_record_cannot_be_silently_rewritten(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / 'policy.json'
            data = json.dumps(self.policy).encode()
            path.write_bytes(data)
            checksum = hashlib.sha256(data).hexdigest()
            self.assertEqual(self.module.load_policy(path, checksum), self.policy)
            path.write_bytes(data + b' ')
            with self.assertRaises(ValueError):
                self.module.load_policy(path, checksum)

    def test_resource_regression_still_fails_after_real_graph_migration(self):
        import check_core_compatibility as core
        reference = core.load_reference(core.REFERENCE, core.REFERENCE_SHA256)
        candidate = copy.deepcopy(reference)
        candidate['environment'] = core.context()
        self.module.validate_context(reference['environment'], candidate['environment'], core.REFERENCE_SHA256)
        candidate['cases']['dedicated']['sections']['.text'] += 4
        with self.assertRaises(ValueError):
            core.compare_legacy(reference, candidate)


if __name__ == '__main__':
    unittest.main()
