// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! A panel command changes DC while one shared slave select remains asserted.
use embedded_hal::{
    delay::DelayNs,
    spi::{ErrorType, SpiBus},
};
use moteframe_core::{DisplayBus, OutputPin, spi::SharedBus};
use moteframe_display::SharedPanelBus;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
type Log = Rc<RefCell<Vec<String>>>;
#[derive(Debug)]
struct Bus(Log, Rc<Cell<bool>>);
impl ErrorType for Bus {
    type Error = embedded_hal::spi::ErrorKind;
}
impl SpiBus for Bus {
    fn write(&mut self, b: &[u8]) -> Result<(), Self::Error> {
        self.0.borrow_mut().push(format!("bytes{b:?}"));
        self.1.set(true);
        Ok(())
    }
    fn read(&mut self, _: &mut [u8]) -> Result<(), Self::Error> {
        Err(Self::Error::Other)
    }
    fn transfer(&mut self, _: &mut [u8], _: &[u8]) -> Result<(), Self::Error> {
        Err(Self::Error::Other)
    }
    fn transfer_in_place(&mut self, _: &mut [u8]) -> Result<(), Self::Error> {
        Err(Self::Error::Other)
    }
    fn flush(&mut self) -> Result<(), Self::Error> {
        self.0.borrow_mut().push("flush".into());
        self.1.set(false);
        Ok(())
    }
}
#[derive(Debug)]
struct Delay;
impl DelayNs for Delay {
    fn delay_ns(&mut self, _: u32) {}
}
#[derive(Debug)]
struct Pin(Log, &'static str, Rc<Cell<bool>>);
impl OutputPin for Pin {
    type Error = ();
    fn set_level(&mut self, h: bool) -> Result<(), ()> {
        assert!(!self.2.get(), "DC/CS changed while write still pending");
        self.0.borrow_mut().push(format!("{}={h}", self.1));
        Ok(())
    }
}
#[test]
fn test_should_keep_command_and_parameters_in_one_shared_transaction() {
    let log = Rc::default();
    let pending = Rc::new(Cell::new(false));
    let shared = SharedBus::new(Bus(Rc::clone(&log), pending.clone()), Delay);
    let dev = shared
        .device(Pin(log.clone(), "cs", pending.clone()))
        .unwrap_or_else(|e| panic!("{e:?}"));
    let mut display = SharedPanelBus::new(dev, Pin(log.clone(), "dc", pending.clone()));
    log.borrow_mut().clear();
    display
        .command(0x3a, &[5])
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        *log.borrow(),
        [
            "cs=false",
            "dc=false",
            "bytes[58]",
            "flush",
            "dc=true",
            "bytes[5]",
            "flush",
            "cs=true"
        ]
    );
}
