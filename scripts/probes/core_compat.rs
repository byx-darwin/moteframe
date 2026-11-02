// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Fixed synthetic workloads: no hardware, heap, product code or static state.
#![no_std]
#![allow(dead_code, unused_imports, unused_mut)]
use core::{cell::Cell, hint::black_box};
use embedded_hal::{
    delay::DelayNs,
    digital,
    spi::{ErrorType, SpiBus},
};
use moteframe_core::{DelayMs, Dimensions, Display, OutputPin, Rect, SpiWrite};
use moteframe_display::{DcsDisplay, Rgb565, SharedPanelBus, SpiPanelBus};

#[derive(Clone, Copy, Debug)]
pub enum Fault {
    Write,
    Flush,
    Pin,
}
impl embedded_hal::spi::Error for Fault {
    fn kind(&self) -> embedded_hal::spi::ErrorKind {
        embedded_hal::spi::ErrorKind::Other
    }
}
impl digital::Error for Fault {
    fn kind(&self) -> digital::ErrorKind {
        digital::ErrorKind::Other
    }
}
fn event(trace: &Cell<u32>, word: u32) {
    trace.set(trace.get().wrapping_mul(31).wrapping_add(word));
}
#[derive(Debug)]
pub struct Bus<'a> {
    trace: &'a Cell<u32>,
    failure: u32,
}
impl ErrorType for Bus<'_> {
    type Error = Fault;
}
impl SpiBus for Bus<'_> {
    fn read(&mut self, _: &mut [u8]) -> Result<(), Fault> {
        Err(Fault::Write)
    }
    fn transfer(&mut self, _: &mut [u8], _: &[u8]) -> Result<(), Fault> {
        Err(Fault::Write)
    }
    fn transfer_in_place(&mut self, _: &mut [u8]) -> Result<(), Fault> {
        Err(Fault::Write)
    }
    fn write(&mut self, bytes: &[u8]) -> Result<(), Fault> {
        event(self.trace, 101);
        for &byte in bytes {
            event(self.trace, u32::from(byte));
        }
        if self.failure & 1 != 0 {
            Err(Fault::Write)
        } else {
            Ok(())
        }
    }
    fn flush(&mut self) -> Result<(), Fault> {
        event(self.trace, 103);
        if self.failure & 2 != 0 {
            Err(Fault::Flush)
        } else {
            Ok(())
        }
    }
}
#[derive(Debug)]
pub struct Pin<'a, const N: usize> {
    trace: &'a Cell<u32>,
    identity: u32,
    fail: bool,
    payload: [u8; N],
}
impl<const N: usize> digital::ErrorType for Pin<'_, N> {
    type Error = Fault;
}
impl<const N: usize> digital::OutputPin for Pin<'_, N> {
    fn set_low(&mut self) -> Result<(), Fault> {
        event(self.trace, self.identity);
        if self.fail { Err(Fault::Pin) } else { Ok(()) }
    }
    fn set_high(&mut self) -> Result<(), Fault> {
        event(self.trace, self.identity + 1);
        if self.fail { Err(Fault::Pin) } else { Ok(()) }
    }
}
#[derive(Debug)]
pub struct Delay<'a>(&'a Cell<u32>);
impl DelayNs for Delay<'_> {
    fn delay_ns(&mut self, ns: u32) {
        event(self.0, ns);
    }
    fn delay_ms(&mut self, ms: u32) {
        event(self.0, ms.wrapping_add(107));
    }
}

// A correct manual adapter is the comparison, including required flush cleanup.
#[derive(Debug)]
pub struct ManualSpi<S>(S);
impl<S> ManualSpi<S> {
    const fn new(inner: S) -> Self {
        Self(inner)
    }
}
impl<S: SpiBus> SpiWrite for ManualSpi<S> {
    type Error = S::Error;
    fn write(&mut self, bytes: &[u8]) -> Result<(), S::Error> {
        let first = self.0.write(bytes);
        let cleanup = self.0.flush();
        first.and(cleanup)
    }
}
#[derive(Debug)]
pub struct ManualPin<P>(P);
impl<P> ManualPin<P> {
    const fn new(inner: P) -> Self {
        Self(inner)
    }
}
impl<P: digital::OutputPin> OutputPin for ManualPin<P> {
    type Error = P::Error;
    fn set_level(&mut self, high: bool) -> Result<(), P::Error> {
        if high {
            self.0.set_high()
        } else {
            self.0.set_low()
        }
    }
}
#[derive(Debug)]
pub struct ManualDelay<D>(D);
impl<D> ManualDelay<D> {
    const fn new(inner: D) -> Self {
        Self(inner)
    }
}
impl<D: DelayNs> DelayMs for ManualDelay<D> {
    fn delay_ms(&mut self, ms: u16) {
        DelayNs::delay_ms(&mut self.0, u32::from(ms));
    }
}

#[cfg(not(core_hal))]
use self::{ManualDelay as BridgeDelay, ManualPin as BridgePin, ManualSpi as BridgeSpi};
#[cfg(core_hal)]
use moteframe_core::hal::{
    HalDelay as BridgeDelay, HalOutputPin as BridgePin, HalSpiBus as BridgeSpi,
};

#[cfg(core_large)]
const PAYLOAD: usize = 40;
#[cfg(not(core_large))]
const PAYLOAD: usize = 0;

macro_rules! layout {
    ($name:ident, $align:ident, $ty:ty) => {
        #[cfg(core_layout)]
        #[used]
        pub static $name: [u8; core::mem::size_of::<$ty>()] = [0; core::mem::size_of::<$ty>()];
        #[cfg(core_layout)]
        #[used]
        pub static $align: [u8; core::mem::align_of::<$ty>()] = [0; core::mem::align_of::<$ty>()];
    };
}
type Owner<'a> = moteframe_core::spi::SharedBus<Bus<'a>, Delay<'a>>;
#[cfg(not(core_borrowed))]
type Select<'a> = BridgePin<Pin<'a, PAYLOAD>>;
#[cfg(core_borrowed)]
type Select<'a> = BridgePin<&'a mut Pin<'a, PAYLOAD>>;
#[cfg(not(core_borrowed))]
type WrappedSpi<'a> = BridgeSpi<Bus<'a>>;
#[cfg(core_borrowed)]
type WrappedSpi<'a> = BridgeSpi<&'a mut Bus<'a>>;
#[cfg(not(core_borrowed))]
type RawSpi<'a> = Bus<'a>;
#[cfg(core_borrowed)]
type RawSpi<'a> = &'a mut Bus<'a>;
#[cfg(not(core_borrowed))]
type RawSelect<'a> = Pin<'a, PAYLOAD>;
#[cfg(core_borrowed)]
type RawSelect<'a> = &'a mut Pin<'a, PAYLOAD>;
layout!(MF_OWNER, MA_OWNER, Owner<'static>);
layout!(
    MF_DEVICE,
    MA_DEVICE,
    moteframe_core::spi::Device<'static, Bus<'static>, Delay<'static>, Select<'static>>
);
layout!(MF_SPI, MA_SPI, WrappedSpi<'static>);
layout!(MF_PIN, MA_PIN, Select<'static>);
layout!(MF_DELAY, MA_DELAY, BridgeDelay<Delay<'static>>);
layout!(MF_RAW_SPI, MA_RAW_SPI, RawSpi<'static>);
layout!(MF_RAW_PIN, MA_RAW_PIN, RawSelect<'static>);
layout!(MF_RAW_DELAY, MA_RAW_DELAY, Delay<'static>);
#[cfg(core_recover)]
layout!(MF_CREATE_ERROR, MA_CREATE_ERROR, moteframe_core::spi::DeviceCreateError<Fault, Select<'static>>);
#[cfg(core_recover)]
layout!(
    MF_CREATE_RESULT,
    MA_CREATE_RESULT,
    moteframe_core::spi::TryDeviceResult<'static, Bus<'static>, Delay<'static>, Select<'static>>
);

fn pin<const N: usize>(trace: &Cell<u32>, seed: u32, identity: u32) -> Pin<'_, N> {
    Pin {
        trace,
        identity,
        fail: seed & 4 != 0,
        payload: [seed.to_le_bytes()[0]; N],
    }
}

#[cfg(not(core_recover))]
#[inline(never)]
pub fn exercise(seed: u32) -> u32 {
    let trace = Cell::new(seed.wrapping_add(1));
    let mut bus = Bus {
        trace: &trace,
        failure: seed,
    };
    let mut cs = pin::<0>(&trace, seed, 109);
    let mut dc = pin::<0>(&trace, seed >> 1, 113);
    let mut delay = BridgeDelay::new(Delay(&trace));
    delay.delay_ms(u16::from_le_bytes([
        seed.to_le_bytes()[0],
        seed.to_le_bytes()[1],
    ]));
    #[cfg(core_shared)]
    {
        let owner = moteframe_core::spi::SharedBus::new(bus, Delay(&trace));
        let device = owner.device(BridgePin::new(cs));
        match device {
            Ok(device) => {
                let panel = SharedPanelBus::new(device, BridgePin::new(dc));
                draw(panel, &mut delay, &trace, seed);
            }
            Err(_) => event(&trace, 127),
        }
    }
    #[cfg(not(core_shared))]
    {
        #[cfg(core_borrowed)]
        let panel = SpiPanelBus::new(
            BridgeSpi::new(&mut bus),
            BridgePin::new(&mut cs),
            BridgePin::new(&mut dc),
        );
        #[cfg(not(core_borrowed))]
        let panel = SpiPanelBus::new(BridgeSpi::new(bus), BridgePin::new(cs), BridgePin::new(dc));
        draw(panel, &mut delay, &trace, seed);
    }
    black_box(trace.get())
}

fn draw<B: moteframe_core::DisplayBus, D: DelayMs>(
    bus: B,
    delay: &mut D,
    trace: &Cell<u32>,
    seed: u32,
) {
    let mut screen = DcsDisplay::new(bus, Dimensions::new(33, 1), (0, 0));
    let pixels = [black_box(Rgb565::new(seed.to_le_bytes()[0], 17, 9)); 33];
    if screen.initialize(&[], delay).is_err() {
        event(trace, 131);
        return;
    }
    let area = Rect::new(0, 0, 33, 1);
    event(
        trace,
        if screen.write(area, &pixels).is_ok() {
            137
        } else {
            139
        },
    );
    event(
        trace,
        if screen.write(area, &pixels).is_ok() {
            149
        } else {
            151
        },
    );
}

#[cfg(core_recover)]
#[inline(never)]
pub fn exercise(seed: u32) -> u32 {
    use embedded_hal::spi::SpiDevice;
    let trace = Cell::new(seed.wrapping_add(1));
    let owner = Owner::new(
        Bus {
            trace: &trace,
            failure: seed,
        },
        Delay(&trace),
    );
    let original = black_box(pin::<PAYLOAD>(&trace, seed, 109));
    let select = match owner.try_device(BridgePin::new(original)) {
        Ok(mut device) => {
            event(
                &trace,
                if device.write(&seed.to_le_bytes()).is_ok() {
                    157
                } else {
                    163
                },
            );
            // Exercise faulted/healthy construction and retain the second pin.
            let next = BridgePin::new(pin::<PAYLOAD>(&trace, 0, 173));
            match owner.try_device(next) {
                Ok(device) => {
                    let _ = black_box(device.into_select());
                }
                Err(failed) => {
                    event(&trace, 179);
                    let (_, original) = failed.into_parts();
                    let _ = black_box(original);
                }
            }
            device.into_select()
        }
        Err(failed) => {
            event(&trace, 181);
            let (_, original) = failed.into_parts();
            original
        }
    };
    let selected = black_box(select.into_inner());
    event(&trace, selected.identity);
    for value in black_box(selected.payload) {
        event(&trace, u32::from(value));
    }
    let (bus, mut delay) = black_box(owner.into_parts());
    event(&trace, bus.failure);
    delay.delay_ns(seed & 255);
    black_box(trace.get())
}
