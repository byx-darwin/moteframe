#!/usr/bin/env python3
"""Execute the standalone composition and measure its actual Cortex-M0 owners."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

from check_boundary import RESOLVED_UPSTREAM, REGISTRY, RUST_FILES
from check_core_compatibility import (ROOT, TARGET, SEEDS, LINKER, PROFILE, fixed_environment,
                                      options, run, validate_case, build_configuration)
from check_memory import read_layout, read_object
from check_ui_memory import linked_sections

APP=ROOT/'examples/device-foundation'
MANIFEST=APP/'Cargo.toml'
OUT=ROOT/'target/device-foundation'
PROBE=ROOT/'scripts/probes/device_foundation.rs'
LAYOUTS={'APP_'+key for key in ['UI','DEMO','SESSION','SERVICES','LINK','CATALOG','RENDER',
                                'SOURCE','SINK','PIXEL_BYTES','RAW_CAPACITY','EVENT_CAPACITY',
                                'VALUE_BYTES','ASSET_BYTES']}
DEPENDENCIES={'moteframe-'+name:(['refresh','frame-pacing'] if name=='ui' else [])
              for name in ['core','ui','assets','storage','input','ble']}



def validate_targets(package):
    """Allow only the exact source/kind/name tuples before Cargo executes code."""
    name=package['name']
    owner=APP if name=='moteframe-device-foundation' else ROOT/'crates'/name
    paths=({'src/lib.rs','examples/replay.rs','tests/services.rs','tests/integration.rs','tests/replay.rs'}
           if name=='moteframe-device-foundation' else
           {p for p in RUST_FILES[name] if p=='src/lib.rs' or p.startswith(('tests/','examples/'))})
    expected=set()
    for relative in paths:
        path=owner/relative
        if not path.resolve().is_relative_to(owner.resolve()):
            raise ValueError('target source escapes its owner')
        kind='lib' if relative=='src/lib.rs' else 'test' if relative.startswith('tests/') else 'example'
        target_name=name.replace('-','_') if kind=='lib' else path.stem
        expected.add((target_name,(kind,),('lib',) if kind=='lib' else ('bin',),str(path.resolve())))
    actual=[(t['name'],tuple(t['kind']),tuple(t['crate_types']),str(Path(t['src_path']).resolve()))
            for t in package['targets']]
    if len(actual)!=len(expected) or set(actual)!=expected:
        raise ValueError('unreviewed Cargo target source/kind/name: '+name)

def validate_metadata(m,small):
    """Reject publication, private paths/patches and unexpected feature unification."""
    p=next((p for p in m['packages'] if Path(p['manifest_path'])==MANIFEST),None)
    if (p is None or p['publish']!=[] or p['source'] is not None
            or p['features']!={'default':[],'small-memory':[]}
            or Path(m['workspace_root'])!=APP or m['workspace_members']!=[p['id']]):
        raise ValueError('standalone example workspace/publication/features drift')
    validate_targets(p)
    deps={d['name']:d for d in p['dependencies']}
    if set(deps)!=set(DEPENDENCIES):raise ValueError('unexpected application dependencies')
    literals=re.findall(r'\bpath\s*=\s*"([^"\n]+)"',MANIFEST.read_text())
    if len(literals)!=6 or set(literals)!={'../../crates/'+n for n in DEPENDENCIES}:
        raise ValueError('example must retain six reviewed relative paths')
    for name,features in DEPENDENCIES.items():
        d=deps[name]
        if (Path(d['path'])!=ROOT/'crates'/name or d['uses_default_features']
                or d['features']!=features or d['kind'] is not None or d['req']!='=0.2.0'):
            raise ValueError('application path/default features/kind/version drift: '+name)
    expected=set(DEPENDENCIES)|{p['name']}
    actual={item['name'] for item in m['packages'] if item['source'] is None}
    if actual!=expected:raise ValueError('unexpected local/private resolved package')
    for item in m['packages']:
        name=item['name']
        if name in DEPENDENCIES:
            validate_targets(item)
            if (item['source'] is not None or Path(item['manifest_path'])!=ROOT/'crates'/name/'Cargo.toml'
                    or item['version']!='0.2.0'):
                raise ValueError('replaced portable package: '+name)
        elif name!=p['name'] and (name not in RESOLVED_UPSTREAM or item['source']!=REGISTRY):
            raise ValueError('unreviewed upstream dependency: '+name)
    if len({item['name'] for item in m['packages']})!=len(m['packages']):
        raise ValueError('duplicate resolved package identity')
    nodes={n['id']:n for n in m['resolve']['nodes']}
    if set(nodes)!={item['id'] for item in m['packages']}:
        raise ValueError('incomplete resolved graph')
    if sorted(nodes[p['id']]['features'])!=(['default','small-memory'] if small else ['default']):
        raise ValueError('wrong application profile resolved')
    for item in m['packages']:
        if item['name'] in DEPENDENCIES:
            wanted=['frame-pacing','refresh'] if item['name']=='moteframe-ui' else []
            if nodes[item['id']]['features']!=wanted:
                raise ValueError('unexpected portable feature activation: '+item['name'])


def capacities(small):return [8,2,1,8,2] if small else [32,8,4,16,16]


def validate_image(data):
    """Check final independent pixels in addition to comparing full PPM bytes."""
    prefix=b'P6\n32 16\n255\n'
    if not data.startswith(prefix) or len(data)!=len(prefix)+32*16*3:
        raise ValueError('wrong/truncated final PPM')
    pixels=data[len(prefix):]
    for x,y,rgb in [(1,1,(255,255,255)),(2,1,(0,0,0)),(8,1,(255,0,0)),
                    (9,1,(0,255,0)),(10,1,(0,0,255)),(1,8,(255,0,0)),
                    (1,14,(0,0,0)),(5,14,(0,0,0)),(9,14,(255,255,255))]:
        start=(y*32+x)*3
        if tuple(pixels[start:start+3])!=rgb:raise ValueError('unexpected replay pixel')


def validate_evidence(cases):
    if set(cases)!={'default','small-memory'}:raise ValueError('missing actual RAM profile')
    for name,c in cases.items():
        validate_case(c)
        if c['sections']['.data'] or c['sections']['.bss']:raise ValueError('unexpected static mutable RAM')
        if set(c['layout'])!=LAYOUTS:raise ValueError('missing owner/capacity layout')
        small=name=='small-memory';ui,raw,events,value,asset=capacities(small)
        for key,size in {'APP_PIXEL_BYTES':ui*2,'APP_RAW_CAPACITY':raw,'APP_EVENT_CAPACITY':events,
                         'APP_VALUE_BYTES':value,'APP_ASSET_BYTES':asset}.items():
            if c['layout'][key]!=size:raise ValueError('capacity does not match real compiled app')
        total=sum(c['layout'][key] for key in ['APP_UI','APP_DEMO','APP_SOURCE','APP_SINK'])
        if c['owner_total']!=total:raise ValueError('owner total double counts borrowed/nested storage')
        r=c['replay']
        if (r['capacities']!=capacities(small) or any(r[k]!=16 for k in ['samples','events','accepted','received'])
                or r['recoveries']!=3 or r['selection']!=0):raise ValueError('incomplete replay behavior')
        if not re.fullmatch('[0-9a-f]{64}',c['image_sha256']):raise ValueError('invalid image digest')
        if not c['exercise_frame'] or any(not value for value in c['host_outputs']):
            raise ValueError('unexecuted combined workload')
    a,b=cases['default'],cases['small-memory']
    if a['host_outputs']!=b['host_outputs'] or a['image_sha256']!=b['image_sha256']:
        raise ValueError('profile changed actual combined output')
    if {k:v for k,v in a['replay'].items() if k!='capacities'}!={k:v for k,v in b['replay'].items() if k!='capacities'}:
        raise ValueError('profile changed logical replay observations')
    if b['owner_total']>=a['owner_total']:raise ValueError('small profile did not reduce actual owned storage')


def cargo(args,log,features):
    result=subprocess.run(['cargo',args[0],'--manifest-path',str(MANIFEST),'--offline','--locked',
                           *args[1:],*features],cwd=ROOT,env={**os.environ,'CARGO_TARGET_DIR':str(OUT/'build')},
                          text=True,capture_output=True)
    (OUT/log).write_text(result.stdout+result.stderr)
    if result.returncode:raise RuntimeError('Cargo failed: '+log+'\n'+result.stdout+result.stderr)
    return result.stdout


def build(target,small):
    folder=OUT/('small-memory' if small else 'default')/('build-'+target)
    settings={'opt-level':'"s"','lto':'"fat"','codegen-units':'1','panic':'"abort"','debug':'0','strip':'"none"',
              'debug-assertions':'false','overflow-checks':'false','incremental':'false','split-debuginfo':'"off"','rpath':'false'}
    config=[a for k,v in settings.items() for a in ['--config',f'profile.release.{k}={v}']]
    output=run(['cargo','build','--manifest-path',str(MANIFEST),'--lib','--release','--target',target,
                '--target-dir',str(folder),'--offline','--locked','--message-format=json',*config,
                *(['--features','small-memory'] if small else [])])
    libs={}
    for line in output.splitlines():
        item=json.loads(line)
        if item.get('reason')=='compiler-artifact':
            paths=[p for p in item['filenames'] if p.endswith('.rlib')]
            if paths:libs[item['target']['name']]=paths[0]
    if not {'moteframe_core','moteframe_ui','moteframe_device_foundation'}<=set(libs):
        raise ValueError('missing actual compiled composition libraries')
    return libs


def measure(name,host,hlibs,alibs):
    folder=OUT/name;folder.mkdir(parents=True,exist_ok=True)
    hflags=options(host,hlibs,[])+['--check-cfg','cfg(foundation_layout)']
    aflags=options(TARGET,alibs,[])+['--check-cfg','cfg(foundation_layout)']
    library=folder/'libfoundation_probe.rlib'
    run(['rustc',str(PROBE),'--crate-name','foundation_probe','--crate-type','rlib',*hflags,'-o',str(library)])
    harness=folder/'host.rs';harness.write_text('fn main(){println!("{:?}",'+str(SEEDS)+'.map(|s|foundation_probe::exercise(core::hint::black_box(s))));}\n')
    executable=folder/'host'
    run(['rustc',str(harness),*hflags,'--extern','foundation_probe='+str(library),'-Clto=fat','-o',str(executable)])
    outputs=json.loads(run([str(executable)]))
    ir,obj=folder/'probe.ll',folder/'probe.o'
    run(['rustc',str(PROBE),'--crate-name','foundation_probe','--crate-type','rlib',*aflags,
         '--cfg','foundation_layout','-Zemit-stack-sizes','--emit',f'llvm-ir={ir},obj={obj}'],
        env={**os.environ,'RUSTC_BOOTSTRAP':'1'})
    layout=read_layout(ir.read_text(),LAYOUTS)
    frames=read_object(obj.read_bytes())['function_frames']
    # The app's generic workload is monomorphized here with the real logical Sink.
    exercise=[size for symbol,size in frames.items() if 'moteframe_device_foundation' in symbol and '8exercise' in symbol]
    if len(exercise)!=1:raise ValueError('missing/ambiguous combined workload frame')
    run(['rustc',str(PROBE),'--crate-name','foundation_probe','--crate-type','rlib',*aflags,'-o',str(library)])
    entry,linker,elf=folder/'entry.rs',folder/'link.ld',folder/'consumer.elf'
    entry.write_text('''#![no_std]
#![no_main]
#[panic_handler] fn panic(_: &core::panic::PanicInfo)->!{loop{core::hint::spin_loop();}}
#[unsafe(no_mangle)] pub extern "C" fn probe_entry()->!{let _=core::hint::black_box(foundation_probe::exercise(core::hint::black_box(0)));loop{core::hint::spin_loop();}}
''');linker.write_text(LINKER)
    run(['rustc',str(entry),*aflags,'--extern','foundation_probe='+str(library),'-Clto=fat',
         '-Clink-arg=-T'+str(linker),'-o',str(elf)])
    return {'host_outputs':outputs,'layout':layout,'sections':linked_sections(elf.read_bytes()),
            'owner_total':sum(layout[k] for k in ['APP_UI','APP_DEMO','APP_SOURCE','APP_SINK']),
            'exercise_frame':exercise[0],'max_local_frame':max(frames.values()),'local_frames':frames}


def main():
    fixed_environment();OUT.mkdir(parents=True,exist_ok=True)
    run(['rustfmt','--edition','2024','--check',str(PROBE)])
    run(['cargo','fmt','--manifest-path',str(MANIFEST),'--','--check'])
    host=next(x.removeprefix('host: ') for x in run(['rustc','-vV']).splitlines() if x.startswith('host: '))
    cases={}
    for small in [False,True]:
        name='small-memory' if small else 'default';features=['--features','small-memory'] if small else []
        validate_metadata(json.loads(cargo(['metadata','--format-version','1'],name+'-metadata.log',features)),small)
        cargo(['test','--all-targets'],name+'-tests.log',features)
        cargo(['test','--doc'],name+'-doctests.log',features)
        cargo(['clippy','--all-targets',*features,'--','-D','warnings'],name+'-lint.log',[])
        cargo(['clippy','--lib','--target',TARGET,*features,'--','-D','warnings'],name+'-arm-lint.log',[])
        cargo(['doc','--no-deps'],name+'-docs.log',features)
        image=OUT/(name+'.ppm')
        replay=json.loads(cargo(['run','--example','replay',*features,'--',str(image)],name+'-replay.log',[]))
        validate_image(image.read_bytes())
        case=measure(name,host,build(host,small),build(TARGET,small))
        case.update(replay=replay,image_sha256=hashlib.sha256(image.read_bytes()).hexdigest())
        cases[name]=case
        print(name+': replay, host/ARM checks and linked owner/frame evidence collected')
    validate_evidence(cases)
    report={'environment':{'compiler':run(['rustc','--version']).strip(),'target':TARGET,'profile':PROFILE,
                           'cargo_configuration':build_configuration(ROOT)},
            'source_sha256':{p.relative_to(ROOT).as_posix():hashlib.sha256(p.read_bytes()).hexdigest()
                             for p in sorted(APP.rglob('*.rs')) if 'target' not in p.relative_to(APP).parts},
            'manifest_sha256':hashlib.sha256(MANIFEST.read_bytes()).hexdigest(),
            'lock_sha256':hashlib.sha256((APP/'Cargo.lock').read_bytes()).hexdigest(),
            'probe_sha256':hashlib.sha256(PROBE.read_bytes()).hexdigest(),'cases':cases,
            'limits':'Synthetic host execution, Cortex-M0 link and local frames in this monomorphized consumer only. Owned layouts overlap nested fields and are not added to compiler frames. Excludes complete call chain, IRQ, pools, real clocks/radio and optical hardware behavior.'}
    (OUT/'report.json').write_text(json.dumps(report,indent=2,sort_keys=True)+'\n')
    print('Default/small outputs identical; report: target/device-foundation/report.json')


if __name__=='__main__':
    try:main()
    except (RuntimeError,ValueError,KeyError,OSError) as e:
        print(str(e),file=sys.stderr);sys.exit(1)
