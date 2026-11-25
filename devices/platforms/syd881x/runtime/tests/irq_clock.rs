//! Counter fixtures model the observable pending-bit/VAL read ordering.
#![allow(
    clippy::expect_used,
    reason = "Unexpected finite register fixture exhaustion"
)]
use core::num::NonZeroU32;
use std::collections::VecDeque;
use syd881x_runtime::irq_clock::{IrqClock, SnapshotRegisters};

struct Registers {
    pending: VecDeque<bool>,
    values: VecDeque<u32>,
}
impl Registers {
    fn new(pending: &[bool], values: &[u32]) -> Self {
        Self {
            pending: pending.iter().copied().collect(),
            values: values.iter().copied().collect(),
        }
    }
}
impl SnapshotRegisters for Registers {
    fn pending(&mut self) -> bool {
        self.pending.pop_front().expect("unexpected pending read")
    }
    fn current(&mut self) -> u32 {
        self.values.pop_front().expect("unexpected VAL read")
    }
}
fn clock(hz: u32) -> IrqClock {
    IrqClock::new(NonZeroU32::new(hz).expect("fixture frequency"))
}

#[test]
fn test_should_count_exact_wrap_even_when_val_is_unchanged() {
    let mut clock = clock(64_000_000);
    assert_eq!(clock.now(&mut Registers::new(&[false, false], &[0])), 0);
    assert_eq!(clock.now(&mut Registers::new(&[true, true], &[0])), 262_144);
    clock.on_interrupt();
    assert_eq!(
        clock.now(&mut Registers::new(&[false, false], &[0])),
        262_144
    );
    clock.on_interrupt();
    assert_eq!(
        clock.now(&mut Registers::new(&[false, false], &[0])),
        524_288
    );
}

#[test]
fn test_should_resample_val_when_wrap_arrives_during_snapshot() {
    let clock = clock(64_000_000);
    // The old VAL=1 predates wrap; the second VAL=0 belongs to the pending period.
    assert_eq!(
        clock.now(&mut Registers::new(&[false, true], &[1, 0])),
        262_144
    );
    assert_eq!(
        clock.now(&mut Registers::new(&[false, true], &[1, 16_777_152])),
        262_145
    );
}

#[test]
fn test_should_compensate_already_pending_wrap_without_consuming_it() {
    let mut clock = clock(64_000_000);
    assert_eq!(
        clock.now(&mut Registers::new(&[true, true], &[16_777_152])),
        262_145
    );
    assert_eq!(
        clock.now(&mut Registers::new(&[true, true], &[16_777_088])),
        262_146
    );
    clock.on_interrupt();
    assert_eq!(
        clock.now(&mut Registers::new(&[false, false], &[16_777_088])),
        262_146
    );
}

#[test]
fn test_should_keep_fractional_cycles_across_irq_boundaries() {
    let mut clock = clock(3_000_000);
    assert_eq!(
        clock.now(&mut Registers::new(&[false, false], &[16_777_215])),
        0
    );
    assert_eq!(
        clock.now(&mut Registers::new(&[false, false], &[16_777_213])),
        1
    );
    clock.on_interrupt();
    assert_eq!(
        clock.now(&mut Registers::new(&[false, false], &[16_777_214])),
        5_592_406
    );
}

#[test]
fn test_should_not_invent_a_wrap_when_val_read_precedes_interrupt() {
    let mut clock = clock(64_000_000);
    assert_eq!(
        clock.now(&mut Registers::new(&[false, false], &[64])),
        262_143
    );
    clock.on_interrupt();
    assert_eq!(
        clock.now(&mut Registers::new(&[false, false], &[0])),
        262_144
    );
}
