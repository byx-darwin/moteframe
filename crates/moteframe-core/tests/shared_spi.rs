// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Shared transaction ownership and failure cleanup.
use embedded_hal::{
    delay::DelayNs,
    spi::{ErrorType, Operation, SpiBus, SpiDevice},
};
use moteframe_core::{
    OutputPin,
    spi::{Error, SharedBus},
};
use std::{cell::RefCell, rc::Rc};
type Log = Rc<RefCell<Vec<String>>>;
#[derive(Debug)]
struct Bus {
    log: Log,
    fail: bool,
}
impl ErrorType for Bus {
    type Error = embedded_hal::spi::ErrorKind;
}
impl SpiBus for Bus {
    fn read(&mut self, b: &mut [u8]) -> Result<(), Self::Error> {
        self.log.borrow_mut().push(format!("read{}", b.len()));
        b.fill(42);
        Ok(())
    }
    fn write(&mut self, b: &[u8]) -> Result<(), Self::Error> {
        self.log.borrow_mut().push(format!("write{b:?}"));
        if self.fail {
            Err(Self::Error::Other)
        } else {
            Ok(())
        }
    }
    fn transfer(&mut self, r: &mut [u8], w: &[u8]) -> Result<(), Self::Error> {
        self.write(w)?;
        self.read(r)
    }
    fn transfer_in_place(&mut self, b: &mut [u8]) -> Result<(), Self::Error> {
        self.log.borrow_mut().push("inplace".into());
        b.fill(21);
        Ok(())
    }
    fn flush(&mut self) -> Result<(), Self::Error> {
        self.log.borrow_mut().push("flush".into());
        Ok(())
    }
}
#[derive(Debug)]
struct Delay(Log);
impl DelayNs for Delay {
    fn delay_ns(&mut self, n: u32) {
        self.0.borrow_mut().push(format!("delay{n}"));
    }
}
#[derive(Debug)]
struct Pin {
    log: Log,
    id: u8,
    fail_high: bool,
}
impl OutputPin for Pin {
    type Error = bool;
    fn set_level(&mut self, h: bool) -> Result<(), bool> {
        self.log.borrow_mut().push(format!("cs{}={h}", self.id));
        if h && self.fail_high {
            Err(true)
        } else {
            Ok(())
        }
    }
}
fn setup(fail: bool) -> (SharedBus<Bus, Delay>, Log) {
    let l = Rc::default();
    (
        SharedBus::new(
            Bus {
                log: Rc::clone(&l),
                fail,
            },
            Delay(Rc::clone(&l)),
        ),
        l,
    )
}
#[test]
fn test_should_keep_one_select_through_operations_flush_then_release() {
    let (bus, l) = setup(false);
    let mut a = bus
        .device(Pin {
            log: l.clone(),
            id: 1,
            fail_high: false,
        })
        .unwrap_or_else(|e| panic!("{e:?}"));
    let mut b = bus
        .device(Pin {
            log: l.clone(),
            id: 2,
            fail_high: false,
        })
        .unwrap_or_else(|e| panic!("{e:?}"));
    l.borrow_mut().clear();
    let mut data = [0; 2];
    a.transaction(&mut [
        Operation::Write(&[0x9f]),
        Operation::Read(&mut data),
        Operation::DelayNs(5),
    ])
    .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(data, [42; 2]);
    assert_eq!(
        *l.borrow(),
        [
            "cs1=false",
            "write[159]",
            "read2",
            "flush",
            "delay5",
            "flush",
            "cs1=true"
        ]
    );
    a.transaction_with(|_, _| {
        assert!(matches!(b.write(&[1]), Err(Error::Busy)));
        Ok(())
    })
    .unwrap_or_else(|e| panic!("{e:?}"));
}
#[test]
fn test_should_release_select_after_bus_error_and_poison_all_devices() {
    let (bus, l) = setup(true);
    let mut a = bus
        .device(Pin {
            log: l.clone(),
            id: 1,
            fail_high: false,
        })
        .unwrap_or_else(|e| panic!("{e:?}"));
    let mut b = bus
        .device(Pin {
            log: l.clone(),
            id: 2,
            fail_high: false,
        })
        .unwrap_or_else(|e| panic!("{e:?}"));
    l.borrow_mut().clear();
    assert!(matches!(a.write(&[6]), Err(Error::Spi(_))));
    assert_eq!(*l.borrow(), ["cs1=false", "write[6]", "flush", "cs1=true"]);
    let count = l.borrow().len();
    assert!(matches!(b.write(&[1]), Err(Error::Faulted)));
    assert_eq!(l.borrow().len(), count);
}
