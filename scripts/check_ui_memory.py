#!/usr/bin/env python3
"""Measure UI configuration layouts and linked fixed consumers on ARM, offline."""
from pathlib import Path
import json
import os
import struct
import subprocess
from check_memory import read_layout, read_object
from check_ui_configuration import effective

ROOT = Path(__file__).resolve().parents[1]
TARGET = 'thumbv6m-none-eabi'
OUT = ROOT / 'target/ui-config-implementation/memory'
BASE_NAMES = {'MF_'+n for n in ['SETTINGS_565','SETTINGS_888','SETTINGS_GRAY','SETTINGS_BINARY','STATE','CONTEXT','RESOURCE_KEY','RESOURCE_META','RESOURCE_VIEW','RUN_8','RUN_32','WORKSPACE']}
COMBINATIONS = {'base': [], 'layout': ['layout'], 'refresh': ['refresh'], 'damage': ['damage'], 'pacing': ['frame-pacing'], 'default': ['layout','refresh','damage'], 'all': ['layout','refresh','damage','frame-pacing']}


def linked_sections(data):
    """Require a valid linked ARM ELF32 and classify allocated sections only."""
    if len(data) < 52 or data[:6] != b'\x7fELF\x01\x01':
        raise ValueError('expected ELF32 little endian')
    kind, machine, _, _, _, offset, _, _, _, _, stride, count, strings = struct.unpack_from('<HHIIIIIHHHHHH', data, 16)
    if kind != 2 or machine != 40 or stride != 40 or not count or strings >= count or offset + count*stride > len(data):
        raise ValueError('invalid linked ARM section table')
    sections = [struct.unpack_from('<IIIIIIIIII', data, offset+i*stride) for i in range(count)]
    start, size = sections[strings][4:6]
    if start+size > len(data):
        raise ValueError('invalid section names')
    names = data[start:start+size]
    result = {'.text':0,'.rodata':0,'.data':0,'.bss':0}
    for section in sections:
        name, section_type, flags, _, start, size, *_ = section
        end = names.find(b'\0', name)
        if name >= len(names) or end < 0:
            raise ValueError('invalid section name')
        if section_type != 8 and start+size > len(data):
            raise ValueError('truncated section')
        if not flags & 2:
            continue
        key = '.bss' if section_type == 8 else '.data' if flags & 1 else '.text' if flags & 4 else '.rodata'
        result[key] += size
    return result


def run(args, env=None):
    result = subprocess.run(args,cwd=ROOT,capture_output=True,text=True,env=env)
    if result.returncode:
        raise RuntimeError('failed: '+' '.join(map(str,args))+'\n'+result.stdout+result.stderr)
    return result.stdout


def main():
    OUT.mkdir(parents=True,exist_ok=True)
    source=ROOT/'crates/moteframe-ui/examples/config_memory.rs'
    report={'compiler':run(['rustc','--version']).strip(),'target':TARGET,'combinations':{},'limits':'Fixed synchronous RGB565 consumers with LTO, not a firmware or IRQ/full-call-chain stack bound. Diagnostic frame metadata uses RUSTC_BOOTSTRAP only in direct rustc. No allocator, resource contents or product code linked.'}
    for name, features in COMBINATIONS.items():
        folder=OUT/name;folder.mkdir(exist_ok=True)
        flags=['--no-default-features']+(['--features',','.join(features)] if features else [])
        host=json.loads(run(['cargo','run','-p','moteframe-ui','--example','config_memory','--offline','--locked',*flags]))
        artifacts=run(['cargo','build','-p','moteframe-ui','--lib','--release','--target',TARGET,'--offline','--locked','--message-format=json',*flags])
        libraries={}
        for line in artifacts.splitlines():
            record=json.loads(line)
            if record.get('reason')=='compiler-artifact' and record['target']['name'] in {'moteframe_ui','moteframe_core'}:
                libraries[record['target']['name']]=next(f for f in record['filenames'] if f.endswith('.rlib'))
        if len(libraries)!=2:
            raise ValueError('missing libraries')
        deps=Path(libraries['moteframe_ui']).parent
        common=['--edition=2024','--target',TARGET,'-Copt-level=s','-Cpanic=abort','-Cembed-bitcode=yes','-L','dependency='+str(deps),'-L','dependency='+str(deps/'deps')]
        for library,path in libraries.items():
            common+=['--extern',library+'='+path]
        cfg=[]
        for feature in sorted(effective(features)):
            cfg+=['--cfg',f'feature="{feature}"']
        env={**os.environ,'RUSTC_BOOTSTRAP':'1'}
        lib=folder/'libconfig_probe.rlib';ir=folder/'probe.ll';obj=folder/'probe.o'
        run(['rustc',str(source),'--crate-name','config_probe','--crate-type','rlib',*common,*cfg,'--cfg','ui_layout_probe','-Zemit-stack-sizes','--emit',f'link={lib},llvm-ir={ir},obj={obj}'],env)
        names=set(BASE_NAMES)
        present=effective(features)
        if 'refresh' in present:
            names|={'MF_SESSION','MF_POLICY','MF_DEFERRED'}
        if 'frame-pacing' in present:
            names.add('MF_PACING')
        layout=read_layout(ir.read_text(),names)
        # Layout statics are compiler evidence, not runtime data. Recompile the
        # same consumer without them so LTO cannot retain measurements in Flash.
        run(['rustc',str(source),'--crate-name','config_probe','--crate-type','rlib',*common,*cfg,'--emit',f'link={lib}'])
        harness=folder/'entry.rs'
        harness.write_text('''#![no_std]
#![no_main]
#[panic_handler] fn panic(_: &core::panic::PanicInfo)->! {loop{core::hint::spin_loop();}}
#[unsafe(no_mangle)] pub extern "C" fn probe_entry()->! {let _ = core::hint::black_box(config_probe::exercise(core::hint::black_box(1)));loop{core::hint::spin_loop();}}
''')
        linker=folder/'probe.ld'
        linker.write_text('ENTRY(probe_entry)\nMEMORY { FLASH (rx) : ORIGIN = 0x10000, LENGTH = 1M\n RAM (rw) : ORIGIN = 0x20000000, LENGTH = 64K }\nSECTIONS { .text : { *(.text .text.*) } > FLASH .rodata : { *(.rodata .rodata.*) } > FLASH .data : { *(.data .data.*) } > RAM AT>FLASH .bss (NOLOAD) : { *(.bss .bss.*) } > RAM }\n')
        executable=folder/'consumer.elf'
        run(['rustc',str(harness),'--crate-name','ui_memory_entry','--crate-type','bin',*common,'--extern','config_probe='+str(lib),'-L','dependency='+str(folder),'-Clto=fat','-Clink-arg=-T'+str(linker),'-o',str(executable)])
        sections=linked_sections(executable.read_bytes())
        if not sections['.text'] or not host['checksum']:
            raise ValueError('empty/nonexecuted consumer')
        report['combinations'][name]={'requested_features':features,'effective_features':sorted(present),'host_layout':host,'arm_layout':layout,'linked_sections':sections,'flash_section_bytes':sections['.text']+sections['.rodata']+sections['.data'],'object_frames':read_object(obj.read_bytes())['function_frames']}
    (OUT/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print('UI configuration ARM layout/link/frame evidence: target/ui-config-implementation/memory/report.json')


if __name__=='__main__':
    main()
