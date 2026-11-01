// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! GPIO/SPI boundary cleanup, without a simulated BLE controller.
use moteframe_core::{DisplayBus, OutputPin, SpiWrite};
use moteframe_display::{SpiError, SpiPanelBus};
use std::{cell::RefCell, rc::Rc};

#[derive(Debug)]
struct Pin(Rc<RefCell<Vec<bool>>>);
impl OutputPin for Pin {
    type Error = ();
    fn set_level(&mut self, high: bool) -> Result<(), Self::Error> {
        self.0.borrow_mut().push(high);
        Ok(())
    }
}
#[derive(Debug)]
struct Spi {
    fail: bool,
    bytes: Vec<u8>,
}
impl SpiWrite for Spi {
    type Error = &'static str;
    fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        if self.fail {
            return Err("spi");
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }
}
#[test]
fn test_should_release_chip_select_when_spi_fails() {
    let cs = Rc::new(RefCell::new(Vec::new()));
    let dc = Rc::new(RefCell::new(Vec::new()));
    let mut bus = SpiPanelBus::new(
        Spi {
            fail: true,
            bytes: vec![],
        },
        Pin(cs.clone()),
        Pin(dc),
    );
    assert!(matches!(bus.command(0x11, &[]), Err(SpiError::Spi("spi"))));
    assert_eq!(*cs.borrow(), vec![false, true]);
}
#[test]
fn test_should_send_command_then_parameters_and_data_with_correct_dc() {
    let cs = Rc::new(RefCell::new(Vec::new()));
    let dc = Rc::new(RefCell::new(Vec::new()));
    let mut bus = SpiPanelBus::new(
        Spi {
            fail: false,
            bytes: vec![],
        },
        Pin(cs.clone()),
        Pin(dc.clone()),
    );
    assert!(bus.command(0x3a, &[5]).is_ok());
    assert!(bus.pixels(&[0xf8, 0]).is_ok());
    let (spi, _, _) = bus.into_parts();
    assert_eq!(spi.bytes, vec![0x3a, 5, 0xf8, 0]);
    assert_eq!(*cs.borrow(), vec![false, true, false, true]);
    assert_eq!(*dc.borrow(), vec![false, true, true]);
}
