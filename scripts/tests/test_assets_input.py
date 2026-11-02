"""Resource evidence must prove real capacities, execution and no static buffers."""
import copy
import importlib.util
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).parents[1]))
PATH = Path(__file__).parents[1] / 'check_assets_input.py'


class AssetsInputEvidenceTests(unittest.TestCase):
    def setUp(self):
        spec = importlib.util.spec_from_file_location('assets_input_cost', PATH)
        self.module = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(self.module)
        case = {'sections': {'.text': 100, '.rodata': 0, '.data': 0, '.bss': 0},
                'exercise_frame': 48, 'max_local_frame': 48,
                'layout': {'MODULE_OWNER': 16, 'MODULE_SCRATCH': 2, 'MODULE_PIXELS': 64},
                'host_outputs': [1, 2, 3, 4, 5, 6, 7, 8]}
        self.cases = {name: copy.deepcopy(case) for name in self.module.MODES}
        self.cases['assets16']['layout']['MODULE_SCRATCH'] = 16
        for mode, size in [('input1',12),('input4',12),('input16',24)]:
            self.cases[mode]['layout'] = {'MODULE_BUTTON': 64, 'MODULE_TOUCH': 64, 'MODULE_QUEUE': size, 'MODULE_TOUCH_QUEUE':size*2}
        self.cases['off']['layout'] = {'MODULE_OWNER': 4}

    def test_complete_executed_consumers_and_actual_capacities_are_accepted(self):
        self.module.validate_evidence(self.cases)

    def test_missing_unexecuted_and_static_storage_evidence_are_rejected(self):
        for change in [lambda x:x.pop('off'),lambda x:x['assets2'].update(host_outputs=[0]*8),
                       lambda x:x['input1']['sections'].update({'.bss':1}),
                       lambda x:x['input4'].update(exercise_frame=None),
                       lambda x:x['assets16'].update(max_local_frame=-1)]:
            cases=copy.deepcopy(self.cases);change(cases)
            with self.subTest(change=change),self.assertRaises(ValueError):
                self.module.validate_evidence(cases)

    def test_asset_scratch_sizes_must_preserve_executed_pixels(self):
        self.cases['assets16']['host_outputs'][0]+=1
        with self.assertRaises(ValueError):self.module.validate_evidence(self.cases)

    def test_large_queue_or_scratch_cannot_report_small_capacity_layout(self):
        for mode,key,value in [('input16','MODULE_QUEUE',12),('assets16','MODULE_SCRATCH',2),('assets2','MODULE_PIXELS',0)]:
            cases=copy.deepcopy(self.cases);cases[mode]['layout'][key]=value
            with self.subTest(mode=mode),self.assertRaises(ValueError):self.module.validate_evidence(cases)


if __name__ == '__main__':
    unittest.main()
