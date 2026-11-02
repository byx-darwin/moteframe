// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Opt-in observations of idle lines after both STOP sequences.
use crate::{BitBangI2c, I2cError, Line, NormalAddress7};

/// A checked read failure, separate from the legacy exhaustive error enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckedReadError {
    /// The first legacy validation, ACK or SCL polling failure.
    Protocol(I2cError),
    /// Both actual lines were not observed high within the post-STOP budget.
    BusNotIdle,
}

impl<C: Line, D: Line, F: FnMut()> BitBangI2c<C, D, F> {
    fn checked_stop(&mut self) -> Result<(), CheckedReadError> {
        self.stop().map_err(CheckedReadError::Protocol)?;
        for _ in 0..self.polls {
            if self.clock.high() && self.data.high() {
                return Ok(());
            }
            self.pause();
        }
        Err(CheckedReadError::BusNotIdle)
    }

    // Keep this opt-in sequence separate so legacy consumers retain their exact
    // methods, errors, polling and linked workload. No runtime mode is stored.
    fn read_checked_inner(
        &mut self,
        address: u8,
        register: u8,
        out: &mut [u8],
    ) -> Result<(), CheckedReadError> {
        self.start().map_err(CheckedReadError::Protocol)?;
        self.write_byte(address << 1)
            .map_err(CheckedReadError::Protocol)?;
        self.write_byte(register)
            .map_err(CheckedReadError::Protocol)?;
        self.checked_stop()?;
        self.start().map_err(CheckedReadError::Protocol)?;
        self.write_byte((address << 1) | 1)
            .map_err(CheckedReadError::Protocol)?;
        let count = out.len();
        for (index, byte) in out.iter_mut().enumerate() {
            *byte = self
                .read_byte(index + 1 == count)
                .map_err(CheckedReadError::Protocol)?;
        }
        Ok(())
    }

    /// Reads an ordinary target and checks actual SCL/SDA after each STOP.
    ///
    /// Pointer write and read START remain STOP-separated. Each STOP first uses
    /// the existing SCL wait, then observes both lines at most `polls` times,
    /// pausing after unsuccessful observations. This is not arbitration, a
    /// calibrated timing check, bus-clear recovery or an electrical guarantee.
    /// Platform line/delay calls must themselves finish within finite budgets.
    ///
    /// # Errors
    /// Empty output is rejected before I/O. Every valid call attempts a final
    /// checked STOP, even on failure; the first error wins. Master outputs are
    /// released, but a target can still hold the wires low. Discard all output
    /// on error, investigate hardware before reuse, and never assume a retry
    /// happened. This API does not latch a fault or perform automatic recovery.
    pub fn read_checked(
        &mut self,
        address: NormalAddress7,
        register: u8,
        out: &mut [u8],
    ) -> Result<(), CheckedReadError> {
        if out.is_empty() {
            return Err(CheckedReadError::Protocol(I2cError::Invalid));
        }
        let result = self.read_checked_inner(address.get(), register, out);
        let cleanup = self.checked_stop();
        result.and(cleanup)
    }
}
