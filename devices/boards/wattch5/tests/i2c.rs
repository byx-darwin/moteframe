//! Normal pointer-read I2C protocol, including fragmented slave ACK/data.
use board_syd8810::drivers::i2c::{BitBangI2c, Line};
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
