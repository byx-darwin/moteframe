//! Preserve shared counter setup while keeping force-restart separate from acquisition.
use syd881x_runtime::counter::{CounterRegisters, TimerBusy, restart, try_start};

#[derive(Debug, Default)]
struct Port {
    control: u32,
    events: Vec<String>,
}
impl CounterRegisters for Port {
    fn control(&mut self) -> u32 {
        self.events.push("read-control".into());
        self.control
    }
    fn set_control(&mut self, value: u32) {
        self.events.push(format!("control-{value}"));
        self.control = value;
    }
    fn set_reload(&mut self, value: u32) {
        self.events.push(format!("reload-{value}"));
    }
    fn clear_current(&mut self) {
        self.events.push("clear-current".into());
    }
}

#[test]
fn test_should_refuse_running_or_interrupt_enabled_timer_without_writes() {
    for control in [1, 2, 3, 5, 7, 0x1_0002] {
        let mut port = Port {
            control,
            ..Port::default()
        };
        assert_eq!(try_start(&mut port), Err(TimerBusy));
        assert_eq!(port.control, control);
        assert_eq!(port.events, ["read-control"]);
    }
}

#[test]
fn test_should_preserve_checked_setup_order_and_ignore_countflag_when_disabled() {
    let mut port = Port {
        control: 0x1_0000,
        ..Port::default()
    };
    assert_eq!(try_start(&mut port), Ok(()));
    assert_eq!(
        port.events,
        [
            "read-control",
            "reload-16777215",
            "clear-current",
            "control-5"
        ]
    );
    assert_eq!(port.control, 5);
}

#[test]
fn test_should_stop_before_forced_restart_without_checked_acquire_side_effects() {
    let mut port = Port {
        control: 7,
        ..Port::default()
    };
    restart(&mut port);
    assert_eq!(
        port.events,
        ["control-0", "reload-16777215", "clear-current", "control-5"]
    );
    assert_eq!(port.control, 5);
}

impl syd881x_runtime::counter::IrqCounterRegisters for Port {
    fn clear_pending(&mut self) {
        self.events.push("clear-pending".into());
    }
}

#[test]
fn test_should_clear_stale_pending_before_enabling_irq_timer() {
    let mut port = Port::default();
    assert_eq!(syd881x_runtime::counter::try_start_irq(&mut port), Ok(()));
    assert_eq!(
        port.events,
        [
            "read-control",
            "reload-16777215",
            "clear-current",
            "clear-pending",
            "control-7"
        ]
    );
    // Both polling and IRQ acquisition reject the already owned counter.
    port.events.clear();
    assert_eq!(try_start(&mut port), Err(TimerBusy));
    assert_eq!(
        syd881x_runtime::counter::try_start_irq(&mut port),
        Err(TimerBusy)
    );
    assert_eq!(port.events, ["read-control", "read-control"]);
}

#[test]
fn test_should_refuse_irq_acquisition_without_clearing_another_owners_pending() {
    for control in [1, 2, 3, 5, 7] {
        let mut port = Port {
            control,
            ..Port::default()
        };
        assert_eq!(
            syd881x_runtime::counter::try_start_irq(&mut port),
            Err(TimerBusy)
        );
        assert_eq!(port.events, ["read-control"]);
    }
}
