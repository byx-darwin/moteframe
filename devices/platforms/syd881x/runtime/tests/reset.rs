//! Reset profiles must preserve exact write order and keep the vendor path distinct.
use syd881x_runtime::reset::{ResetRegisters, disable_watchdog, prepare_native_lab};
#[derive(Debug, Default)]
struct Port {
    events: Vec<String>,
    watchdog: u8,
}
impl ResetRegisters for Port {
    fn disable_interrupts(&mut self) {
        self.events.push("disable".into());
    }
    fn clear_pending(&mut self) {
        self.events.push("clear".into());
    }
    fn stop_systick(&mut self) {
        self.events.push("timer".into());
    }
    fn watchdog(&mut self) -> u8 {
        self.events.push("read-wdt".into());
        self.watchdog
    }
    fn set_watchdog(&mut self, value: u8) {
        self.events.push(format!("wdt-{value}"));
        self.watchdog = value;
    }
}
#[test]
fn test_should_preserve_native_reset_order_and_reserved_watchdog_bits() {
    let mut p = Port {
        watchdog: 0xfb,
        ..Port::default()
    };
    prepare_native_lab(&mut p);
    assert_eq!(
        p.events,
        ["disable", "clear", "timer", "read-wdt", "wdt-248"]
    );
    assert_eq!(p.watchdog, 0xf8);
}
#[test]
fn test_should_not_reconfigure_native_irqs_or_timer_for_vendor_watchdog_only() {
    let mut p = Port {
        watchdog: 0xfe,
        ..Port::default()
    };
    disable_watchdog(&mut p);
    assert_eq!(p.events, ["read-wdt", "wdt-252"]);
    assert_eq!(p.watchdog, 0xfc);
}

impl syd881x_runtime::reset::CandidateResetRegisters for Port {
    fn clear_system_pending(&mut self, value: u32) {
        self.events.push(format!("system-pending-{value:08x}"));
    }
    fn clear_sleep_configuration(&mut self) {
        self.events.push("sleep-zero".into());
    }
    fn clear_system_priorities(&mut self) {
        self.events.push("priorities-zero".into());
    }
}
#[test]
fn test_should_normalize_candidate_exception_state_before_enabling_its_clock() {
    let mut p = Port {
        watchdog: 0xfb,
        ..Port::default()
    };
    syd881x_runtime::reset::prepare_candidate_entry(&mut p);
    assert_eq!(
        p.events,
        [
            "disable",
            "clear",
            "timer",
            "system-pending-0a000000",
            "sleep-zero",
            "priorities-zero",
            "read-wdt",
            "wdt-248"
        ]
    );
}
