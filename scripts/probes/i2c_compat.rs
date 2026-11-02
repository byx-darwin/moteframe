// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Fixed two-byte pointer-read consumer; synthetic I/O, no hardware qualification.
#![no_std]
#![allow(dead_code)]
use core::{cell::Cell, hint::black_box};
use moteframe_i2c::{BitBangI2c, I2cError, Line};

#[derive(Debug)]
struct Pin<'a> {
    trace: &'a Cell<u32>,
    sample: Cell<u32>,
    seed: u32,
    clock: bool,
}
fn event(trace: &Cell<u32>, value: u32) {
    trace.set(trace.get().wrapping_mul(31).wrapping_add(value));
}
impl Line for Pin<'_> {
    fn low(&mut self) {
        event(self.trace, if self.clock { 2 } else { 3 });
    }
    fn release(&mut self) {
        event(self.trace, if self.clock { 5 } else { 7 });
    }
    fn high(&self) -> bool {
        let n = self.sample.get();
        self.sample.set(n.wrapping_add(1));
        event(self.trace, 11);
        if self.clock {
            self.seed != 1
        } else {
            match n {
                0 | 3 => true,
                1 => self.seed == 2,
                2 | 4 => false,
                _ => self.seed.rotate_right(n) & 1 != 0,
            }
        }
    }
}
type Owner<'a> = BitBangI2c<Pin<'a>, Pin<'a>, fn()>;
#[cfg(i2c_layout)]
#[unsafe(no_mangle)]
pub static I2C_OWNER: [u8; core::mem::size_of::<Owner<'static>>()] =
    [0; core::mem::size_of::<Owner<'static>>()];
#[cfg(i2c_layout)]
#[unsafe(no_mangle)]
pub static I2C_PIN: [u8; core::mem::size_of::<Pin<'static>>()] =
    [0; core::mem::size_of::<Pin<'static>>()];

/// Executes the same old public API for runtime input and error cases.
pub fn exercise(seed: u32) -> u32 {
    let trace = Cell::new(seed);
    let pin = |clock| Pin {
        trace: &trace,
        sample: Cell::new(0),
        seed: black_box(seed),
        clock,
    };
    let mut bus = match BitBangI2c::new(pin(true), pin(false), || event(&trace, 13), 3) {
        Ok(bus) => bus,
        Err(_) => return 0,
    };
    let mut out = [0xee; 2];
    let result = bus.read(black_box(0x15), black_box(1), &mut out);
    let code = match result {
        Ok(()) => 17,
        Err(I2cError::Invalid) => 19,
        Err(I2cError::Busy) => 23,
        Err(I2cError::Nack) => 29,
        Err(I2cError::ClockTimeout) => 31,
    };
    event(&trace, code);
    event(&trace, u32::from(out[0]));
    event(&trace, u32::from(out[1]));
    trace.get()
}
// EXTENSION PROBES
// The prefix above is the frozen legacy workload; these opt-in consumers are
// measured separately and are never subtracted from unlike legacy workloads.

#[cfg(i2c_checked)]
struct CheckedPin<'a>(Pin<'a>);
#[cfg(i2c_checked)]
impl Line for CheckedPin<'_> {
    fn low(&mut self) {
        self.0.low();
    }
    fn release(&mut self) {
        self.0.release();
    }
    fn high(&self) -> bool {
        if self.0.clock {
            return self.0.high();
        }
        let n = self.0.sample.get();
        self.0.sample.set(n.wrapping_add(1));
        event(self.0.trace, 11);
        match n {
            0 | 3 | 4 | 22 => true,
            1 => self.0.seed == 2,
            2 | 5 => false,
            _ => self.0.seed.rotate_right(n - 1) & 1 != 0,
        }
    }
}
#[cfg(i2c_normal)]
pub fn exercise_normal(seed: u32) -> u32 {
    let trace = Cell::new(seed);
    let pin = |clock| Pin {
        trace: &trace,
        sample: Cell::new(0),
        seed: black_box(seed),
        clock,
    };
    let mut bus = match BitBangI2c::new(pin(true), pin(false), || event(&trace, 13), 3) {
        Ok(bus) => bus,
        Err(_) => return 0,
    };
    let mut out = [0xee; 2];
    let Ok(address) = moteframe_i2c::NormalAddress7::new(black_box(0x15)) else {
        return 0;
    };
    let result = bus.read_normal(address, black_box(1), &mut out);
    let code = match result {
        Ok(()) => 17,
        Err(I2cError::Invalid) => 19,
        Err(I2cError::Busy) => 23,
        Err(I2cError::Nack) => 29,
        Err(I2cError::ClockTimeout) => 31,
    };
    event(&trace, code);
    event(&trace, u32::from(out[0]));
    event(&trace, u32::from(out[1]));
    trace.get()
}

#[cfg(i2c_checked)]
pub fn exercise_checked(seed: u32) -> u32 {
    let trace = Cell::new(seed);
    let pin = |clock| {
        CheckedPin(Pin {
            trace: &trace,
            sample: Cell::new(0),
            seed: black_box(seed),
            clock,
        })
    };
    let mut bus = match BitBangI2c::new(pin(true), pin(false), || event(&trace, 13), 3) {
        Ok(bus) => bus,
        Err(_) => return 0,
    };
    let mut out = [0xee; 2];
    let Ok(address) = moteframe_i2c::NormalAddress7::new(black_box(0x15)) else {
        return 0;
    };
    let result = bus.read_checked(address, black_box(1), &mut out);
    let code = match result {
        Ok(()) => 17,
        Err(moteframe_i2c::CheckedReadError::BusNotIdle) => 37,
        Err(moteframe_i2c::CheckedReadError::Protocol(I2cError::Invalid)) => 19,
        Err(moteframe_i2c::CheckedReadError::Protocol(I2cError::Busy)) => 23,
        Err(moteframe_i2c::CheckedReadError::Protocol(I2cError::Nack)) => 29,
        Err(moteframe_i2c::CheckedReadError::Protocol(I2cError::ClockTimeout)) => 31,
    };
    event(&trace, code);
    event(&trace, u32::from(out[0]));
    event(&trace, u32::from(out[1]));
    trace.get()
}

/// Resource recovery workload, not a display or read-workload comparison.
#[cfg(i2c_recover)]
pub fn exercise_recover(seed: u32) -> u32 {
    let trace = Cell::new(seed);
    let pin = |clock| Pin {
        trace: &trace,
        sample: Cell::new(0),
        seed: black_box(seed),
        clock,
    };
    let budget = black_box(if seed & 1 == 0 { 0 } else { 3 });
    let (mut clock, mut data, mut delay) =
        match BitBangI2c::try_new(pin(true), pin(false), || event(&trace, 13), budget) {
            Ok(bus) => bus.into_parts(),
            Err(error) => {
                let (reason, c, d, f) = error.into_parts();
                if reason == I2cError::Invalid {
                    event(&trace, 19);
                }
                (c, d, f)
            }
        };
    clock.low();
    data.release();
    delay();
    trace.get()
}
