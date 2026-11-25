"""Offline regression checks for the SWD observer's reconnect requirement."""
from pathlib import Path
import shutil
import subprocess
import unittest


TCLSH = shutil.which("tclsh")
TOOLS = Path(__file__).resolve().parent


@unittest.skipUnless(TCLSH, "tclsh is required to exercise the SWD observer")
class NotifyTrialTests(unittest.TestCase):
    def test_waits_for_fresh_tx_on_the_reconnected_session(self):
        result = subprocess.run(
            [TCLSH],
            input=r"""
source gatt_notify_sram_trial.tcl
set ::samples {
    {4 1 0x39325246 0x3537544e 9 0 0 0 10 7 7 5 2 4 7 0 27}
    {2 1 0x39325246 0x3537544e 9 1 0 0 11 7 7 5 2 4 7 0 27}
    {4 1 0x39325246 0x3537544e 9 1 0 0 12 7 7 5 2 4 7 0 27}
    {4 2 0x39325246 0x32304352 9 1 0 0 15 9 9 6 2 5 8 0 27}
}
proc notify_diag {} {
    if {[llength $::samples] == 0} {error "ended without verifying the second session"}
    set ::sample [lindex $::samples 0]
    set ::samples [lrange $::samples 1 end]
    return $::sample
}
proc echo {message} {puts $message}
proc sleep {wait} {}
proc read_memory {address width count} {
    set payload {0xb 0x0 0x4 0x0 0x1b 0x9 0x0}
    foreach word [lrange $::sample 2 3] {
        for {set shift 0} {$shift < 32} {incr shift 8} {
            lappend payload [format 0x%x [expr {($word >> $shift) & 0xff}]]
        }
    }
    return $payload
}
if {[catch {notify_wait_for_tx 2 2} message] || [llength $::samples] != 0} {
    puts stderr "Expected two fresh notifications. $message"
    exit 1
}
""",
            cwd=TOOLS,
            text=True,
            capture_output=True,
            timeout=5,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.count("TX_QUEUE_CONFIRMED "), 2)


if __name__ == "__main__":
    unittest.main()
