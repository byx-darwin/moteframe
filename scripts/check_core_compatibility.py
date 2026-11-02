#!/usr/bin/env python3
"""Verify real core consumers and fixed Cortex-M0 linked cost evidence offline."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess

from check_memory import read_layout, read_object
from check_ui_configuration import validate_build
from check_ui_memory import linked_sections
from check_build_context import validate_context

ROOT = Path(__file__).resolve().parents[1]
OUT = ROOT / 'target/core-extensibility-implementation/consumers'
REFERENCE = ROOT / 'scripts/core-compatibility-baseline.json'
REFERENCE_SHA256 = '0ed2ec51e890d2f5ab71e6eb5cf9c6625a121057edf40bbe7f595dc6f73562c7'
PROBE = ROOT / 'scripts/probes/core_compat.rs'
TARGET = 'thumbv6m-none-eabi'
REVISION = '8727225c79ffe8d993a1efc4419f42122ae365f1'
PROFILE = 'opt-level=s;codegen-units=1;panic=abort;fat-LTO;debug=0;strip=none;overflow-checks=false;debug-assertions=false;incremental=false;fixed-linker;no-layout-statics'
LEGACY = ('dedicated', 'shared')
MODES = {'dedicated': [], 'shared': ['core_shared'], 'hal-owned': ['core_hal'],
         'hal-borrowed': ['core_hal', 'core_borrowed'],
         'recover-small': ['core_hal', 'core_recover'],
         'recover-large': ['core_hal', 'core_recover', 'core_large']}
SEEDS = [0, 1, 2, 3, 4, 8, 65535, 4294967295]
BASELINE_MANIFESTS = ('Cargo.toml', *(f'crates/{crate}/Cargo.toml' for crate in
             ['moteframe-core', 'moteframe-display', 'moteframe-ui', 'moteframe-i2c']))
MANIFESTS = BASELINE_MANIFESTS
CURRENT_MANIFESTS = (*BASELINE_MANIFESTS, 'crates/moteframe-storage/Cargo.toml', 'crates/moteframe-ble/Cargo.toml')
BASELINE_SOURCES = (*BASELINE_MANIFESTS, 'Cargo.lock',
                    'crates/moteframe-core/src/lib.rs', 'crates/moteframe-core/src/read.rs',
                    'crates/moteframe-core/src/spi.rs', 'crates/moteframe-display/src/lib.rs',
                    'crates/moteframe-display/src/spi.rs', 'crates/moteframe-display/src/shared.rs')
LINKER = '''ENTRY(probe_entry)
MEMORY { FLASH (rx) : ORIGIN = 0x10000, LENGTH = 1M
 RAM (rw) : ORIGIN = 0x20000000, LENGTH = 64K }
SECTIONS { .text : { *(.text .text.*) } > FLASH
 .rodata : { *(.rodata .rodata.*) } > FLASH
 .data : { *(.data .data.*) } > RAM AT>FLASH
 .bss (NOLOAD) : { *(.bss .bss.*) } > RAM }
'''


def run(arguments, cwd=ROOT, env=None):
    result = subprocess.run(arguments, cwd=cwd, env=env, capture_output=True, text=True)
    if result.returncode:
        raise RuntimeError('command failed: ' + ' '.join(map(str, arguments)) + '\n' + result.stdout + result.stderr)
    return result.stdout


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def context(root=ROOT):
    return {'compiler': run(['rustc', '--version']).strip(), 'target': TARGET,
            'profile': PROFILE, 'lock_sha256': digest(root / 'Cargo.lock'),
            'probe_sha256': digest(PROBE), 'linker_sha256': hashlib.sha256(LINKER.encode()).hexdigest(),
            'cargo_configuration': build_configuration(root)}


def transport_only_config(path):
    """Only ignore clearly scoped registry/network tables, never parse build TOML loosely.

    Unknown headers or top-level/dotted/inline assignments are fingerprinted in
    full. Registry credentials/URLs are never copied into the public report.
    """
    inside = False
    for line in path.read_text().splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith('#'):
            continue
        if stripped.startswith('['):
            header = re.fullmatch(r'\[([\w.\"-]+)\]\s*(?:#.*)?', stripped)
            if not header or header[1].split('.')[0].strip('"') not in {
                    'source', 'registry', 'registries', 'net', 'http', 'credential-alias'}:
                return False
            inside = True
        elif not inside:
            return False
    return True


def build_configuration(root):
    """Fingerprint all workspace manifests and non-transport Cargo configuration."""
    config_hashes = []
    seen = set()
    cargo_home = Path(os.environ.get('CARGO_HOME', str(Path.home() / '.cargo')))
    # Cargo prefers legacy 'config' when both spellings exist; fingerprint both
    # conservatively. Precedence is kept, while private absolute paths stay out.
    for config_dir in [root / '.cargo', *(directory / '.cargo' for directory in root.parents), cargo_home]:
        for name in ['config', 'config.toml']:
            path = config_dir / name
            if path.is_file() and path.resolve() not in seen:
                seen.add(path.resolve())
                if not transport_only_config(path):
                    config_hashes.append(digest(path))
    return {'manifests': {name: digest(root / name) for name in CURRENT_MANIFESTS if (root / name).is_file()},
            'nontransport_configs_sha256': config_hashes}


def validate_provenance(reference):
    sources = reference.get('baseline_source_sha256')
    if reference.get('baseline_revision') != REVISION or not isinstance(sources, dict) or set(sources) != set(BASELINE_SOURCES):
        raise ValueError('missing or unexpected baseline source provenance')
    if any(not isinstance(value, str) or not re.fullmatch(r'[0-9a-f]{64}', value) or value == '0' * 64
           for value in sources.values()):
        raise ValueError('invalid baseline source digest')


def load_reference(path, expected_hash):
    """Normal checks cannot accept a silently rewritten or incomplete budget."""
    data = path.read_bytes()
    if hashlib.sha256(data).hexdigest() != expected_hash:
        raise ValueError('frozen reference file checksum differs; explicit baseline review required')
    reference = json.loads(data)
    if not isinstance(reference, dict):
        raise ValueError('invalid frozen reference')
    validate_provenance(reference)
    return reference


def fixed_environment():
    """Refuse unrecorded compiler/profile overrides instead of publishing bad deltas."""
    blocked = {'RUSTFLAGS', 'CARGO_ENCODED_RUSTFLAGS', 'CARGO_BUILD_RUSTFLAGS',
               'CARGO_TARGET_THUMBV6M_NONE_EABI_RUSTFLAGS', 'RUSTC', 'RUSTC_WRAPPER',
               'RUSTC_WORKSPACE_WRAPPER', 'RUSTC_BOOTSTRAP', 'CARGO_BUILD_RUSTC',
               'CARGO_BUILD_RUSTC_WRAPPER', 'CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER',
               'CARGO_INCREMENTAL', 'CARGO_BUILD_INCREMENTAL'}
    # Cargo's build.* aliases and host-specific target flags can change library
    # compilation even when direct `rustc --version` and the profile match.
    changed = sorted(key for key in os.environ if key in blocked or key.startswith('CARGO_PROFILE_')
                     or (key.startswith('CARGO_TARGET_') and key.endswith('_RUSTFLAGS')))
    if changed:
        raise ValueError('resource probe requires default compiler environment; unset: ' + ', '.join(changed))


def nonnegative(value):
    return type(value) is int and value >= 0


def validate_case(case):
    if not isinstance(case, dict):
        raise ValueError('missing consumer evidence')
    sections = case.get('sections')
    if (not isinstance(sections, dict) or set(sections) != {'.text', '.rodata', '.data', '.bss'}
            or any(not nonnegative(x) for x in sections.values()) or sections['.text'] == 0):
        raise ValueError('missing/invalid linked sections')
    layout = case.get('layout')
    if not isinstance(layout, dict) or not layout or any(not nonnegative(x) or x == 0 for x in layout.values()):
        raise ValueError('missing/invalid type layouts')
    outputs = case.get('host_outputs')
    if (not isinstance(outputs, list) or len(outputs) != len(SEEDS)
            or any(not nonnegative(x) or x > 0xffffffff for x in outputs) or len(set(outputs)) < 2):
        raise ValueError('missing/nonexecuted consumer checksums')
    for key in ['exercise_frame', 'max_local_frame']:
        if not nonnegative(case.get(key)):
            raise ValueError('missing/invalid local frame evidence')
    if case['max_local_frame'] < case['exercise_frame']:
        raise ValueError('inconsistent frame evidence')


def flash(case):
    return sum(case['sections'][name] for name in ['.text', '.rodata', '.data'])


def validate_wrappers(case):
    """Check the concrete wrapper against the owned or borrowed object it holds."""
    layout = case.get('layout', {})
    for name in ['SPI', 'PIN', 'DELAY']:
        for prefix in ['MF_', 'MA_']:
            wrapped, raw = layout.get(prefix + name), layout.get(prefix + 'RAW_' + name)
            if not nonnegative(wrapped) or wrapped == 0 or wrapped != raw:
                raise ValueError('wrapper size/alignment differs from its inner object: ' + prefix + name)


def compare_legacy(reference, report):
    """A bad or incomplete baseline never permits a size or behavior regression."""
    deltas = {}
    for data in [reference, report]:
        if not isinstance(data, dict) or data.get('format_version') != 1:
            raise ValueError('unsupported reference/report format')
        if not isinstance(data.get('cases'), dict) or not isinstance(data.get('environment'), dict):
            raise ValueError('missing consumer/build evidence')
    validate_provenance(reference)
    if not reference['environment']:
        raise ValueError('missing frozen build context')
    validate_context(reference['environment'], report['environment'], REFERENCE_SHA256)
    for name in LEGACY:
        before = reference['cases'].get(name)
        after = report['cases'].get(name)
        validate_case(before)
        validate_case(after)
        if before['host_outputs'] != after['host_outputs']:
            raise ValueError(name + ': behavior differs from baseline')
        if before['layout'] != after['layout']:
            raise ValueError(name + ': type layout differs from baseline')
        delta = {'flash_bytes': flash(after) - flash(before),
                 'static_ram_bytes': after['sections']['.data'] + after['sections']['.bss']
                 - before['sections']['.data'] - before['sections']['.bss'],
                 'exercise_frame': after['exercise_frame'] - before['exercise_frame'],
                 'max_local_frame': after['max_local_frame'] - before['max_local_frame']}
        if any(value > 0 for value in delta.values()):
            raise ValueError(name + ': legacy resource regression: ' + json.dumps(delta))
        if after['sections']['.data'] or after['sections']['.bss']:
            raise ValueError(name + ': unexpected static mutable storage')
        deltas[name] = delta
    return deltas


def libraries(root, target, folder):
    settings = {'opt-level': '"s"', 'lto': '"fat"', 'codegen-units': '1', 'panic': '"abort"',
                'debug': '0', 'strip': '"none"', 'debug-assertions': 'false',
                'overflow-checks': 'false', 'incremental': 'false', 'split-debuginfo': '"off"', 'rpath': 'false'}
    config = [argument for key, value in settings.items()
              for argument in ['--config', f'profile.release.{key}={value}']]
    output = run(['cargo', 'build', '-p', 'moteframe-core', '-p', 'moteframe-display', '--lib',
                  '--release', '--target', target, '--offline', '--locked', '--message-format=json',
                  '--target-dir', str(folder), *config], cwd=root)
    result = {}
    for line in output.splitlines():
        item = json.loads(line)
        if item.get('reason') == 'compiler-artifact' and item['target']['name'] in {'moteframe_core', 'moteframe_display', 'embedded_hal'}:
            result[item['target']['name']] = next(x for x in item['filenames'] if x.endswith('.rlib'))
    if set(result) != {'moteframe_core', 'moteframe_display', 'embedded_hal'}:
        raise ValueError('missing actual core/display/HAL libraries')
    return result


def options(target, libs, cfg):
    args = ['--edition=2024', '--target', target, '-Copt-level=s', '-Ccodegen-units=1',
            '-Cpanic=abort', '-Cembed-bitcode=yes', '-Cdebuginfo=0', '-Cstrip=none',
            '-Cdebug-assertions=no', '-Coverflow-checks=no']
    for name, path in libs.items():
        args += ['--extern', name + '=' + path]
    deps = Path(libs['moteframe_core']).parent
    args += ['-L', 'dependency=' + str(deps), '-L', 'dependency=' + str(deps / 'deps')]
    for flag in ['core_hal', 'core_borrowed', 'core_shared', 'core_recover', 'core_large', 'core_layout']:
        args += ['--check-cfg', 'cfg(' + flag + ')']
    for flag in cfg:
        args += ['--cfg', flag]
    return args


def measure(name, host, host_libs, arm_libs, folder):
    folder.mkdir(parents=True, exist_ok=True)
    cfg = MODES[name]
    host_flags = options(host, host_libs, cfg)
    host_library = folder / 'libcore_probe_host.rlib'
    run(['rustc', str(PROBE), '--crate-name', 'core_probe', '--crate-type', 'rlib', *host_flags, '-o', str(host_library)])
    harness = folder / 'host.rs'
    harness.write_text('fn main() { let values = ' + str(SEEDS) + '.map(|seed| core_probe::exercise(core::hint::black_box(seed))); println!("{:?}", values); }\n')
    executable = folder / 'host-consumer'
    run(['rustc', str(harness), '--crate-name', 'core_host_entry', *host_flags,
         '--extern', 'core_probe=' + str(host_library), '-Clto=fat', '-o', str(executable)])
    outputs = json.loads(run([str(executable)]))
    flags = options(TARGET, arm_libs, cfg)
    library, ir, obj = folder / 'libcore_probe.rlib', folder / 'probe.ll', folder / 'probe.o'
    run(['rustc', str(PROBE), '--crate-name', 'core_probe', '--crate-type', 'rlib', *flags,
         '--cfg', 'core_layout', '-Zemit-stack-sizes', '--emit', f'link={library},llvm-ir={ir},obj={obj}'],
        env={**os.environ, 'RUSTC_BOOTSTRAP': '1'})
    names = ['OWNER', 'DEVICE', 'SPI', 'PIN', 'DELAY', 'RAW_SPI', 'RAW_PIN', 'RAW_DELAY']
    if 'core_recover' in cfg:
        names += ['CREATE_ERROR', 'CREATE_RESULT']
    marker_names = {prefix + name for name in names for prefix in ['MF_', 'MA_']}
    layout = read_layout(ir.read_text(), marker_names)
    frames = read_object(obj.read_bytes())['function_frames']
    exercise = [size for name, size in frames.items() if '8exercise' in name]
    if len(exercise) != 1:
        raise ValueError('missing/ambiguous executed consumer frame')
    # Diagnostic layout markers must not contribute to final linked Flash.
    run(['rustc', str(PROBE), '--crate-name', 'core_probe', '--crate-type', 'rlib', *flags, '-o', str(library)])
    entry = folder / 'entry.rs'
    entry.write_text('''#![no_std]
#![no_main]
#[panic_handler] fn panic(_: &core::panic::PanicInfo)->! {loop{core::hint::spin_loop();}}
#[unsafe(no_mangle)] pub extern "C" fn probe_entry()->! {let _=core::hint::black_box(core_probe::exercise(core::hint::black_box(0)));loop{core::hint::spin_loop();}}
''')
    linker = folder / 'probe.ld'
    linker.write_text(LINKER)
    elf = folder / 'consumer.elf'
    run(['rustc', str(entry), '--crate-name', 'core_entry', *flags, '--extern', 'core_probe=' + str(library),
         '-L', 'dependency=' + str(folder), '-Clto=fat', '-Clink-arg=-T' + str(linker), '-o', str(elf)])
    case = {'host_outputs': outputs, 'layout': layout, 'sections': linked_sections(elf.read_bytes()),
            'exercise_frame': exercise[0], 'max_local_frame': max(frames.values()), 'local_frames': frames}
    validate_case(case)
    if case['sections']['.data'] or case['sections']['.bss']:
        raise ValueError(name + ': synthetic consumer unexpectedly allocates static mutable storage')
    return case


def external_consumers():
    """Compile ownership at the actual consuming source and a graphics-free color."""
    common = '''#![no_std]
use moteframe_core::{Display,Dimensions,Rect,OutputPin,spi::SharedBus};
use embedded_hal::{spi::{SpiBus,SpiDevice},delay::DelayNs};
'''
    borrowing = '''pub fn use_device<B:SpiBus,D:DelayNs,C:OutputPin>(owner:SharedBus<B,D>,select:C)
where C::Error:core::fmt::Debug {
let Ok(mut device)=owner.device(select) else {return};
RECLAIM
let _=device.write(&[1]);
}
'''
    examples = [
        ('custom-color', common + '''#[derive(Clone,Copy)] pub struct Ink(pub u8);
pub struct Screen;
impl Display for Screen {type Color=Ink;type Error=();fn dimensions(&self)->Dimensions{Dimensions::new(1,1)}fn write(&mut self,_:Rect,_:&[Ink])->Result<(),()>{Ok(())}}
pub fn use_color()->Result<(),()> {Screen.write(Rect::new(0,0,1,1),&[Ink(7)])}
''', True, None),
        ('owner-borrow-valid', common + borrowing.replace('RECLAIM', ''), True, None),
        ('owner-borrow', common + borrowing.replace('RECLAIM', 'let _=owner.into_parts();'), False, ('E0505', 'owner')),
    ]
    records = []
    for name, source, success, diagnostic in examples:
        directory = OUT / 'external' / name
        (directory / 'src').mkdir(parents=True, exist_ok=True)
        manifest = directory / 'Cargo.toml'
        manifest.write_text('[package]\nname="core-external-consumer"\nversion="0.0.0"\nedition="2024"\n[workspace]\n[dependencies]\n'
                            + 'moteframe-core={path=' + json.dumps(str(ROOT / 'crates/moteframe-core')) + '}\nembedded-hal="=1.0.0"\n')
        consumer_source = directory / 'src/lib.rs'
        consumer_source.write_text(source)
        run(['cargo', 'generate-lockfile', '--manifest-path', str(manifest), '--offline'])
        result = subprocess.run(['cargo', 'build', '--manifest-path', str(manifest), '--target', TARGET,
                                 '--target-dir', str(OUT / 'external-build'), '--offline', '--locked', '--message-format=json'],
                                capture_output=True, text=True)
        (directory / 'result.log').write_text(result.stdout + result.stderr)
        matched = validate_build(result, consumer_source, success, diagnostic)
        metadata = json.loads(run(['cargo', 'metadata', '--manifest-path', str(manifest), '--offline', '--locked', '--format-version', '1']))
        if {p['name'] for p in metadata['packages']} != {'core-external-consumer', 'moteframe-core', 'embedded-hal'}:
            raise ValueError('core consumer acquired an unexpected dependency')
        records.append({'name': name, 'success_expected': success, 'matched_errors': matched})
    return records


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--baseline-root', type=Path, help='measure legacy consumers against the reviewed original checkout; never rewrites the reference')
    args = parser.parse_args()
    fixed_environment()
    run(['rustfmt', '--edition', '2024', '--check', str(PROBE)])
    root = args.baseline_root.resolve() if args.baseline_root else ROOT
    if args.baseline_root:
        for path in BASELINE_SOURCES:
            original = subprocess.check_output(['git', 'show', REVISION + ':' + path], cwd=ROOT)
            if (root / path).read_bytes() != original:
                raise ValueError('baseline root differs from reviewed source: ' + path)
    prefix = OUT / ('baseline' if args.baseline_root else 'candidate')
    prefix.mkdir(parents=True, exist_ok=True)
    host = next(x.removeprefix('host: ') for x in run(['rustc', '-vV']).splitlines() if x.startswith('host: '))
    host_libs = libraries(root, host, prefix / 'host-build')
    arm_libs = libraries(root, TARGET, prefix / 'arm-build')
    report = {'format_version': 1, 'environment': context(root), 'cases': {},
              'limits': 'Fixed Cortex-M0 synthetic consumers and object-local frames, not complete firmware, worst call-chain/IRQ stack, hardware or power qualification.'}
    for name in (LEGACY if args.baseline_root else MODES):
        report['cases'][name] = measure(name, host, host_libs, arm_libs, prefix / name)
    if args.baseline_root:
        # Optional reproduction must really use the reviewed original source.
        # Normal CI consumes the frozen reference and needs no Git history.
        report['baseline_revision'] = REVISION
        report['baseline_source_sha256'] = {path: digest(root / path) for path in BASELINE_SOURCES}
    else:
        reference = load_reference(REFERENCE, REFERENCE_SHA256)
        report['legacy_deltas'] = compare_legacy(reference, report)
        report['workspace_context_migration'] = validate_context(reference['environment'], report['environment'], REFERENCE_SHA256)
        for case in report['cases'].values():
            validate_wrappers(case)
        for mode in ['hal-owned', 'hal-borrowed']:
            if report['cases'][mode]['host_outputs'] != report['cases']['dedicated']['host_outputs']:
                raise ValueError(mode + ': HAL bridge is not equivalent to correct manual bridge')
        report['external_consumers'] = external_consumers()
    destination = prefix / 'report.json'
    destination.write_text(json.dumps(report, indent=2, sort_keys=True) + '\n')
    print('Core compatibility and linked resource evidence: ' + str(destination.relative_to(ROOT)))


if __name__ == '__main__':
    main()
