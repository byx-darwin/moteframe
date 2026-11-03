"""Reject misleading complete-consumer or standalone metadata evidence."""
import copy
import importlib.util
import json
from pathlib import Path
import subprocess
import sys
import unittest
sys.path.insert(0,str(Path(__file__).parents[1]))
PATH=Path(__file__).parents[1]/'check_device_foundation.py'

class FoundationTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        spec=importlib.util.spec_from_file_location('device_foundation',PATH)
        cls.module=importlib.util.module_from_spec(spec);spec.loader.exec_module(cls.module)
        cls.metadata=json.loads(subprocess.check_output(['cargo','metadata','--manifest-path',str(cls.module.MANIFEST),'--offline','--locked','--format-version','1'],text=True))
    def test_metadata_accepts_only_standalone_approved_six_path_dependencies(self):
        self.module.validate_metadata(self.metadata,False)
    def test_publication_workspace_feature_and_dependency_drift_are_rejected(self):
        app=next(p for p in self.metadata['packages'] if p['name']=='moteframe-device-foundation')
        for mutation in [lambda m:m.update(workspace_root='/tmp/other'),
                         lambda m:next(p for p in m['packages'] if p['id']==app['id']).update(publish=None),
                         lambda m:next(p for p in m['packages'] if p['id']==app['id'])['features'].update(extra=[]),
                         lambda m:next(p for p in m['packages'] if p['id']==app['id'])['dependencies'][0].update(uses_default_features=True),
                         lambda m:m['packages'].append(dict(app,id='extra',name='private-extra')),
                         lambda m:next(p for p in m['packages'] if p['name']=='moteframe-ui').update(source='git+unreviewed')]:
            m=copy.deepcopy(self.metadata);mutation(m)
            with self.subTest(mutation=mutation),self.assertRaises(ValueError):self.module.validate_metadata(m,False)
    def test_external_and_reclassified_targets_are_rejected_before_building(self):
        for name in ['moteframe-device-foundation','moteframe-core','moteframe-ui']:
            for kind in ['lib','test','example','custom-build']:
                m=copy.deepcopy(self.metadata)
                package=next(p for p in m['packages'] if p['name']==name)
                package['targets']=[{'kind':[kind],'crate_types':['bin'],
                                     'name':'outside','src_path':'/private/outside-foundation.rs'}]
                with self.subTest(name=name,kind=kind),self.assertRaises(ValueError):
                    self.module.validate_metadata(m,False)
        m=copy.deepcopy(self.metadata)
        p=next(p for p in m['packages'] if p['name']=='moteframe-device-foundation')
        p['targets'][0]['kind']=['custom-build']
        with self.assertRaises(ValueError):self.module.validate_metadata(m,False)

    def evidence(self):
        cases={}
        for name,small in [('default',False),('small-memory',True)]:
            layout={k:16 for k in self.module.LAYOUTS}
            layout.update(APP_UI=64 if small else 112,APP_DEMO=800 if small else 1200,
                          APP_PIXEL_BYTES=16 if small else 64,APP_RAW_CAPACITY=2 if small else 8,
                          APP_EVENT_CAPACITY=1 if small else 4,APP_VALUE_BYTES=8 if small else 16,
                          APP_ASSET_BYTES=2 if small else 16)
            cases[name]={'layout':layout,'sections':{'.text':100,'.rodata':20,'.data':0,'.bss':0},
                         'host_outputs':[1,2,3,4,5,6,7,8],'exercise_frame':100,'max_local_frame':200,
                         'owner_total':layout['APP_UI']+layout['APP_DEMO']+layout['APP_SOURCE']+layout['APP_SINK'],
                         'replay':{'capacities':[8,2,1,8,2] if small else [32,8,4,16,16],
                                   'samples':16,'events':16,'accepted':16,'received':16,'recoveries':3,'selection':0,'tx_fingerprint':123},
                         'image_sha256':'1'*64}
        return cases
    def test_complete_default_small_evidence_is_accepted(self):self.module.validate_evidence(self.evidence())
    def test_missing_frames_static_storage_capacity_and_double_counting_are_rejected(self):
        for mutation in [lambda c:c.pop('default'),lambda c:c['default'].update(exercise_frame=None),
                         lambda c:c['default']['sections'].update({'.bss':1}),
                         lambda c:c['small-memory']['layout'].update(APP_ASSET_BYTES=16),
                         lambda c:c['default'].update(owner_total=99999),
                         lambda c:c['default'].update(host_outputs=[0]*8),
                         lambda c:c['small-memory'].update(image_sha256='2'*64),
                         lambda c:c['small-memory']['replay'].update(accepted=15)]:
            c=self.evidence();mutation(c)
            with self.subTest(mutation=mutation),self.assertRaises(ValueError):self.module.validate_evidence(c)
    def test_replay_pixels_are_validated_not_just_the_hash(self):
        data=b'P6\n32 16\n255\n'+bytes(32*16*3)
        with self.assertRaises(ValueError):self.module.validate_image(data)
        with self.assertRaises(ValueError):self.module.validate_image(data[:-1])
