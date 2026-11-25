#!/usr/bin/env python3
"""Offline host/ARM/feature verification; never touches devices or Git."""
from pathlib import Path
import hashlib
import json
import re
import subprocess
import sys

root = Path(__file__).resolve().parent
logs = root / 'verification'
logs.mkdir(exist_ok=True)
manifest = str(root / 'Cargo.toml')
results = []

def run(name, args):
    result = subprocess.run(args, cwd=root, text=True, stdout=subprocess.PIPE,
                            stderr=subprocess.STDOUT, check=False)
    (logs / (name + '.log')).write_text(result.stdout)
    results.append({'name': name, 'command': args, 'exit_code': result.returncode})
    print(name, result.returncode, flush=True)
    return result

for label, features in [('default', []), ('all', ['--all-features'])] + [
        (name, ['--features', name]) for name in
        ['raw-data', 'self-test', 'engineering', 'factory-config', 'device-jobs']]:
    base = ['--manifest-path', manifest, '--no-default-features'] + features
    run(label + '-host', ['cargo', 'test'] + base)
    run(label + '-arm', ['cargo', 'check'] + base + ['--target', 'thumbv6m-none-eabi'])
    run(label + '-lint', ['cargo', 'clippy'] + base + ['--all-targets', '--', '-D', 'warnings'])
    run(label + '-arm-lint', ['cargo', 'clippy'] + base + ['--lib', '--target', 'thumbv6m-none-eabi', '--', '-D', 'warnings'])
run('fmt', ['cargo', 'fmt', '--manifest-path', manifest, '--check'])
tree = run('commercial-tree', ['cargo', 'tree', '--manifest-path', manifest,
                             '--no-default-features', '--features', 'device-jobs', '-e', 'normal', '--prefix', 'none'])
isolated = tree.returncode == 0 and not any(
    line.startswith('device-maintenance v') for line in tree.stdout.splitlines())
results.append({'name': 'commercial-dependency-isolation', 'exit_code': 0 if isolated else 1})
files = [root / 'Cargo.toml', root / 'Cargo.lock', root / 'README.md', root / 'DESIGN.md', root / 'verify.py']
files += sorted((root / 'src').glob('*.rs')) + sorted((root / 'tests').glob('*.rs'))
frozen = root.parent / 'device-maintenance'
frozen_files = [frozen / 'Cargo.toml', *sorted((frozen / 'src').glob('*.rs'))]
all_tests = sum(map(int, re.findall(r'test result: ok\. (\d+) passed', (logs / 'all-host.log').read_text())))
report = {'results': results, 'all_feature_test_cases': all_tests, 'hardware_tested': False,
          'frozen_engine_source_sha256': {str(path.relative_to(frozen)): hashlib.sha256(path.read_bytes()).hexdigest() for path in frozen_files}, 'source_sha256': {
    str(path.relative_to(root)): hashlib.sha256(path.read_bytes()).hexdigest()
    for path in files}, 'all_passed': all(item['exit_code'] == 0 for item in results)}
(root / 'verification.json').write_text(json.dumps(report, indent=2) + '\n')
sys.exit(0 if report['all_passed'] else 1)
