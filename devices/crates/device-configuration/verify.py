#!/usr/bin/env python3
"""Run bounded, offline configuration-crate verification and retain exact evidence locally."""
import datetime,hashlib,json,pathlib,subprocess
root=pathlib.Path(__file__).resolve().parent
section=(root/'Cargo.toml').read_text().split('[features]')[1].split('[dependencies]')[0]
features=[line.split('=')[0].strip() for line in section.splitlines() if '=' in line and not line.startswith('default')]
variants=[('default',[]),('all',['--all-features'])]+[(f,['--no-default-features','--features',f]) for f in features]
commands=[['cargo','fmt','--manifest-path',str(root/'Cargo.toml'),'--check']]
for name,flags in variants:
 for prefix in [['cargo','test'],['cargo','check'],['cargo','clippy','--all-targets'],['cargo','check','--target','thumbv6m-none-eabi'],['cargo','clippy','--target','thumbv6m-none-eabi','--lib']]:
  cmd=prefix+['--manifest-path',str(root/'Cargo.toml')]+flags
  if 'clippy' in prefix:cmd+=['--','-D','warnings']
  commands.append(cmd)
results=[]
with (root/'verification.log').open('w') as log:
 for cmd in commands:
  run=subprocess.run(cmd,cwd=root,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
  log.write('$ '+' '.join(cmd)+'\n'+run.stdout+'\n');log.flush()
  results.append({'command':cmd,'exit_code':run.returncode})
  if run.returncode:print('FAILED',' '.join(cmd));print(run.stdout[-5000:]);break
hashes={str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted(root.rglob('*')) if p.is_file() and ('src' in p.relative_to(root).parts or 'tests' in p.relative_to(root).parts or p.name=='Cargo.toml')}
report={'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'scope':'Author verification snapshot; no hardware, GATT, persistence, medical algorithm or authorization acceptance','features':features,'expected_commands':len(commands),'executed_commands':len(results),'passed':all(r['exit_code']==0 for r in results) and len(results)==len(commands),'results':results,'sha256':hashes}
(root/'verification.json').write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps({k:report[k] for k in ['expected_commands','executed_commands','passed']}))
raise SystemExit(0 if report['passed'] else 1)
