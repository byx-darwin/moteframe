"""Offline regression checks for the SWD observer's reconnect requirement."""
from pathlib import Path
import shutil
import subprocess
import unittest


TCLSH = shutil.which("tclsh")
TOOLS = Path(__file__).resolve().parent


@unittest.skipUnless(TCLSH, "tclsh is required to exercise the SWD observer")
class NotifyTrialTests(unittest.TestCase):
    def test_reports_event_wake_counters_only_for_the_event_image(self):
        result = subprocess.run(
            [TCLSH],
            input=r"""
source gatt_notify_sram_trial.tcl
set ::notify_diag 0x20007d00
proc echo {message} {puts $message}
proc read_memory {address width count} {
    if {$address != 0x20007de4 || $width != 32 || $count != 4} {
        error "unexpected wake counter read"
    }
    return {4 5 9 0xb}
}
if {[catch {
    notify_report_wake
    set ::notify_event_driven 0
    notify_report_wake
    set ::notify_event_driven 1
    notify_report_wake
} message]} {
    puts stderr $message
    exit 1
}
""",
            cwd=TOOLS,
            text=True,
            capture_output=True,
            timeout=5,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, "WAKE_STATS RADIO_IRQ=4 TIMER_IRQ=5 WAIT_ATTEMPTS=9 MIN_LEAD_TICKS=0xb\n")

    def test_waits_for_fresh_tx_on_the_reconnected_session(self):
        output = self.run_observer(r"""
    {{4 1 0x39325246 0x3537544e 9 0 0 0 10 7 7 0 0 4 7 0 27} {7 1 0x39325246 0x3537544e}}
    {{4 2 0x39325246 0x3537544e 9 0 0 0 10 8 8 0 0 4 8 0 27} {8 1 0x39325246 0x3537544e}}
    {{2 2 0x39325246 0x3537544e 9 1 0 0 11 8 8 0 0 4 8 0 27} {8 1 0x39325246 0x3537544e}}
    {{4 3 0x39325246 0x32304352 9 1 0 0 15 9 9 0 0 5 9 0 27} {9 2 0x39325246 0x32304352}}
""")
        self.assertIn("NOTIFY_ACK SESSION=1 TOKEN=7", output)
        self.assertIn("NOTIFY_ACK SESSION=2 TOKEN=9", output)
        self.assertEqual(output.count("NOTIFY_ACK "), 2)

    def test_uses_the_notification_ack_even_if_advertising_was_not_sampled(self):
        output = self.run_observer(r"""
    {{4 1 0x39325246 0x3537544e 9 0 0 0 10 7 7 0 0 4 7 0 27} {7 1 0x39325246 0x3537544e}}
    {{4 2 0x39325246 0x32304352 9 1 0 0 15 9 8 0 0 5 8 0 27} {7 1 0x39325246 0x3537544e}}
    {{4 2 0x39325246 0x32304352 9 1 0 0 15 10 9 0 0 5 8 0 27} {10 2 0x39325246 0x32304352}}
""")
        self.assertIn("NOTIFY_ACK SESSION=1 TOKEN=7", output)
        self.assertIn("NOTIFY_ACK SESSION=2 TOKEN=10", output)
        self.assertEqual(output.count("NOTIFY_ACK "), 2)

    def run_observer(self, samples):
        result = subprocess.run(
            [TCLSH],
            input="""
source gatt_notify_sram_trial.tcl
set ::notify_diag 0x20007d00
set ::samples {
""" + samples + r"""
}
proc notify_diag {} {
    if {[llength $::samples] == 0} {error "missed the ACK on the second connection"}
    lassign [lindex $::samples 0] ::sample ::ack
    set ::samples [lrange $::samples 1 end]
    return $::sample
}
proc echo {message} {puts $message}
proc sleep {wait} {}
proc read_memory {address width count} {
    if {$address == 0x20007df8 && $width == 32 && $count == 1} {return [lindex $::ack 0]}
    if {$address == 0x20007d84 && $width == 32 && $count == 3} {return [lrange $::ack 1 3]}
    if {$address == 0x400101ea && $width == 16 && $count == 1} {return 5}
    if {$address == 0x40011984 && $width == 8 && $count == 27} {return {}}
    error "unexpected read"
}
if {[catch {notify_wait_for_tx 2 2} message] || [llength $::samples] != 0} {
    puts stderr "Expected two ACKed notifications. $message"
    exit 1
}
""",
            cwd=TOOLS,
            text=True,
            capture_output=True,
            timeout=5,
        )
        self.assertEqual(result.returncode, 0, result.stderr)
        return result.stdout


if __name__ == "__main__":
    unittest.main()
