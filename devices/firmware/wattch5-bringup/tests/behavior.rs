//! Real core and bounded input/readiness behavior.
#![allow(clippy::unwrap_used)]
use device_runtime::{PortError, Scope, SessionFactory, Work};
use device_services::{DeviceInfo, ServiceHandler};
use wattch5_bringup::{
    CoreFactory, Event, InputState, LinkStage, Readiness, ResetGate, UnboundStorage,
};
fn info() -> DeviceInfo {
    DeviceInfo {
        product_id: 0,
        fw_major: 0,
        fw_minor: 1,
        fw_patch: 0,
        build_id: 1,
    }
}
#[test]
fn test_should_allocate_fresh_subscription_sessions_without_wrapping() {
    let f = CoreFactory::new(info()).unwrap();
    for n in 1..=3 {
        let (c, h) = f.open(Scope::new(n, 20).unwrap()).unwrap();
        assert_eq!(c.session, n as u32);
        assert_eq!(c.modules.len(), 1);
        assert_eq!(c.modules[0].module, 0);
        assert_eq!(c.limits.data_max, 0);
        assert!(!h.authorized(
            device_protocol::Frame::decode(&[0x10, 2, 0x30, 0, 2, 0, 0, 0, 0, 0], 20, 128).unwrap()
        ));
    }
}
#[test]
fn test_should_retain_both_key_and_touch_and_reject_outside_coordinates() {
    let mut i = InputState::new();
    i.key(false, 0).unwrap();
    i.key(false, 30_000).unwrap();
    i.key(true, 40_000).unwrap();
    i.key(true, 70_000).unwrap();
    i.touch(Ok([5, 127, 126, 0]), 70_000).unwrap();
    assert_eq!(i.pop(), Some(Event::KeyRelease));
    assert_eq!(i.pop(), Some(Event::Tap { x: 127, y: 126 }));
    assert_eq!(
        i.touch(Ok([5, 128, 0, 0]), 300_000),
        Err(PortError::InvalidInput)
    );
    assert_eq!(i.pop(), None);
}
#[test]
fn test_should_backoff_faults_probe_without_irq_and_debounce_latches() {
    let mut i = InputState::new();
    assert!(i.due(false, 0));
    assert_eq!(i.touch(Err(PortError::Fault), 0), Err(PortError::Fault));
    assert!(!i.due(true, 499_999));
    assert!(i.due(false, 500_000));
    i.touch(Ok([1, 0, 0, 0]), 500_000).unwrap();
    i.touch(Ok([1, 0, 0, 0]), 510_000).unwrap();
    assert_eq!(i.pop(), Some(Event::Down));
    assert_eq!(i.pop(), None);
    i.touch(Ok([0; 4]), 520_000).unwrap();
    i.touch(Ok([1, 0, 0, 0]), 530_000).unwrap();
    assert_eq!(i.pop(), Some(Event::Down));
}
#[test]
fn test_should_report_queue_overflow_and_single_hold() {
    let mut i = InputState::new();
    i.key(false, 0).unwrap();
    i.key(false, 30_000).unwrap();
    i.key(false, 3_030_000).unwrap();
    i.key(false, 6_030_000).unwrap();
    assert_eq!(i.pop(), Some(Event::KeyLongPress));
    assert_eq!(i.pop(), None);
    for n in 0..4 {
        i.touch(Ok([2, 0, 0, 0]), n * 200_000).unwrap();
    }
    assert_eq!(
        i.touch(Ok([3, 0, 0, 0]), 1_000_000),
        Err(PortError::Unavailable)
    );
    for _ in 0..4 {
        assert_eq!(i.pop(), Some(Event::Up));
    }
}
#[test]
fn test_should_keep_unbound_providers_and_retire_ui_before_reset() {
    let mut bg = UnboundStorage;
    assert_eq!(bg.step(0), Err(PortError::Unavailable));
    let r = Readiness::new();
    assert_eq!(r.line(3), "STORAGE UNBOUND");
    assert_eq!(r.line(4), "UTC UNSYNC");
    assert_eq!(r.line(5), "SENSOR UNBOUND");
    assert_eq!(r.link(), LinkStage::Initializing);
    let g = ResetGate::new();
    assert!(!g.ui_allowed());
    g.completed();
    assert!(!g.ui_allowed());
    g.arm_ui().unwrap();
    assert!(g.ui_allowed());
    assert_eq!(g.begin(), Err(PortError::Fault));
    assert!(!g.ui_allowed());
    assert_eq!(g.arm_ui(), Err(PortError::Fault));
}

#[derive(Debug)]
struct Panel {
    writes: std::rc::Rc<std::cell::Cell<usize>>,
    fail: std::rc::Rc<std::cell::Cell<bool>>,
}
impl device_core::Display for Panel {
    type Error = ();
    fn dimensions(&self) -> device_core::Dimensions {
        device_core::Dimensions::new(128, 128)
    }
    fn write(&mut self, r: device_core::Rect, p: &[device_core::Rgb565]) -> Result<(), ()> {
        assert_eq!(r.height, 1);
        assert_eq!(p.len(), 128);
        if self.fail.get() {
            return Err(());
        }
        self.writes.set(self.writes.get() + 1);
        Ok(())
    }
}
#[derive(Debug)]
struct Inputs {
    calls: std::rc::Rc<std::cell::Cell<usize>>,
    fault: bool,
}
impl wattch5_bringup::InputSource for Inputs {
    fn sample(&mut self, _: u64) -> wattch5_bringup::InputReport {
        self.calls.set(self.calls.get() + 1);
        wattch5_bringup::InputReport {
            event: None,
            touch: wattch5_bringup::TouchStatus::Fault,
            fault: self.fault.then_some(PortError::Fault),
        }
    }
}
#[test]
fn test_should_preserve_failed_row_render_despite_touch_fault_and_retire_all_bus_access() {
    use std::{cell::Cell, rc::Rc};
    let writes = Rc::new(Cell::new(0));
    let calls = Rc::new(Cell::new(0));
    let fail = Rc::new(Cell::new(true));
    let gate = ResetGate::new();
    gate.completed();
    gate.arm_ui().unwrap();
    let status = Cell::new(Readiness::new());
    let mut ui = wattch5_bringup::ReadinessUi::new(
        Panel {
            writes: writes.clone(),
            fail: fail.clone(),
        },
        Inputs {
            calls: calls.clone(),
            fault: true,
        },
        &status,
        &gate,
    )
    .unwrap();
    assert_eq!(ui.step(0), Err(PortError::Fault));
    assert_eq!(ui.pending_row(), Some(0));
    fail.set(false);
    assert_eq!(ui.step(1), Err(PortError::Fault));
    assert_eq!(writes.get(), 1);
    assert_eq!(ui.pending_row(), Some(1));
    let mut live = status.get();
    live.set_link(LinkStage::Connected);
    status.set(live);
    for n in 2..129 {
        assert_eq!(ui.step(n), Err(PortError::Fault));
    }
    assert_eq!(ui.pending_row(), Some(0)); // coalesced follow-up retains unfinished newer damage
    assert_eq!(gate.begin(), Err(PortError::Fault));
    let before = (writes.get(), calls.get());
    assert_eq!(ui.step(200), Err(PortError::Fault));
    assert_eq!((writes.get(), calls.get()), before);
}

#[test]
fn test_should_retain_key_hold_and_release_for_retry_when_public_input_queue_is_full() {
    let mut i = InputState::new();
    assert_eq!(i.queue_capacity(), 4);
    for n in 0..4 {
        i.touch(Ok([2, 0, 0, 0]), n * 200_000).unwrap();
    }
    i.key(false, 700_000).unwrap();
    i.key(false, 730_000).unwrap();
    assert_eq!(i.key(false, 3_730_000), Err(PortError::Unavailable));
    assert_eq!(i.pop(), Some(Event::Up));
    i.key(false, 3_730_000).unwrap();
    for _ in 0..3 {
        assert_eq!(i.pop(), Some(Event::Up));
    }
    assert_eq!(i.pop(), Some(Event::KeyLongPress));
    assert_eq!(i.pop(), None);
    i.key(true, 3_740_000).unwrap();
    i.key(true, 3_770_000).unwrap();
    assert_eq!(i.pop(), None);
    // A normal release must also remain retryable after full, with no fabricated extra event.
    i.key(false, 4_000_000).unwrap();
    i.key(false, 4_030_000).unwrap();
    for n in 0..4 {
        i.touch(Ok([2, 0, 0, 0]), 4_100_000 + n * 200_000).unwrap();
    }
    i.key(true, 5_000_000).unwrap();
    assert_eq!(i.key(true, 5_030_000), Err(PortError::Unavailable));
    i.pop();
    i.key(true, 5_030_000).unwrap();
    for _ in 0..3 {
        i.pop();
    }
    assert_eq!(i.pop(), Some(Event::KeyRelease));
}

#[test]
fn test_should_reprobe_a_full_touch_queue_after_draining_without_another_irq() {
    let mut i = InputState::new();
    for n in 0..4 {
        i.touch(Ok([2, 0, 0, 0]), n * 200_000).unwrap();
    }
    assert_eq!(
        i.touch(Ok([3, 0, 0, 0]), 1_000_000),
        Err(PortError::Unavailable)
    );
    assert!(i.due(false, 1_010_000));
    i.pop();
    i.touch(Ok([3, 0, 0, 0]), 1_010_000).unwrap();
    for _ in 0..3 {
        i.pop();
    }
    assert_eq!(i.pop(), Some(Event::Left));
}
