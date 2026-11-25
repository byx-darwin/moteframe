#!/usr/bin/env python3
"""Run the independent service matrix; never touch hardware or Git."""
import hashlib,json,pathlib,subprocess
root=pathlib.Path(__file__).resolve().parent
features=['heart-rate','blood-pressure','oxygen','glucose','temperature','ecg','wellbeing','microcirculation']
results=[]
for label,flags in [('default',[]),('all',['--all-features']),*[(f,['--no-default-features','--features',f]) for f in features]]:
 for kind,args in [('test',['test']),('arm',['check','--target','thumbv6m-none-eabi']),('lint',['clippy','--all-targets'])]:
  cmd=['cargo',*args,'--manifest-path',str(root/'Cargo.toml'),*flags,'--locked']
  if kind=='lint':cmd+=['--','-D','warnings']
  run=subprocess.run(cmd,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT)
  (root/f'verify-{label}-{kind}.log').write_text(run.stdout)
  results.append({'label':label,'kind':kind,'command':cmd,'exit':run.returncode})
  print(label,kind,run.returncode,flush=True)
cmd=['cargo','fmt','--manifest-path',str(root/'Cargo.toml'),'--','--check']
run=subprocess.run(cmd,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT);(root/'verify-fmt.log').write_text(run.stdout);results.append({'label':'fmt','command':cmd,'exit':run.returncode})
files=[root/'Cargo.toml',root/'README.md',*sorted((root/'src').glob('*.rs')),*sorted((root/'tests').glob('*.rs'))]
(root/'verification.json').write_text(json.dumps({'commands':results,'sha256':{str(p.relative_to(root)):hashlib.sha256(p.read_bytes()).hexdigest() for p in files},'hardware_actions':0,'git_actions':0},indent=2)+'\n')
raise SystemExit(any(r['exit']!=0 for r in results))
