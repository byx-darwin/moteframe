#!/usr/bin/env python3
"""Offline feature/Host/ARM verification. Never touch hardware or Git."""
import hashlib
import json
import pathlib
import subprocess

root = pathlib.Path(__file__).resolve().parent
features = ['rri', 'uv', 'womens-health']
results = []
for label, flags in [('default', []), ('all', ['--all-features'])] + [
        (name, ['--no-default-features', '--features', name]) for name in features]:
    for kind, args in [('test', ['test']), ('lint', ['clippy', '--all-targets']),
                       ('arm', ['build', '--target', 'thumbv6m-none-eabi']),
                       ('arm-lint', ['clippy', '--target', 'thumbv6m-none-eabi'])]:
        cmd = ['cargo'] + args + ['--manifest-path', str(root/'Cargo.toml'), '--locked'] + flags
        if kind.endswith('lint'):
            cmd += ['--', '-D', 'warnings']
        run = subprocess.run(cmd, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
        (root/('verify-'+label+'-'+kind+'.log')).write_text(run.stdout)
        results.append({'label': label, 'kind': kind, 'command': cmd, 'exit': run.returncode})
        print(label, kind, run.returncode, flush=True)
cmd = ['cargo', 'fmt', '--manifest-path', str(root/'Cargo.toml'), '--check']
run = subprocess.run(cmd, text=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
(root/'verify-fmt.log').write_text(run.stdout)
results.append({'label': 'fmt', 'command': cmd, 'exit': run.returncode})
files = [root/'Cargo.toml', root/'Cargo.lock', root/'README.md', root/'verify.py']
files += sorted((root/'src').glob('*.rs')) + sorted((root/'tests').glob('*.rs'))
(root/'verification.json').write_text(json.dumps({
    'commands': results,
    'sha256': {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest() for p in files},
    'hardware_actions': 0, 'git_actions': 0,
}, indent=2)+'\n')
raise SystemExit(any(r['exit'] != 0 for r in results))
