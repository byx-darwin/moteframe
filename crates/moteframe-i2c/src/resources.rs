// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Additive address validation and resource-preserving initialization.
use core::fmt;

use crate::{BitBangI2c, I2cError, Line};

/// An ordinary seven-bit target address, excluding reserved/special uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NormalAddress7(u8);
impl NormalAddress7 {
    /// Validates an ordinary target address before any bus activity.
    /// # Errors
    /// Returns `Invalid` outside `0x08..=0x77`. Use raw `read` deliberately for
    /// system-specific reserved-address use; it may have broadcast side effects.
    pub const fn new(address: u8) -> Result<Self, I2cError> {
        if address >= 0x08 && address <= 0x77 {
            Ok(Self(address))
        } else {
            Err(I2cError::Invalid)
        }
    }

    /// Returns the right-aligned seven-bit address.
    #[must_use]
    pub const fn get(self) -> u8 {
        self.0
    }
}

/// Failed initialization retaining the original clock, data and delay objects.
/// Taking them back performs no I/O and does not repair hardware.
pub struct InitError<C, D, F> {
    error: I2cError,
    clock: C,
    data: D,
    delay: F,
}
impl<C, D, F> InitError<C, D, F> {
    /// Borrows the initialization failure without surrendering resources.
    #[must_use]
    pub const fn error(&self) -> &I2cError {
        &self.error
    }

    /// Returns the reason and all original resources without I/O.
    #[must_use]
    pub fn into_parts(self) -> (I2cError, C, D, F) {
        (self.error, self.clock, self.data, self.delay)
    }
}
impl<C, D, F> fmt::Debug for InitError<C, D, F> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("InitError")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}
impl<C, D, F> BitBangI2c<C, D, F> {
    /// Returns the original clock, data and delay without changing any line.
    /// The physical bus may be uncertain; investigate and repair it explicitly
    /// before rebuilding. This is not bus clear, reset or recovery.
    #[must_use]
    pub fn into_parts(self) -> (C, D, F) {
        (self.clock, self.data, self.delay)
    }
}
impl<C: Line, D: Line, F: FnMut()> BitBangI2c<C, D, F> {
    /// Releases the configured open-drain outputs and keeps resources on failure.
    /// # Errors
    /// A zero polling budget returns `InitError` with all original objects,
    /// without I/O. The legacy `new` instead consumes these objects on error.
    pub fn try_new(
        mut clock: C,
        mut data: D,
        delay: F,
        polls: u16,
    ) -> Result<Self, InitError<C, D, F>> {
        if polls == 0 {
            return Err(InitError {
                error: I2cError::Invalid,
                clock,
                data,
                delay,
            });
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

    /// Reads an ordinary target with the legacy STOP/release completion semantics.
    /// Use `read_checked` to additionally observe both lines high after each STOP.
    /// # Errors
    /// Rejects empty output before I/O and propagates ACK/clock failures. Discard
    /// the whole output on error; no automatic retry or recovery is performed.
    pub fn read_normal(
        &mut self,
        address: NormalAddress7,
        register: u8,
        out: &mut [u8],
    ) -> Result<(), I2cError> {
        self.read(address.get(), register, out)
    }
}
