// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Standard HAL bridges preserve completion, ownership and errors.
use embedded_hal::{
    delay::DelayNs,
    digital::{ErrorType as PinErrorType, OutputPin as HalPin},
    spi::{ErrorType, SpiBus},
};
use moteframe_core::{
    DelayMs, OutputPin, SpiWrite,
    hal::{HalDelay, HalOutputPin, HalSpiBus},
};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fault {
    Write,
    Flush,
    Pin,
}
impl embedded_hal::spi::Error for Fault {
    fn kind(&self) -> embedded_hal::spi::ErrorKind {
        embedded_hal::spi::ErrorKind::Other
    }
}
impl embedded_hal::digital::Error for Fault {
    fn kind(&self) -> embedded_hal::digital::ErrorKind {
        embedded_hal::digital::ErrorKind::Other
    }
}
#[derive(Debug, PartialEq, Eq)]
enum Event {
    Write(Vec<u8>),
    Flush,
    Pin(bool),
    Milliseconds(u32),
    Nanoseconds(u32),
}
type Log = Rc<RefCell<Vec<Event>>>;

#[derive(Debug)]
struct Bus {
    log: Log,
    fail_write: bool,
    fail_flush: bool,
    identity: u8,
}
impl ErrorType for Bus {
    type Error = Fault;
}
impl SpiBus for Bus {
    fn read(&mut self, _: &mut [u8]) -> Result<(), Fault> {
        panic!("bridge must not read")
    }
    fn transfer(&mut self, _: &mut [u8], _: &[u8]) -> Result<(), Fault> {
        panic!("bridge must not transfer")
    }
    fn transfer_in_place(&mut self, _: &mut [u8]) -> Result<(), Fault> {
        panic!("bridge must not transfer in place")
    }
    fn write(&mut self, bytes: &[u8]) -> Result<(), Fault> {
        self.log.borrow_mut().push(Event::Write(bytes.to_vec()));
        if self.fail_write {
            Err(Fault::Write)
        } else {
            Ok(())
        }
    }
    fn flush(&mut self) -> Result<(), Fault> {
        self.log.borrow_mut().push(Event::Flush);
        if self.fail_flush {
            Err(Fault::Flush)
        } else {
            Ok(())
        }
    }
}

#[derive(Debug)]
struct Pin {
    log: Log,
    fail: bool,
    identity: u8,
}
impl PinErrorType for Pin {
    type Error = Fault;
}
impl HalPin for Pin {
    fn set_low(&mut self) -> Result<(), Fault> {
        self.log.borrow_mut().push(Event::Pin(false));
        if self.fail { Err(Fault::Pin) } else { Ok(()) }
    }
    fn set_high(&mut self) -> Result<(), Fault> {
        self.log.borrow_mut().push(Event::Pin(true));
        if self.fail { Err(Fault::Pin) } else { Ok(()) }
    }
}
#[derive(Debug)]
struct Delay(Log);
impl DelayNs for Delay {
    fn delay_ns(&mut self, nanoseconds: u32) {
        self.0.borrow_mut().push(Event::Nanoseconds(nanoseconds));
    }
    fn delay_ms(&mut self, milliseconds: u32) {
        self.0.borrow_mut().push(Event::Milliseconds(milliseconds));
    }
}

#[test]
fn spi_finishes_once_and_keeps_first_error() {
    for (fail_write, fail_flush, expected) in [
        (false, false, Ok(())),
        (true, false, Err(Fault::Write)),
        (false, true, Err(Fault::Flush)),
        (true, true, Err(Fault::Write)),
    ] {
        let log = Log::default();
        let mut bridge = HalSpiBus::new(Bus {
            log: Rc::clone(&log),
            fail_write,
            fail_flush,
            identity: 7,
        });
        assert_eq!(bridge.write(&[1, 2, 3]), expected);
        assert_eq!(*log.borrow(), [Event::Write(vec![1, 2, 3]), Event::Flush]);
        assert_eq!(bridge.into_inner().identity, 7);
    }
}

#[test]
fn borrowed_spi_is_returned_without_hidden_ownership() {
    let log = Log::default();
    let mut bus = Bus {
        log: Rc::clone(&log),
        fail_write: false,
        fail_flush: false,
        identity: 9,
    };
    let mut bridge = HalSpiBus::new(&mut bus);
    assert_eq!(bridge.write(&[]), Ok(()));
    bridge.into_inner().identity = 11;
    assert_eq!(bus.identity, 11);
    assert_eq!(*log.borrow(), [Event::Write(vec![]), Event::Flush]);
}

#[test]
fn pin_preserves_levels_errors_and_owned_identity() {
    let log = Log::default();
    let mut bridge = HalOutputPin::new(Pin {
        log: Rc::clone(&log),
        fail: false,
        identity: 3,
    });
    assert_eq!(bridge.set_level(false), Ok(()));
    assert_eq!(bridge.set_level(true), Ok(()));
    let mut pin = bridge.into_inner();
    assert_eq!(pin.identity, 3);
    pin.fail = true;
    let mut borrowed = HalOutputPin::new(&mut pin);
    assert_eq!(borrowed.set_level(true), Err(Fault::Pin));
    assert_eq!(borrowed.set_level(false), Err(Fault::Pin));
    borrowed.into_inner().identity = 4;
    assert_eq!(pin.identity, 4);
    assert_eq!(
        *log.borrow(),
        [
            Event::Pin(false),
            Event::Pin(true),
            Event::Pin(true),
            Event::Pin(false)
        ]
    );
}

#[test]
fn delay_forwards_zero_and_maximal_milliseconds_without_multiplication() {
    let log = Log::default();
    let mut bridge = HalDelay::new(Delay(Rc::clone(&log)));
    bridge.delay_ms(0);
    bridge.delay_ms(u16::MAX);
    let mut delay = bridge.into_inner();
    let mut borrowed = HalDelay::new(&mut delay);
    borrowed.delay_ms(17);
    let _: &mut Delay = borrowed.into_inner();
    assert_eq!(
        *log.borrow(),
        [
            Event::Milliseconds(0),
            Event::Milliseconds(65535),
            Event::Milliseconds(17)
        ]
    );
}
