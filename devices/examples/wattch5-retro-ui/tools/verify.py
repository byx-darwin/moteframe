"""Compare actual Rust output with the approved design; no original vendor assets involved."""
from pathlib import Path
import sys,json,hashlib
p=Path(sys.argv[1]);results=[]
for expected in sorted(p.glob('*.reference.rgb565')):
 actual=p/expected.name.replace('.reference.','.actual.');a=actual.read_bytes();b=expected.read_bytes()
 if len(a)!=32768 or a!=b:raise ValueError('Reference mismatch:'+expected.name)
 results.append({'id':expected.name.split('.')[0],'pixels':16384,'mismatches':0,'sha256':hashlib.sha256(a).hexdigest()})
if len(results)!=561:raise ValueError('Missing reference cases')
report={'states':len(results),'pixels':len(results)*16384,'mismatches':0,'results':results}
(p/'comparison.json').write_text(json.dumps(report,indent=2)+'\n');print('PASS',report['states'],'states,',report['pixels'],'pixels, zero mismatches')
