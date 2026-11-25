//! Exercise real public display/source contracts through the private compatibility ports.
#![allow(clippy::unwrap_used)]
use device_core::{Dimensions, Display, Rect, Rgb565};
use device_moteframe::{DisplayRef, NativeDisplay, Pin, ReadRef, initialization};
use embedded_graphics::pixelcolor::{Rgb565 as Native, raw::RawU16};
use moteframe_core::{Display as MoteDisplay, OutputPin, ReadAt};
use moteframe_display::{DcsDisplay, InitStep};
#[derive(Debug)]
struct Bus {
    commands: std::vec::Vec<u8>,
    pixels: std::vec::Vec<u8>,
    fail: bool,
}
impl moteframe_core::DisplayBus for Bus {
    type Error = u8;
    fn command(&mut self, c: u8, _: &[u8]) -> Result<(), u8> {
        self.commands.push(c);
        Ok(())
    }
    fn pixels(&mut self, d: &[u8]) -> Result<(), u8> {
        if self.fail {
            return Err(9);
        }
        self.pixels.extend_from_slice(d);
        Ok(())
    }
}
struct Delay;
impl moteframe_core::DelayMs for Delay {
    fn delay_ms(&mut self, _: u16) {}
}
#[test]
fn test_should_forward_native_ui_pixels_directly_without_splitting_a_valid_window() {
    let mut compat = NativeDisplay::new(DcsDisplay::new(
        Bus {
            commands: vec![],
            pixels: vec![],
            fail: false,
        },
        moteframe_core::Dimensions::new(128, 128),
        (2, 1),
    ));
    compat.inner_mut().initialize(&[], &mut Delay).unwrap();
    let colors = [Native::from(RawU16::new(0xf800)); 40];
    MoteDisplay::write(
        &mut DisplayRef::new(&mut compat),
        moteframe_core::Rect::new(0, 0, 40, 1),
        &colors,
    )
    .unwrap();
    let bus = compat.into_inner().into_bus();
    assert_eq!(bus.commands, vec![0x2a, 0x2b, 0x2c]);
    assert_eq!(bus.pixels, [0xf8, 0].repeat(40));
}
#[test]
fn test_should_preserve_legacy_raw_pixels_and_stop_on_actual_native_error() {
    let mut d = DcsDisplay::new(
        Bus {
            commands: vec![],
            pixels: vec![],
            fail: true,
        },
        moteframe_core::Dimensions::new(128, 128),
        (0, 0),
    );
    d.initialize(&[], &mut Delay).unwrap();
    let mut compat = NativeDisplay::new(d);
    assert!(
        compat
            .write(Rect::new(0, 0, 1, 1), &[Rgb565(0xf800)])
            .is_err()
    );
    assert!(
        compat
            .write(Rect::new(127, 0, 2, 1), &[Rgb565(0); 2])
            .is_err()
    );
}
#[derive(Debug)]
struct Old {
    calls: usize,
    values: std::vec::Vec<u16>,
}
impl Display for Old {
    type Error = u8;
    fn dimensions(&self) -> Dimensions {
        Dimensions::new(128, 128)
    }
    fn write(&mut self, _: Rect, v: &[Rgb565]) -> Result<(), u8> {
        self.calls += 1;
        self.values.extend(v.iter().map(|p| p.0));
        Ok(())
    }
}
#[test]
fn test_should_allow_legacy_display_implementations_to_consume_public_ui_colors() {
    let mut old = Old {
        calls: 0,
        values: vec![],
    };
    let mut adapter = DisplayRef::new(&mut old);
    let colors = [Native::from(RawU16::new(0x07e0)); 20];
    MoteDisplay::write(
        &mut adapter,
        moteframe_core::Rect::new(0, 0, 10, 2),
        &colors,
    )
    .unwrap();
    assert_eq!(old.values, vec![0x07e0; 20]);
    assert_eq!(old.calls, 4);
    let mut adapter = DisplayRef::new(&mut old);
    assert!(
        MoteDisplay::write(
            &mut adapter,
            moteframe_core::Rect::new(0, 0, 10, 2),
            &colors[..19]
        )
        .is_err()
    );
    assert_eq!(old.calls, 4);
}
#[test]
fn test_should_bound_resource_views_and_preserve_original_source_failure() {
    #[derive(Debug)]
    struct Source;
    impl device_core::ReadAt for Source {
        type Error = u8;
        fn capacity(&self) -> u32 {
            8
        }
        fn read_exact(&mut self, o: u32, out: &mut [u8]) -> Result<(), u8> {
            if o == 3 {
                return Err(7);
            }
            out.fill(u8::try_from(o).map_err(|_| 8_u8)?);
            Ok(())
        }
    }
    let mut s = ReadRef::new(Source);
    let mut view =
        moteframe_storage::ReadWindow::new(moteframe_storage::ReadRef::new(&mut s), 2, 4).unwrap();
    let mut out = [0; 2];
    view.read_exact(0, &mut out).unwrap();
    assert_eq!(out, [2; 2]);
    assert!(view.read_exact(1, &mut out).is_err());
    assert!(view.read_exact(3, &mut out).is_err());
}
#[test]
fn test_should_forward_pin_error_and_exact_panel_steps() {
    #[derive(Debug)]
    struct P;
    impl device_core::OutputPin for P {
        type Error = u8;
        fn set_level(&mut self, v: bool) -> Result<(), u8> {
            if v { Ok(()) } else { Err(8) }
        }
    }
    const STEPS: [InitStep<'static>; 2] = initialization([
        device_display::InitStep::Command(0x36, &[0xd8]),
        device_display::InitStep::DelayMs(120),
    ]);
    let mut p = Pin(P);
    assert_eq!(p.set_level(false), Err(8));
    p.set_level(true).unwrap();
    assert!(matches!(STEPS[0], InitStep::Command(0x36, [0xd8])));
    assert!(matches!(STEPS[1], InitStep::DelayMs(120)));
}
