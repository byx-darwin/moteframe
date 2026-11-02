// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Additive address and resource lifecycle guarantees, including non-Debug GPIO.
use moteframe_i2c::{BitBangI2c, I2cError, Line, NormalAddress7};
use std::{cell::Cell, rc::Rc};

struct Pin {
    identity: u8,
    calls: Rc<Cell<u32>>,
}
impl Line for Pin {
    fn low(&mut self) {
        self.calls.set(self.calls.get() + 1);
    }
    fn release(&mut self) {
        self.calls.set(self.calls.get() + 1);
    }
    fn high(&self) -> bool {
        self.calls.set(self.calls.get() + 1);
        true
    }
}
impl Line for &mut Pin {
    fn low(&mut self) {
        (**self).low();
    }
    fn release(&mut self) {
        (**self).release();
    }
    fn high(&self) -> bool {
        (**self).high()
    }
}
fn pin(identity: u8, calls: &Rc<Cell<u32>>) -> Pin {
    Pin {
        identity,
        calls: calls.clone(),
    }
}

#[test]
fn ordinary_address_bounds_exclude_special_uses() {
    for address in 0..=255 {
        match NormalAddress7::new(address) {
            Ok(normal) => {
                assert!((0x08..=0x77).contains(&address));
                assert_eq!(normal.get(), address);
            }
            Err(error) => {
                assert!(!(0x08..=0x77).contains(&address));
                assert_eq!(error, I2cError::Invalid);
            }
        }
    }
}

#[test]
fn zero_budget_retains_owned_pins_and_captured_delay_without_io() {
    let calls = Rc::new(Cell::new(0));
    let pauses = Cell::new(0);
    let marker = Rc::new(Cell::new(7));
    let captured = marker.clone();
    let mut count = 0;
    let delay = || {
        count += 1;
        pauses.set(count);
        captured.set(9);
    };
    let Err(failure) = BitBangI2c::try_new(pin(1, &calls), pin(2, &calls), delay, 0) else {
        panic!("zero budget accepted")
    };
    // Neither GPIO nor the capturing closure implements Debug.
    assert!(!format!("{failure:?}").is_empty());
    assert_eq!(failure.error(), &I2cError::Invalid);
    let (reason, clock, data, mut delay) = failure.into_parts();
    assert_eq!(reason, I2cError::Invalid);
    assert_eq!((clock.identity, data.identity), (1, 2));
    assert_eq!(calls.get(), 0);
    assert_eq!(pauses.get(), 0);
    delay();
    assert_eq!(pauses.get(), 1);
    assert_eq!(marker.get(), 9);
}

#[test]
fn borrowed_resources_can_be_rebuilt_after_failed_construction() {
    let calls = Rc::new(Cell::new(0));
    let mut clock = pin(3, &calls);
    let mut data = pin(4, &calls);
    let Err(failure) = BitBangI2c::try_new(&mut clock, &mut data, || {}, 0) else {
        panic!("zero budget accepted")
    };
    let (_, clock, data, delay) = failure.into_parts();
    assert_eq!(calls.get(), 0);
    let bus =
        BitBangI2c::try_new(clock, data, delay, 2).unwrap_or_else(|error| panic!("{error:?}"));
    assert_eq!(calls.get(), 2);
    let (clock, data, _) = bus.into_parts();
    assert_eq!((clock.identity, data.identity), (3, 4));
    assert_eq!(calls.get(), 2, "taking ownership back must not perform I/O");
}

#[test]
fn ordinary_read_preserves_raw_completion_and_empty_read_is_zero_io() {
    let calls = Rc::new(Cell::new(0));
    let mut bus = BitBangI2c::new(pin(5, &calls), pin(6, &calls), || {}, 2)
        .unwrap_or_else(|error| panic!("{error:?}"));
    let normal = NormalAddress7::new(0x15).unwrap_or_else(|error| panic!("{error:?}"));
    calls.set(0);
    assert_eq!(bus.read_normal(normal, 1, &mut []), Err(I2cError::Invalid));
    assert_eq!(calls.get(), 0);
    assert_eq!(bus.read_normal(normal, 1, &mut [0]), Err(I2cError::Nack));
    let normal_calls = calls.get();
    calls.set(0);
    assert_eq!(bus.read(0x15, 1, &mut [0]), Err(I2cError::Nack));
    assert_eq!(calls.get(), normal_calls);
    calls.set(0);
    assert_eq!(bus.read(0, 6, &mut [0]), Err(I2cError::Nack));
    assert!(
        calls.get() > 0,
        "legacy special addresses remain explicit raw I/O"
    );
}
