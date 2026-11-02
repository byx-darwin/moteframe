// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Recover ownership without silently retrying or repairing hardware.
use embedded_hal::{
    delay::DelayNs,
    spi::{ErrorType, SpiBus, SpiDevice},
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
    identity: u8,
}
impl ErrorType for Bus {
    type Error = embedded_hal::spi::ErrorKind;
}
impl SpiBus for Bus {
    fn write(&mut self, _: &[u8]) -> Result<(), Self::Error> {
        self.log.borrow_mut().push("write");
        if self.fail {
            Err(Self::Error::Other)
        } else {
            Ok(())
        }
    }
    fn read(&mut self, _: &mut [u8]) -> Result<(), Self::Error> {
        panic!("not used")
    }
    fn transfer(&mut self, _: &mut [u8], _: &[u8]) -> Result<(), Self::Error> {
        panic!("not used")
    }
    fn transfer_in_place(&mut self, _: &mut [u8]) -> Result<(), Self::Error> {
        panic!("not used")
    }
    fn flush(&mut self) -> Result<(), Self::Error> {
        self.log.borrow_mut().push("flush");
        Ok(())
    }
}
#[derive(Debug)]
struct Delay {
    log: Log,
    identity: u8,
}
impl DelayNs for Delay {
    fn delay_ns(&mut self, _: u32) {
        self.log.borrow_mut().push("delay");
    }
}
#[derive(Debug)]
struct Pin {
    log: Log,
    fail: bool,
    identity: u8,
    payload: [u8; 40],
}
impl OutputPin for Pin {
    type Error = u8;
    fn set_level(&mut self, high: bool) -> Result<(), u8> {
        self.log
            .borrow_mut()
            .push(if high { "high" } else { "low" });
        if self.fail { Err(17) } else { Ok(()) }
    }
}
fn owner(log: &Log, fail: bool) -> SharedBus<Bus, Delay> {
    SharedBus::new(
        Bus {
            log: log.clone(),
            fail,
            identity: 3,
        },
        Delay {
            log: log.clone(),
            identity: 5,
        },
    )
}
fn pin(log: &Log, fail: bool) -> Pin {
    Pin {
        log: log.clone(),
        fail,
        identity: 7,
        payload: [29; 40],
    }
}
fn assert_pin(pin: &Pin) {
    assert_eq!(pin.identity, 7);
    assert_eq!(pin.payload, [29; 40]);
}

fn failure<T, E>(result: Result<T, E>) -> E {
    match result {
        Err(error) => error,
        Ok(_) => panic!("expected a creation error"),
    }
}

#[test]
fn successful_reclamation_preserves_resources_without_io() {
    let log = Log::default();
    let shared = owner(&log, false);
    let first = shared
        .try_device(pin(&log, false))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let second = shared
        .device(pin(&log, false))
        .unwrap_or_else(|e| panic!("{e:?}"));
    log.borrow_mut().clear();
    assert_pin(&first.into_select());
    assert_pin(&second.into_select());
    let (bus, delay) = shared.into_parts();
    assert_eq!((bus.identity, delay.identity), (3, 5));
    assert!(log.borrow().is_empty());
}

#[test]
fn failed_initial_select_returns_original_pin_and_latches_fault() {
    let log = Log::default();
    let shared = owner(&log, false);
    let failed = failure(shared.try_device(pin(&log, true)));
    assert_eq!(failed.error(), &Error::Pin(17));
    assert_eq!(*log.borrow(), ["high"]);
    let (error, original) = failed.into_parts();
    assert_eq!(error, Error::Pin(17));
    assert_pin(&original);
    let failed = failure(shared.try_device(pin(&log, false)));
    assert_eq!(failed.error(), &Error::Faulted);
    let (_, original) = failed.into_parts();
    assert_pin(&original);
    assert_eq!(*log.borrow(), ["high"]);
    let (bus, delay) = shared.into_parts();
    assert_eq!((bus.identity, delay.identity), (3, 5));
    assert_eq!(*log.borrow(), ["high"]);
}

#[test]
fn busy_creation_returns_pin_without_io_or_poison() {
    let log = Log::default();
    let shared = owner(&log, false);
    let mut first = shared
        .device(pin(&log, false))
        .unwrap_or_else(|e| panic!("{e:?}"));
    log.borrow_mut().clear();
    first
        .transaction_with(|_, _| {
            let before = log.borrow().len();
            let failed = failure(shared.try_device(pin(&log, false)));
            assert_eq!(failed.error(), &Error::Busy);
            let (_, original) = failed.into_parts();
            assert_pin(&original);
            assert_eq!(log.borrow().len(), before);
            Ok(())
        })
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(first.write(&[1]), Ok(()));
}

#[test]
fn transfer_failure_retains_fault_until_explicit_hardware_rebuild() {
    let log = Log::default();
    let shared = owner(&log, true);
    let mut device = shared
        .try_device(pin(&log, false))
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(matches!(device.write(&[1]), Err(Error::Spi(_))));
    log.borrow_mut().clear();
    assert_eq!(device.write(&[1]), Err(Error::Faulted));
    let failed = failure(shared.try_device(pin(&log, false)));
    assert_eq!(failed.error(), &Error::Faulted);
    assert_pin(&failed.into_parts().1);
    let select = device.into_select();
    let (mut bus, delay) = shared.into_parts();
    assert!(log.borrow().is_empty());
    // The fixture represents explicit external hardware repair, not a core retry.
    bus.fail = false;
    let rebuilt = SharedBus::new(bus, delay);
    let mut device = rebuilt
        .try_device(select)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(device.write(&[1]), Ok(()));
    assert_eq!(*log.borrow(), ["high", "low", "write", "flush", "high"]);
}
