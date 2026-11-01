// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Bounded open-drain pointer reads. No controller payload writes or updater API.
#![no_std]
#![forbid(unsafe_code)]

/// Open-drain wire; release never actively drives a high level.
pub trait Line {
    /// Pull the wire low.
    fn low(&mut self);
    /// Release the wire.
    fn release(&mut self);
    /// Sample its actual electrical input.
    fn high(&self) -> bool;
}
/// Protocol or electrical failure; callers may retry only after investigating the bus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum I2cError {
    /// Invalid address, empty read or zero clock budget.
    Invalid,
    /// SDA remains low before START.
    Busy,
    /// Slave did not acknowledge.
    Nack,
    /// SCL did not rise within the finite budget.
    ClockTimeout,
}
/// Generic software bus; delay is one caller-chosen half-cycle, not a calibrated rate.
#[derive(Debug)]
pub struct BitBangI2c<C, D, F> {
    clock: C,
    data: D,
    delay: F,
    polls: u16,
}
impl<C: Line, D: Line, F: FnMut()> BitBangI2c<C, D, F> {
    /// Takes released open-drain wires and a bounded clock-stretch budget.
    /// # Errors
    /// Rejects a zero polling budget.
    pub fn new(mut clock: C, mut data: D, delay: F, polls: u16) -> Result<Self, I2cError> {
        if polls == 0 {
            return Err(I2cError::Invalid);
        }
        data.release();
        clock.release();
        Ok(Self {
            clock,
            data,
            delay,
            polls,
        })
    }
    fn pause(&mut self) {
        (self.delay)();
    }
    fn clock_high(&mut self) -> Result<(), I2cError> {
        self.clock.release();
        for _ in 0..self.polls {
            if self.clock.high() {
                return Ok(());
            }
            self.pause();
        }
        Err(I2cError::ClockTimeout)
    }
    fn start(&mut self) -> Result<(), I2cError> {
        self.data.release();
        self.clock_high()?;
        if !self.data.high() {
            return Err(I2cError::Busy);
        }
        self.pause();
        self.data.low();
        self.pause();
        self.clock.low();
        Ok(())
    }
    fn stop(&mut self) -> Result<(), I2cError> {
        self.data.low();
        self.pause();
        let result = self.clock_high();
        self.pause();
        self.data.release();
        self.pause();
        result
    }
    fn write_byte(&mut self, byte: u8) -> Result<(), I2cError> {
        for bit in (0..8).rev() {
            if byte & (1 << bit) == 0 {
                self.data.low();
            } else {
                self.data.release();
            }
            self.pause();
            self.clock_high()?;
            self.pause();
            self.clock.low();
        }
        self.data.release();
        self.pause();
        self.clock_high()?;
        self.pause();
        let nack = self.data.high();
        self.clock.low();
        if nack { Err(I2cError::Nack) } else { Ok(()) }
    }
    fn read_byte(&mut self, last: bool) -> Result<u8, I2cError> {
        self.data.release();
        let mut value = 0;
        for _ in 0..8 {
            self.pause();
            self.clock_high()?;
            self.pause();
            value = (value << 1) | u8::from(self.data.high());
            self.clock.low();
        }
        if last {
            self.data.release();
        } else {
            self.data.low();
        }
        self.pause();
        self.clock_high()?;
        self.pause();
        self.clock.low();
        self.data.release();
        Ok(value)
    }
    fn read_inner(&mut self, address: u8, register: u8, out: &mut [u8]) -> Result<(), I2cError> {
        self.start()?;
        self.write_byte(address << 1)?;
        self.write_byte(register)?;
        self.stop()?;
        self.start()?;
        self.write_byte((address << 1) | 1)?;
        let count = out.len();
        for (index, byte) in out.iter_mut().enumerate() {
            *byte = self.read_byte(index + 1 == count)?;
        }
        Ok(())
    }
    /// Reads normal8-bit registers with STOP between pointer write and read START.
    /// The only writes are the address and register pointer, never controller payloads.
    /// # Errors
    /// Rejects non7-bit addresses/empty reads and propagates ACK or clock failures.
    /// Both wires are released on return; an errored output slice must be discarded.
    pub fn read(&mut self, address: u8, register: u8, out: &mut [u8]) -> Result<(), I2cError> {
        if address >= 128 || out.is_empty() {
            return Err(I2cError::Invalid);
        }
        let result = self.read_inner(address, register, out);
        let cleanup = self.stop();
        result.and(cleanup)
    }
}
