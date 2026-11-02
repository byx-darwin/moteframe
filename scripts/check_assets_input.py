#!/usr/bin/env python3
"""Executed asset/input consumers, old API compatibility and explicit memory costs."""
import hashlib
import json
import os
from pathlib import Path

from check_core_compatibility import (ROOT, TARGET, SEEDS, LINKER, context,
                                      fixed_environment, libraries, options, run, validate_case)
from check_memory import read_layout, read_object
from check_ui_memory import linked_sections

PROBE = ROOT / 'scripts/probes/assets_input.rs'
OUT = ROOT / 'target/assets-input/resources'
MODES = {'off': 'ai_off', 'assets2': 'ai_assets2', 'assets16': 'ai_assets16',
         'input1': 'ai_input1', 'input4': 'ai_input4', 'input16': 'ai_input16'}


def validate_evidence(cases):
    if set(cases) != set(MODES):
        raise ValueError('missing asset/input consumers')
    for case in cases.values():
        validate_case({'format_version': 1, **case})
        if case['sections']['.data'] or case['sections']['.bss']:
            raise ValueError('unexpected static mutable buffers')
    for mode, size in [('assets2', 2), ('assets16', 16)]:
        layout = cases[mode]['layout']
        if layout.get('MODULE_SCRATCH') != size or layout.get('MODULE_PIXELS') != 64:
            raise ValueError('caller scratch/output capacity is not reflected in layout')
    if cases['assets2']['host_outputs'] != cases['assets16']['host_outputs']:
        raise ValueError('scratch size changes decoded pixels')
    for key in ['MODULE_QUEUE', 'MODULE_TOUCH_QUEUE']:
        if cases['input16']['layout'].get(key, 0) <= cases['input1']['layout'].get(key, 0):
            raise ValueError('larger queue does not retain actual inline storage')
    if any(cases['input1']['host_outputs'] != cases[mode]['host_outputs'] for mode in ['input4', 'input16']):
        raise ValueError('drained input workload changed with queue capacity')


def build(target, folder):
    libs = libraries(ROOT, target, folder)
    settings = {'opt-level': '"s"', 'lto': '"fat"', 'codegen-units': '1', 'panic': '"abort"',
                'debug': '0', 'strip': '"none"', 'debug-assertions': 'false', 'overflow-checks': 'false',
                'incremental': 'false', 'split-debuginfo': '"off"', 'rpath': 'false'}
    config = [arg for key, value in settings.items() for arg in ['--config', f'profile.release.{key}={value}']]
    wanted = {'moteframe_assets', 'moteframe_input', 'moteframe_storage', 'embedded_graphics_core'}
    output = run(['cargo', 'build', '-p', 'moteframe-assets', '-p', 'moteframe-input', '-p', 'moteframe-storage',
                  '--lib', '--release', '--target', target, '--target-dir', str(folder),
                  '--offline', '--locked', '--message-format=json', *config])
    for line in output.splitlines():
        item = json.loads(line)
        if item.get('reason') == 'compiler-artifact' and item['target']['name'] in wanted:
            libs[item['target']['name']] = next(x for x in item['filenames'] if x.endswith('.rlib'))
    if not wanted <= set(libs):
        raise ValueError('missing actual asset/input/storage libraries')
    return libs


def flags(target, libs, mode):
    return options(target, libs, []) + [arg for flag in [*MODES.values(), 'ai_layout']
                                      for arg in ['--check-cfg', f'cfg({flag})']] + ['--cfg', MODES[mode]]


def measure(mode, host, hlibs, alibs, suffix=''):
    folder = OUT / (mode + suffix)
    folder.mkdir(parents=True, exist_ok=True)
    hflags, aflags = flags(host, hlibs, mode), flags(TARGET, alibs, mode)
    library = folder / 'libai_probe.rlib'
    run(['rustc', str(PROBE), '--crate-name', 'ai_probe', '--crate-type', 'rlib', *hflags, '-o', str(library)])
    harness = folder / 'host.rs'
    harness.write_text('fn main(){println!("{:?}",' + str(SEEDS) + '.map(|s|ai_probe::exercise(core::hint::black_box(s))));}\n')
    exe = folder / 'host'
    run(['rustc', str(harness), *hflags, '--extern', 'ai_probe=' + str(library), '-Clto=fat', '-o', str(exe)])
    outputs = json.loads(run([str(exe)]))
    ir, obj = folder / 'probe.ll', folder / 'probe.o'
    run(['rustc', str(PROBE), '--crate-name', 'ai_probe', '--crate-type', 'rlib', *aflags,
         '--cfg', 'ai_layout', '-Zemit-stack-sizes', '--emit', f'link={library},llvm-ir={ir},obj={obj}'],
        env={**os.environ, 'RUSTC_BOOTSTRAP': '1'})
    names = ({'MODULE_OWNER', 'MODULE_GLYPH', 'MODULE_READER', 'MODULE_SCRATCH', 'MODULE_PIXELS'} if mode.startswith('assets') else
             {'MODULE_BUTTON', 'MODULE_TOUCH', 'MODULE_QUEUE', 'MODULE_TOUCH_QUEUE'} if mode.startswith('input') else {'MODULE_OWNER'})
    layout = read_layout(ir.read_text(), names)
    frames = read_object(obj.read_bytes())['function_frames']
    exercise = [size for symbol, size in frames.items() if symbol.endswith('8exercise')]
    if len(exercise) != 1:
        raise ValueError('missing or ambiguous executed consumer frame')
    run(['rustc', str(PROBE), '--crate-name', 'ai_probe', '--crate-type', 'rlib', *aflags, '-o', str(library)])
    entry, linker, elf = folder / 'entry.rs', folder / 'link.ld', folder / 'consumer.elf'
    entry.write_text('''#![no_std]
#![no_main]
#[panic_handler] fn panic(_: &core::panic::PanicInfo)->!{loop{core::hint::spin_loop();}}
#[unsafe(no_mangle)] pub extern "C" fn probe_entry()->!{let _=core::hint::black_box(ai_probe::exercise(core::hint::black_box(0)));loop{core::hint::spin_loop();}}
''')
    linker.write_text(LINKER)
    run(['rustc', str(entry), *aflags, '--extern', 'ai_probe=' + str(library), '-L', 'dependency=' + str(folder),
         '-Clto=fat', '-Clink-arg=-T' + str(linker), '-o', str(elf)])
    return {'host_outputs': outputs, 'layout': layout, 'sections': linked_sections(elf.read_bytes()),
            'exercise_frame': exercise[0], 'max_local_frame': max(frames.values()), 'local_frames': frames}


def validate_graph():
    graph = json.loads(run(['cargo', 'metadata', '--offline', '--locked', '--format-version', '1']))
    for package in graph['packages']:
        if package['name'] in {'moteframe-core', 'moteframe-ui', 'moteframe-display', 'moteframe-ble', 'moteframe-storage', 'moteframe-i2c'}:
            if any(dep['name'] in {'moteframe-assets', 'moteframe-input'} for dep in package['dependencies']):
                raise ValueError('old components acquired new module dependencies')
        if package['name'] == 'moteframe-input' and package['dependencies']:
            raise ValueError('input is no longer independent')


def main():
    fixed_environment()
    validate_graph()
    run(['rustfmt', '--edition', '2024', '--check', str(PROBE)])
    host = next(x.removeprefix('host: ') for x in run(['rustc', '-vV']).splitlines() if x.startswith('host: '))
    hlibs, alibs = build(host, OUT / 'host-build'), build(TARGET, OUT / 'arm-build')
    cases = {mode: measure(mode, host, hlibs, alibs) for mode in MODES}
    validate_evidence(cases)
    excluded = {'moteframe_assets', 'moteframe_input', 'moteframe_storage'}
    off = measure('off', host, {k:v for k,v in hlibs.items() if k not in excluded},
                  {k:v for k,v in alibs.items() if k not in excluded}, '-without-new-libs')
    if any(off[key] != cases['off'][key] for key in ['host_outputs', 'sections', 'layout', 'exercise_frame']):
        raise ValueError('unused new modules contributed to the off consumer')
    report = {'environment': context(), 'probe_sha256': hashlib.sha256(PROBE.read_bytes()).hexdigest(),
              'source_sha256': {path.relative_to(ROOT).as_posix(): hashlib.sha256(path.read_bytes()).hexdigest()
                                for name in ['moteframe-assets', 'moteframe-input']
                                for path in sorted((ROOT / 'crates' / name / 'src').glob('*.rs'))},
              'cases': cases, 'off_without_new_libraries': off,
              'limits': 'Executed synthetic consumers and Cortex-M0 local frames only; excludes full firmware, transport pools, IRQ stack and hardware behavior.'}
    (OUT / 'report.json').write_text(json.dumps(report, indent=2, sort_keys=True) + '\n')
    print('Assets/input compatibility and resource evidence: ' + str((OUT / 'report.json').relative_to(ROOT)))


if __name__ == '__main__':
    main()
