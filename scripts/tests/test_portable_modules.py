"""New resource reports must include executed checks and distinguish fixed capacities."""
import copy
import importlib.util
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).parents[1]))
PATH = Path(__file__).parents[1] / 'check_portable_modules.py'


class ModuleEvidenceTests(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location('portable_cost', PATH)
        self.module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.module)
        case = {'sections': {'.text': 100, '.rodata': 0, '.data': 0, '.bss': 0},
                'exercise_frame': 48, 'layout': {'MODULE_OWNER': 4}, 'host_outputs': [1, 2, 3, 4, 5, 6, 7, 8]}
        self.cases = {name: copy.deepcopy(case) for name in self.module.MODES}
        for name, size in [('ble8', 24), ('ble32', 48), ('ble-borrowed8', 24)]:
            self.cases[name]['layout']['MODULE_SLOT'] = size

    def test_complete_equivalent_consumers_are_accepted(self):
        self.module.validate_evidence(self.cases)

    def test_missing_or_non_executed_consumer_is_rejected(self):
        for change in [lambda x: x.pop('storage-owned'),
                       lambda x: x['ble8'].update(host_outputs=[0] * 8),
                       lambda x: x['ble8'].update(exercise_frame=None),
                       lambda x: x['ble8']['sections'].update({'.bss': 1})]:
            cases = copy.deepcopy(self.cases)
            change(cases)
            with self.subTest(change=change), self.assertRaises(ValueError):
                self.module.validate_evidence(cases)

    def test_owned_borrowed_and_buffer_capacity_preserve_executed_output(self):
        for mode in ['storage-borrowed', 'ble32', 'ble-borrowed8']:
            cases = copy.deepcopy(self.cases)
            cases[mode]['host_outputs'][0] += 1
            with self.subTest(mode=mode), self.assertRaises(ValueError):
                self.module.validate_evidence(cases)

    def test_slot_capacity_difference_is_reflected_in_object_layout(self):
        self.cases['ble32']['layout']['MODULE_SLOT'] = 24
        with self.assertRaises(ValueError):
            self.module.validate_evidence(self.cases)


if __name__ == '__main__':
    unittest.main()
