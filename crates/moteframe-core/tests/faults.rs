// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Select, operation, flush and release error precedence.
use embedded_hal::{
    delay::DelayNs,
    spi::{ErrorKind, ErrorType, SpiBus},
};
use moteframe_core::{
    OutputPin,
    spi::{Error, SharedBus},
};
use std::{cell::RefCell, rc::Rc};
type Log = Rc<RefCell<Vec<&'static str>>>;
#[derive(Debug)]
struct Bus {
    log: Log,
    fail: bool,
}
impl ErrorType for Bus {
    type Error = ErrorKind;
}
impl SpiBus for Bus {
    fn read(&mut self, _: &mut [u8]) -> Result<(), ErrorKind> {
        panic!("unused")
    }
    fn write(&mut self, _: &[u8]) -> Result<(), ErrorKind> {
        panic!("unused")
    }
    fn transfer(&mut self, _: &mut [u8], _: &[u8]) -> Result<(), ErrorKind> {
        panic!("unused")
    }
    fn transfer_in_place(&mut self, _: &mut [u8]) -> Result<(), ErrorKind> {
        panic!("unused")
    }
    fn flush(&mut self) -> Result<(), ErrorKind> {
        self.log.borrow_mut().push("flush");
        if self.fail {
            Err(ErrorKind::Overrun)
        } else {
            Ok(())
        }
    }
}
#[derive(Debug)]
struct Delay;
impl DelayNs for Delay {
    fn delay_ns(&mut self, _: u32) {}
}
#[derive(Debug)]
struct Pin {
    log: Log,
    select: bool,
    release: bool,
    first: bool,
}
impl OutputPin for Pin {
    type Error = u8;
    fn set_level(&mut self, high: bool) -> Result<(), u8> {
        self.log
            .borrow_mut()
            .push(if high { "high" } else { "low" });
        if self.first {
            self.first = false;
            return Ok(());
        }
        if !high && self.select {
            Err(1)
        } else if high && self.release {
            Err(2)
        } else {
            Ok(())
        }
    }
}
#[test]
fn selected_operation_flush_release_fault_matrix() {
    for selected in [false, true] {
        for operation in [false, true] {
            for flush in [false, true] {
                for release in [false, true] {
                    let log = Log::default();
                    let shared = SharedBus::new(
                        Bus {
                            log: log.clone(),
                            fail: flush,
                        },
                        Delay,
                    );
                    let mut dev = shared
                        .try_device(Pin {
                            log: log.clone(),
                            select: selected,
                            release,
                            first: true,
                        })
                        .unwrap_or_else(|error| panic!("{error:?}"));
                    log.borrow_mut().clear();
                    let result = dev.transaction_with(|_, _| {
                        log.borrow_mut().push("op");
                        if operation {
                            Err(Error::Spi(ErrorKind::ModeFault))
                        } else {
                            Ok(())
                        }
                    });
                    let expected = if selected {
                        Err(Error::Pin(1))
                    } else if operation {
                        Err(Error::Spi(ErrorKind::ModeFault))
                    } else if flush {
                        Err(Error::Spi(ErrorKind::Overrun))
                    } else if release {
                        Err(Error::Pin(2))
                    } else {
                        Ok(())
                    };
                    assert_eq!(
                        result, expected,
                        "selected={selected} op={operation} flush={flush} release={release}"
                    );
                    assert_eq!(
                        *log.borrow(),
                        if selected {
                            vec!["low", "flush", "high"]
                        } else {
                            vec!["low", "op", "flush", "high"]
                        }
                    );
                    if expected.is_err() {
                        log.borrow_mut().clear();
                        assert_eq!(
                            dev.transaction_with(|_, _| panic!("faulted operation ran")),
                            Err::<(), _>(Error::Faulted)
                        );
                        assert!(log.borrow().is_empty());
                    }
                }
            }
        }
    }
    println!(
        "16 select/op/flush/release fault combinations passed; first-error, unconditional flush+release and no-I/O fault latch"
    );
}
