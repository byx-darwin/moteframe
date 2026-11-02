#!/usr/bin/env python3
"""Freeze/check fixed I2C consumers; no hardware, Git history or baseline rewriting in CI."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from check_core_compatibility import (ROOT, TARGET, SEEDS, LINKER, context, digest,
                                      fixed_environment, libraries, options, run)
from check_memory import read_layout, read_object
from check_ui_memory import linked_sections
from check_build_context import validate_context

OUT = ROOT / 'target/i2c-extensibility/consumers'
PROBE = ROOT / 'scripts/probes/i2c_compat.rs'
REFERENCE = ROOT / 'scripts/i2c-compatibility-baseline.json'
REFERENCE_SHA256 = 'f9f7035dcb72ab303a0ea92320c6992c371539f6277445489e1caa14b36c0adc'
SOURCE_SHA256 = 'a1bac9c7682f31c017dd67da2bdd8c13fbace4aa78fd352f462f40b5fc8ea2a7'
SOURCE = ROOT / 'crates/moteframe-i2c/src/lib.rs'
MODES = {'legacy': 'exercise', 'normal': 'exercise_normal', 'checked': 'exercise_checked', 'recover': 'exercise_recover'}


def load_reference(path=REFERENCE, expected_hash=REFERENCE_SHA256):
    data = path.read_bytes()
    if hashlib.sha256(data).hexdigest() != expected_hash:
        raise ValueError('frozen I2C reference checksum differs; explicit review required')
    reference = json.loads(data)
    if reference.get('baseline_source_sha256') != SOURCE_SHA256:
        raise ValueError('I2C baseline provenance differs')
    return reference


def validate(case):
    for key in ['exercise_frame', 'max_local_frame']:
        if type(case.get(key)) is not int or case[key] < 0:
            raise ValueError('invalid local frame evidence')
    sections = case.get('sections', {})
    if set(sections) != {'.text', '.rodata', '.data', '.bss'} or any(
            type(x) is not int or x < 0 for x in sections.values()) or not sections['.text']:
        raise ValueError('invalid linked sections')
    if not case.get('host_outputs') or not case.get('layout'):
        raise ValueError('missing behavior/layout evidence')


def compare(reference, report):
    if not reference.get('environment') or not report.get('environment'):
        raise ValueError('missing I2C build/workload context')
    validate_context(reference['environment'], report['environment'], REFERENCE_SHA256)
    before, after = (x['cases']['legacy'] for x in [reference, report])
    validate(before)
    validate(after)
    if any(before[key] != after[key] for key in ['host_outputs', 'layout']):
        raise ValueError('legacy I2C behavior/layout differs')
    def costs(case):
        s = case['sections']
        return {'flash': s['.text'] + s['.rodata'] + s['.data'],
                'static_ram': s['.data'] + s['.bss'],
                'exercise_frame': case['exercise_frame'], 'max_local_frame': case['max_local_frame']}
    old, new = costs(before), costs(after)
    delta = {key: new[key] - old[key] for key in old}
    if any(value > 0 for value in delta.values()):
        raise ValueError('legacy I2C resource regression: ' + json.dumps(delta))
    return delta


def build(target, folder):
    libs = libraries(ROOT, target, folder)
    settings = {'opt-level': '"s"', 'lto': '"fat"', 'codegen-units': '1', 'panic': '"abort"',
                'debug': '0', 'strip': '"none"', 'debug-assertions': 'false',
                'overflow-checks': 'false', 'incremental': 'false', 'split-debuginfo': '"off"', 'rpath': 'false'}
    config = [arg for k, v in settings.items() for arg in ['--config', f'profile.release.{k}={v}']]
    output = run(['cargo', 'build', '-p', 'moteframe-i2c', '--lib', '--release', '--target', target,
                  '--offline', '--locked', '--target-dir', str(folder), '--message-format=json', *config])
    for line in output.splitlines():
        item = json.loads(line)
        if item.get('reason') == 'compiler-artifact' and item['target']['name'] == 'moteframe_i2c':
            libs['moteframe_i2c'] = next(x for x in item['filenames'] if x.endswith('.rlib'))
    if 'moteframe_i2c' not in libs:
        raise ValueError('missing real I2C library')
    return libs


def flags(target, libs, cfg):
    return options(target, libs, []) + [arg for f in ['i2c_layout', 'i2c_normal', 'i2c_checked', 'i2c_recover']
                                      for arg in ['--check-cfg', 'cfg(' + f + ')']] + [arg for f in cfg for arg in ['--cfg', f]]


def measure(name, host, host_libs, arm_libs):
    folder = OUT / name
    folder.mkdir(parents=True, exist_ok=True)
    function = MODES[name]
    cfg = [] if name == 'legacy' else ['i2c_' + name]
    hflags, aflags = flags(host, host_libs, cfg), flags(TARGET, arm_libs, cfg)
    library = folder / 'libi2c_probe.rlib'
    run(['rustc', str(PROBE), '--crate-name', 'i2c_probe', '--crate-type', 'rlib', *hflags, '-o', str(library)])
    entry = folder / 'host.rs'
    entry.write_text('fn main(){println!("{:?}",' + str(SEEDS) + '.map(|s|i2c_probe::' + function + '(core::hint::black_box(s))));}\n')
    exe = folder / 'host'
    run(['rustc', str(entry), *hflags, '--extern', 'i2c_probe=' + str(library), '-Clto=fat', '-o', str(exe)])
    outputs = json.loads(run([str(exe)]))
    ir, obj = folder / 'probe.ll', folder / 'probe.o'
    run(['rustc', str(PROBE), '--crate-name', 'i2c_probe', '--crate-type', 'rlib', *aflags,
         '--cfg', 'i2c_layout', '-Zemit-stack-sizes', '--emit', f'link={library},llvm-ir={ir},obj={obj}'],
        env={**os.environ, 'RUSTC_BOOTSTRAP': '1'})
    layout = read_layout(ir.read_text(), {'I2C_OWNER', 'I2C_PIN'})
    frames = read_object(obj.read_bytes())['function_frames']
    exercise = [size for symbol, size in frames.items() if symbol.endswith(str(len(function)) + function)]
    if len(exercise) != 1:
        raise ValueError('missing exercise frame')
    run(['rustc', str(PROBE), '--crate-name', 'i2c_probe', '--crate-type', 'rlib', *aflags, '-o', str(library)])
    entry = folder / 'entry.rs'
    entry.write_text('''#![no_std]
#![no_main]
#[panic_handler] fn panic(_: &core::panic::PanicInfo)->!{loop{core::hint::spin_loop();}}
#[unsafe(no_mangle)] pub extern "C" fn probe_entry()->!{let _=core::hint::black_box(i2c_probe::exercise(core::hint::black_box(0)));loop{core::hint::spin_loop();}}
'''.replace('i2c_probe::exercise(', 'i2c_probe::' + function + '('))
    linker, elf = folder / 'probe.ld', folder / 'consumer.elf'
    linker.write_text(LINKER)
    run(['rustc', str(entry), *aflags, '--extern', 'i2c_probe=' + str(library), '-L', 'dependency=' + str(folder),
         '-Clto=fat', '-Clink-arg=-T' + str(linker), '-o', str(elf)])
    case = {'host_outputs': outputs, 'layout': layout, 'sections': linked_sections(elf.read_bytes()),
            'exercise_frame': exercise[0], 'max_local_frame': max(frames.values())}
    validate(case)
    return case


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--record-baseline', action='store_true', help='pre-change capture into target only')
    args = parser.parse_args()
    fixed_environment()
    if args.record_baseline and digest(SOURCE) != SOURCE_SHA256:
        raise ValueError('baseline capture requires exact reviewed pre-change I2C source')
    run(['rustfmt', '--edition', '2024', '--check', str(PROBE)])
    environment = context()
    # Freeze the legacy workload separately; new cfg branches may be appended later.
    environment['probe_sha256'] = hashlib.sha256(PROBE.read_bytes().split(b'// EXTENSION PROBES')[0]).hexdigest()
    host = next(x.removeprefix('host: ') for x in run(['rustc', '-vV']).splitlines() if x.startswith('host: '))
    hlibs, alibs = build(host, OUT / 'host-build'), build(TARGET, OUT / 'arm-build')
    report = {'environment': environment, 'cases': {name: measure(name, host, hlibs, alibs) for name in (['legacy'] if args.record_baseline else MODES)},
              'baseline_source_sha256': SOURCE_SHA256, 'full_probe_sha256': digest(PROBE),
              'limits': 'Fixed synthetic Cortex-M0 linked consumer and object-local frames, not full firmware, IRQ stack or hardware.'}
    if not args.record_baseline:
        reference = load_reference()
        report['legacy_deltas'] = compare(reference, report)
        report['workspace_context_migration'] = validate_context(reference['environment'], report['environment'], REFERENCE_SHA256)
        if report['cases']['normal']['host_outputs'] != report['cases']['legacy']['host_outputs']:
            raise ValueError('normal address view changes legacy behavior')
    OUT.mkdir(parents=True, exist_ok=True)
    destination = OUT / ('baseline-capture.json' if args.record_baseline else 'report.json')
    destination.write_text(json.dumps(report, sort_keys=True, indent=2) + '\n')
    print('I2C compatibility and resources: ' + str(destination.relative_to(ROOT)))


if __name__ == '__main__':
    main()
