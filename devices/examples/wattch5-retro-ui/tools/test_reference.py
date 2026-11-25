"""Reference snapshots must retain the model at capture time, not a later mutation."""
from pathlib import Path
import csv,subprocess,tempfile,unittest
class ReferenceTests(unittest.TestCase):
 def test_should_freeze_timer_and_notice_snapshots_before_followup_events(self):
  with tempfile.TemporaryDirectory() as directory:
   subprocess.run(['node','examples/wattch5-retro-ui/tools/reference.mjs',directory],check=True,stdout=subprocess.DEVNULL)
   rows=list(csv.reader((Path(directory)/'cases.csv').read_text().splitlines()))
   self.assertTrue(any(r[1]=='timer' and r[17]=='5999' and r[18]=='IDLE' for r in rows))
   self.assertTrue(any(r[1]=='request' and r[8]=='Restart' and r[9]=='preview' for r in rows))
if __name__=='__main__':unittest.main()
