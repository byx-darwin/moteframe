// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Nested panel wrappers expose ownership for explicit board recovery.
use embedded_hal::{
    delay::DelayNs,
    spi::{ErrorType, SpiBus},
};
use moteframe_core::{Dimensions, Display, OutputPin, spi::SharedBus};
use moteframe_display::{DcsDisplay, Rgb565, SharedPanelBus};
use std::{cell::Cell, rc::Rc};

#[derive(Debug)]
struct Bus {
    calls: Rc<Cell<usize>>,
    fail: bool,
}
impl ErrorType for Bus {
    type Error = embedded_hal::spi::ErrorKind;
}
impl SpiBus for Bus {
    fn write(&mut self, _: &[u8]) -> Result<(), Self::Error> {
        self.calls.set(self.calls.get() + 1);
        if self.fail {
            Err(Self::Error::Other)
        } else {
            Ok(())
        }
    }
    fn read(&mut self, _: &mut [u8]) -> Result<(), Self::Error> {
        panic!("unused")
    }
    fn transfer(&mut self, _: &mut [u8], _: &[u8]) -> Result<(), Self::Error> {
        panic!("unused")
    }
    fn transfer_in_place(&mut self, _: &mut [u8]) -> Result<(), Self::Error> {
        panic!("unused")
    }
    fn flush(&mut self) -> Result<(), Self::Error> {
        self.calls.set(self.calls.get() + 1);
        Ok(())
    }
}
#[derive(Debug)]
struct Delay(u8);
impl DelayNs for Delay {
    fn delay_ns(&mut self, _: u32) {}
}
#[derive(Debug)]
struct Pin {
    calls: Rc<Cell<usize>>,
    identity: u8,
}
impl OutputPin for Pin {
    type Error = ();
    fn set_level(&mut self, _: bool) -> Result<(), ()> {
        self.calls.set(self.calls.get() + 1);
        Ok(())
    }
}
#[test]
fn reclaim_healthy_and_failed_nested_panel_without_touching_hardware() {
    for fail in [false, true] {
        let calls = Rc::new(Cell::new(0));
        let shared = SharedBus::new(
            Bus {
                calls: calls.clone(),
                fail,
            },
            Delay(13),
        );
        let device = shared
            .try_device(Pin {
                calls: calls.clone(),
                identity: 3,
            })
            .unwrap_or_else(|e| panic!("{e:?}"));
        let transport = SharedPanelBus::new(
            device,
            Pin {
                calls: calls.clone(),
                identity: 7,
            },
        );
        let mut display = DcsDisplay::new(transport, Dimensions::new(1, 1), (0, 0));
        display
            .initialize(&[], &mut moteframe_core::hal::HalDelay::new(Delay(0)))
            .unwrap_or_else(|e| panic!("{e:?}"));
        let result = display.write(
            moteframe_core::Rect::new(0, 0, 1, 1),
            &[Rgb565::new(1, 2, 3)],
        );
        assert_eq!(result.is_err(), fail);
        let before = calls.get();
        let (device, dc) = display.into_bus().into_parts();
        let cs = device.into_select();
        let (bus, delay) = shared.into_parts();
        assert_eq!((cs.identity, dc.identity, delay.0), (3, 7, 13));
        assert_eq!(bus.fail, fail);
        assert_eq!(calls.get(), before);
    }
}
