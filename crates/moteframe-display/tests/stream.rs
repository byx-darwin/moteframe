// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! DCS stream and transport failure contracts.
use moteframe_core::{Dimensions, Display, DisplayBus, Rect, Rgb565};
use moteframe_display::{DcsDisplay, InitStep};

#[derive(Debug, Default)]
struct Bus {
    commands: Vec<(u8, Vec<u8>)>,
    pixels: Vec<u8>,
    fail: bool,
    pixel_fail: bool,
}
impl DisplayBus for Bus {
    type Error = &'static str;
    fn command(&mut self, command: u8, data: &[u8]) -> Result<(), Self::Error> {
        if self.fail {
            return Err("bus");
        }
        self.commands.push((command, data.to_vec()));
        Ok(())
    }
    fn pixels(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        if self.fail || self.pixel_fail {
            return Err("bus");
        }
        assert!(data.len() <= 64);
        self.pixels.extend_from_slice(data);
        Ok(())
    }
}
#[derive(Debug, Default)]
struct Delay(Vec<u16>);
impl moteframe_core::DelayMs for Delay {
    fn delay_ms(&mut self, milliseconds: u16) {
        self.0.push(milliseconds);
    }
}

#[test]
fn test_should_send_inclusive_window_and_big_endian_pixels_in_chunks() {
    let mut display = DcsDisplay::new(Bus::default(), Dimensions::new(240, 240), (0, 0));
    let mut delay = Delay::default();
    assert!(
        display
            .initialize(
                &[InitStep::Command(0x3a, &[0x05]), InitStep::DelayMs(10)],
                &mut delay
            )
            .is_ok()
    );
    assert!(
        display
            .write(Rect::new(2, 3, 33, 1), &[Rgb565(0xf800); 33])
            .is_ok()
    );
    let bus = display.into_bus();
    assert_eq!(
        bus.commands,
        vec![
            (0x3a, vec![5]),
            (0x2a, vec![0, 2, 0, 34]),
            (0x2b, vec![0, 3, 0, 3]),
            (0x2c, vec![])
        ]
    );
    assert_eq!(bus.pixels, [0xf8, 0x00].repeat(33));
    assert_eq!(delay.0, vec![10]);
}

#[test]
fn test_should_reject_bad_rectangle_or_length_before_io() {
    let mut display = DcsDisplay::new(Bus::default(), Dimensions::new(240, 240), (0, 0));
    assert!(display.write(Rect::new(0, 0, 1, 1), &[Rgb565(0)]).is_err());
    assert!(display.initialize(&[], &mut Delay::default()).is_ok());
    for rect in [
        Rect::new(239, 0, 2, 1),
        Rect::new(u16::MAX, 0, 2, 1),
        Rect::new(0, 0, 0, 1),
    ] {
        assert!(display.write(rect, &[Rgb565(0)]).is_err());
    }
    assert!(display.write(Rect::new(0, 0, 2, 1), &[Rgb565(0)]).is_err());
    assert!(display.into_bus().commands.is_empty());
}

#[test]
fn test_should_propagate_bus_failure_and_require_reinitialization() {
    let mut display = DcsDisplay::new(
        Bus {
            fail: true,
            ..Bus::default()
        },
        Dimensions::new(240, 240),
        (0, 0),
    );
    assert!(
        display
            .initialize(&[InitStep::Command(0x11, &[])], &mut Delay::default())
            .is_err()
    );
    assert!(display.write(Rect::new(0, 0, 1, 1), &[Rgb565(0)]).is_err());
}

#[test]
fn test_should_check_physical_offset_overflow() {
    let mut display = DcsDisplay::new(Bus::default(), Dimensions::new(240, 240), (u16::MAX, 0));
    assert!(display.initialize(&[], &mut Delay::default()).is_ok());
    assert!(display.write(Rect::new(1, 0, 1, 1), &[Rgb565(0)]).is_err());
    assert!(display.into_bus().commands.is_empty());
}

#[test]
fn test_should_allow_last_physical_coordinate_without_intermediate_overflow() {
    let mut display = DcsDisplay::new(Bus::default(), Dimensions::new(1, 1), (u16::MAX, u16::MAX));
    assert!(display.initialize(&[], &mut Delay::default()).is_ok());
    assert!(display.write(Rect::new(0, 0, 1, 1), &[Rgb565(0)]).is_ok());
}

#[test]
fn test_should_invalidate_display_after_pixel_transfer_failure() {
    let mut display = DcsDisplay::new(
        Bus {
            pixel_fail: true,
            ..Bus::default()
        },
        Dimensions::new(1, 1),
        (0, 0),
    );
    assert!(display.initialize(&[], &mut Delay::default()).is_ok());
    assert_eq!(
        display.write(Rect::new(0, 0, 1, 1), &[Rgb565(0)]),
        Err(moteframe_display::Error::Bus("bus"))
    );
    assert_eq!(
        display.write(Rect::new(0, 0, 1, 1), &[Rgb565(0)]),
        Err(moteframe_display::Error::NotInitialized)
    );
    assert_eq!(display.into_bus().commands.len(), 3);
}

#[test]
fn test_should_reuse_unchanged_axes_but_restart_ram_for_every_write() {
    let mut d = DcsDisplay::new(Bus::default(), Dimensions::new(8, 8), (2, 1));
    assert!(d.initialize(&[], &mut Delay::default()).is_ok());
    for (r, p) in [
        (Rect::new(0, 0, 2, 1), vec![Rgb565(0x1234); 2]),
        (Rect::new(0, 1, 2, 1), vec![Rgb565(0x5678); 2]),
        (Rect::new(1, 1, 1, 1), vec![Rgb565(0x9abc)]),
        (Rect::new(1, 1, 1, 1), vec![Rgb565(0xdef0)]),
    ] {
        assert!(d.write(r, &p).is_ok());
    }
    let b = d.into_bus();
    assert_eq!(
        b.commands,
        vec![
            (0x2a, vec![0, 2, 0, 3]),
            (0x2b, vec![0, 1, 0, 1]),
            (0x2c, vec![]),
            (0x2b, vec![0, 2, 0, 2]),
            (0x2c, vec![]),
            (0x2a, vec![0, 3, 0, 3]),
            (0x2c, vec![]),
            (0x2c, vec![])
        ]
    );
    assert_eq!(
        b.pixels,
        vec![
            0x12, 0x34, 0x12, 0x34, 0x56, 0x78, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0
        ]
    );
}
#[derive(Clone, Debug, Default)]
struct SharedBus(std::rc::Rc<std::cell::RefCell<Bus>>);
impl DisplayBus for SharedBus {
    type Error = &'static str;
    fn command(&mut self, c: u8, d: &[u8]) -> Result<(), Self::Error> {
        self.0.borrow_mut().command(c, d)
    }
    fn pixels(&mut self, d: &[u8]) -> Result<(), Self::Error> {
        self.0.borrow_mut().pixels(d)
    }
}
#[test]
fn test_should_forget_cached_axes_after_reinitialization_and_pixel_failure() {
    let bus = SharedBus::default();
    let state = bus.0.clone();
    let mut d = DcsDisplay::new(bus, Dimensions::new(8, 8), (0, 0));
    assert!(d.initialize(&[], &mut Delay::default()).is_ok());
    let r = Rect::new(1, 1, 1, 1);
    assert!(d.write(r, &[Rgb565(1)]).is_ok());
    state.borrow_mut().pixel_fail = true;
    assert_eq!(
        d.write(r, &[Rgb565(2)]),
        Err(moteframe_display::Error::Bus("bus"))
    );
    let n = state.borrow().commands.len();
    assert_eq!(
        d.write(r, &[Rgb565(2)]),
        Err(moteframe_display::Error::NotInitialized)
    );
    assert_eq!(state.borrow().commands.len(), n);
    state.borrow_mut().pixel_fail = false;
    assert!(d.initialize(&[], &mut Delay::default()).is_ok());
    assert!(d.write(r, &[Rgb565(3)]).is_ok());
    assert!(d.initialize(&[], &mut Delay::default()).is_ok());
    assert!(d.write(r, &[Rgb565(4)]).is_ok());
    let cmds: Vec<_> = state.borrow().commands.iter().map(|(c, _)| *c).collect();
    assert_eq!(
        cmds,
        vec![0x2a, 0x2b, 0x2c, 0x2c, 0x2a, 0x2b, 0x2c, 0x2a, 0x2b, 0x2c]
    );
}
