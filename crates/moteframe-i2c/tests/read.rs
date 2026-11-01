// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Normal pointer-read I2C protocol, including fragmented slave ACK/data.
use moteframe_i2c::{BitBangI2c, I2cError, Line};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};
#[derive(Debug)]
struct Pin {
    clock: bool,
    level: Rc<Cell<bool>>,
    data_level: Rc<Cell<bool>>,
    samples: Rc<RefCell<VecDeque<bool>>>,
    rises: Rc<RefCell<Vec<bool>>>,
    stuck: bool,
}
impl Line for Pin {
    fn low(&mut self) {
        self.level.set(false);
    }
    fn release(&mut self) {
        if self.clock && !self.level.get() {
            self.rises.borrow_mut().push(self.data_level.get());
        }
        self.level.set(true);
    }
    fn high(&self) -> bool {
        if self.clock {
            !self.stuck
        } else {
            self.samples.borrow_mut().pop_front().unwrap_or(true)
        }
    }
}
type Fixture = (Pin, Pin, Rc<RefCell<Vec<bool>>>, Rc<Cell<bool>>);
fn pins(stuck: bool, samples: Vec<bool>) -> Fixture {
    let rises = Rc::new(RefCell::new(Vec::new()));
    let data = Rc::new(Cell::new(true));
    let samples = Rc::new(RefCell::new(samples.into()));
    (
        Pin {
            clock: true,
            level: Rc::new(Cell::new(true)),
            data_level: data.clone(),
            samples: samples.clone(),
            rises: rises.clone(),
            stuck,
        },
        Pin {
            clock: false,
            level: data.clone(),
            data_level: data.clone(),
            samples,
            rises: rises.clone(),
            stuck: false,
        },
        rises,
        data,
    )
}
#[test]
fn test_should_send_stop_separated_normal_address_and_nack_final_byte() {
    let (c, d, rises, _) = pins(
        false,
        vec![
            true, false, false, true, false, false, false, false, false, false, true, false, true,
        ],
    );
    let mut bus = BitBangI2c::new(c, d, || {}, 3).unwrap_or_else(|e| panic!("{e:?}"));
    let mut out = [0];
    assert!(bus.read(0x15, 0x01, &mut out).is_ok());
    assert_eq!(out, [5]);
    let edges = rises.borrow();
    assert_eq!(
        &edges[..8],
        &[false, false, true, false, true, false, true, false]
    );
    assert_eq!(
        &edges[9..17],
        &[false, false, false, false, false, false, false, true]
    );
    assert_eq!(
        &edges[19..27],
        &[false, false, true, false, true, false, true, true]
    );
    assert!(edges[36], "final byte must be NACKed");
}
#[test]
fn test_should_return_nack_or_clock_timeout_with_released_lines() {
    for (stuck, samples) in [(false, vec![true, true]), (true, vec![])] {
        let (c, d, _, released) = pins(stuck, samples);
        let mut bus = BitBangI2c::new(c, d, || {}, 3).unwrap_or_else(|e| panic!("{e:?}"));
        assert!(bus.read(0x15, 1, &mut [0]).is_err());
        assert!(released.get());
    }
}

#[test]
fn invalid_requests_do_not_touch_lines_or_delay() {
    let (c, d, rises, _) = pins(false, vec![]);
    assert!(matches!(
        BitBangI2c::new(c, d, || {}, 0),
        Err(I2cError::Invalid)
    ));
    assert!(rises.borrow().is_empty());
    let (c, d, rises, _) = pins(false, vec![]);
    let pauses = Cell::new(0);
    let mut bus = BitBangI2c::new(c, d, || pauses.set(pauses.get() + 1), 3)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(bus.read(128, 1, &mut [0]), Err(I2cError::Invalid));
    assert_eq!(bus.read(0x15, 1, &mut []), Err(I2cError::Invalid));
    assert!(rises.borrow().is_empty());
    assert_eq!(pauses.get(), 0);
}

#[test]
fn stuck_clock_has_bounded_polling_and_releases_data() {
    let (c, d, _, released) = pins(true, vec![]);
    let pauses = Cell::new(0);
    let mut bus = BitBangI2c::new(c, d, || pauses.set(pauses.get() + 1), 3)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(bus.read(0x15, 1, &mut [0]), Err(I2cError::ClockTimeout));
    assert_eq!(
        pauses.get(),
        9,
        "two bounded clock waits plus three STOP pauses"
    );
    assert!(released.get());
}

#[derive(Debug)]
struct ClockScript {
    samples: RefCell<VecDeque<bool>>,
    released: Rc<Cell<bool>>,
}
impl Line for ClockScript {
    fn low(&mut self) {
        self.released.set(false);
    }
    fn release(&mut self) {
        self.released.set(true);
    }
    fn high(&self) -> bool {
        self.samples.borrow_mut().pop_front().unwrap_or(false)
    }
}
#[test]
fn original_read_error_wins_when_stop_cleanup_also_times_out() {
    let (_, d, _, data_released) = pins(false, vec![true, true]);
    let clock_released = Rc::new(Cell::new(true));
    let c = ClockScript {
        samples: RefCell::new([true; 10].into()),
        released: clock_released.clone(),
    };
    let mut bus = BitBangI2c::new(c, d, || {}, 1).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(bus.read(0x15, 1, &mut [0]), Err(I2cError::Nack));
    assert!(clock_released.get());
    assert!(data_released.get());
}
#[test]
fn successful_read_still_reports_a_failed_stop_cleanup() {
    let (_, d, _, data_released) = pins(
        false,
        vec![
            true, false, false, true, false, true, false, true, false, true, false, true, false,
        ],
    );
    let clock_released = Rc::new(Cell::new(true));
    let c = ClockScript {
        samples: RefCell::new([true; 39].into()),
        released: clock_released.clone(),
    };
    let mut bus = BitBangI2c::new(c, d, || {}, 1).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(bus.read(0x15, 1, &mut [0]), Err(I2cError::ClockTimeout));
    assert!(clock_released.get());
    assert!(data_released.get());
}
