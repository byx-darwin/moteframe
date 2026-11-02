#!/usr/bin/env python3
"""Measure executed storage/BLE consumers without attributing full-stack costs."""
import json
import os
from pathlib import Path

from check_core_compatibility import (ROOT, TARGET, SEEDS, LINKER, context, fixed_environment,
                                      libraries, options, run)
from check_memory import read_layout, read_object
from check_ui_memory import linked_sections

PROBE = ROOT / 'scripts/probes/portable_modules.rs'
OUT = ROOT / 'target/portable-modules-delivery/resources'
MODES = {'storage-owned': 'storage_owned', 'storage-borrowed': 'storage_borrowed',
         'ble8': 'ble8', 'ble32': 'ble32', 'ble-borrowed8': 'ble_borrowed8'}


def validate_evidence(cases):
    if set(cases) != set(MODES):
        raise ValueError('missing portable module consumers')
    for case in cases.values():
        sections = case.get('sections', {})
        values = case.get('host_outputs', [])
        if (set(sections) != {'.text', '.rodata', '.data', '.bss'} or not sections['.text']
                or any(type(n) is not int or n < 0 for n in sections.values())
                or sections['.data'] or sections['.bss']):
            raise ValueError('invalid linked sections or unexpected static mutable storage')
        if (type(case.get('exercise_frame')) is not int or case['exercise_frame'] < 0
                or len(values) != len(SEEDS) or len(set(values)) < 2
                or any(type(n) is not int or not 0 <= n <= 0xffffffff for n in values)
                or not case.get('layout')):
            raise ValueError('missing executed behavior/layout/local-frame evidence')
    for left, right in [('storage-owned', 'storage-borrowed'), ('ble8', 'ble32'), ('ble8', 'ble-borrowed8')]:
        if cases[left]['host_outputs'] != cases[right]['host_outputs']:
            raise ValueError('equivalent consumers differ: ' + left + '/' + right)
    if cases['ble32']['layout']['MODULE_SLOT'] - cases['ble8']['layout']['MODULE_SLOT'] != 24:
        raise ValueError('inline slot layout does not reflect 8/32-byte capacity')


def build(target, folder):
    libs = libraries(ROOT, target, folder)
    settings = {'opt-level': '"s"', 'lto': '"fat"', 'codegen-units': '1', 'panic': '"abort"',
                'debug': '0', 'strip': '"none"', 'debug-assertions': 'false', 'overflow-checks': 'false',
                'incremental': 'false', 'split-debuginfo': '"off"', 'rpath': 'false'}
    config = [arg for key, value in settings.items() for arg in ['--config', f'profile.release.{key}={value}']]
    output = run(['cargo', 'build', '-p', 'moteframe-storage', '-p', 'moteframe-ble', '--lib', '--release',
                  '--target', target, '--target-dir', str(folder), '--offline', '--locked', '--message-format=json', *config])
    for line in output.splitlines():
        item = json.loads(line)
        if item.get('reason') == 'compiler-artifact' and item['target']['name'] in {'moteframe_storage', 'moteframe_ble'}:
            libs[item['target']['name']] = next(x for x in item['filenames'] if x.endswith('.rlib'))
    if not {'moteframe_storage', 'moteframe_ble'} <= set(libs):
        raise ValueError('missing actual new module libraries')
    return libs


def flags(target, libs, mode):
    return options(target, libs, []) + [arg for flag in [*MODES.values(), 'module_layout']
                                      for arg in ['--check-cfg', f'cfg({flag})']] + ['--cfg', MODES[mode]]


def measure(mode, host, host_libs, arm_libs):
    folder = OUT / mode
    folder.mkdir(parents=True, exist_ok=True)
    hflags, aflags = flags(host, host_libs, mode), flags(TARGET, arm_libs, mode)
    library = folder / 'libmodule_probe.rlib'
    run(['rustc', str(PROBE), '--crate-name', 'module_probe', '--crate-type', 'rlib', *hflags, '-o', str(library)])
    host_entry = folder / 'host.rs'
    host_entry.write_text('fn main(){println!("{:?}",' + str(SEEDS) + '.map(|s|module_probe::exercise(core::hint::black_box(s))));}\n')
    exe = folder / 'host'
    run(['rustc', str(host_entry), *hflags, '--extern', 'module_probe=' + str(library), '-Clto=fat', '-o', str(exe)])
    values = json.loads(run([str(exe)]))
    ir, obj = folder / 'probe.ll', folder / 'probe.o'
    run(['rustc', str(PROBE), '--crate-name', 'module_probe', '--crate-type', 'rlib', *aflags,
         '--cfg', 'module_layout', '-Zemit-stack-sizes', '--emit', f'link={library},llvm-ir={ir},obj={obj}'],
        env={**os.environ, 'RUSTC_BOOTSTRAP': '1'})
    names = {'MODULE_OWNER'} | ({'MODULE_SLOT'} if mode.startswith('ble') else set())
    layout = read_layout(ir.read_text(), names)
    frames = read_object(obj.read_bytes())['function_frames']
    entry_frames = [size for symbol, size in frames.items() if symbol.endswith('8exercise')]
    if len(entry_frames) != 1:
        raise ValueError('missing/ambiguous portable consumer frame')
    run(['rustc', str(PROBE), '--crate-name', 'module_probe', '--crate-type', 'rlib', *aflags, '-o', str(library)])
    entry, linker, elf = folder / 'entry.rs', folder / 'link.ld', folder / 'consumer.elf'
    entry.write_text('''#![no_std]
#![no_main]
#[panic_handler] fn panic(_: &core::panic::PanicInfo)->!{loop{core::hint::spin_loop();}}
#[unsafe(no_mangle)] pub extern "C" fn probe_entry()->!{let _=core::hint::black_box(module_probe::exercise(core::hint::black_box(0)));loop{core::hint::spin_loop();}}
''')
    linker.write_text(LINKER)
    run(['rustc', str(entry), *aflags, '--extern', 'module_probe=' + str(library), '-L', 'dependency=' + str(folder),
         '-Clto=fat', '-Clink-arg=-T' + str(linker), '-o', str(elf)])
    return {'host_outputs': values, 'layout': layout, 'exercise_frame': entry_frames[0],
            'sections': linked_sections(elf.read_bytes()), 'local_frames': frames}


def main():
    fixed_environment()
    run(['rustfmt', '--edition', '2024', '--check', str(PROBE)])
    host = next(x.removeprefix('host: ') for x in run(['rustc', '-vV']).splitlines() if x.startswith('host: '))
    hlibs, alibs = build(host, OUT / 'host-build'), build(TARGET, OUT / 'arm-build')
    cases = {name: measure(name, host, hlibs, alibs) for name in MODES}
    validate_evidence(cases)
    import hashlib
    environment = context()
    environment['probe_sha256'] = hashlib.sha256(PROBE.read_bytes()).hexdigest()
    report = {'environment': environment, 'cases': cases,
              'limits': 'Fixed Cortex-M0 consumers/local frames; excludes physical storage, BLE stack, firmware call-chain/IRQ stack and hardware qualification.'}
    (OUT / 'report.json').write_text(json.dumps(report, indent=2, sort_keys=True) + '\n')
    print('Portable storage/BLE resource evidence: ' + str((OUT / 'report.json').relative_to(ROOT)))


if __name__ == '__main__':
    main()
