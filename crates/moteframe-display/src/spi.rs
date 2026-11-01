// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Synchronous SPI plus GPIO transport; cleanup attempts CS high on every path.
use moteframe_core::{DisplayBus, OutputPin, SpiWrite};

/// SPI or control-line failure; preserves the first error if cleanup also fails.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpiError<S, C, D> {
    /// SPI transfer failed.
    Spi(S),
    /// Chip-select operation failed.
    Select(C),
    /// Command/data operation failed.
    DataCommand(D),
}

/// A dedicated SPI bus with independently owned CS and DC output pins.
///
/// Board code must configure mode/frequency and idle CS high before construction.
/// Shared buses require a different `DisplayBus` adapter locking the entire transaction.
#[derive(Debug)]
pub struct SpiPanelBus<S, C, D> {
    spi: S,
    select: C,
    data_command: D,
}
impl<S, C, D> SpiPanelBus<S, C, D> {
    /// Wraps preconfigured hardware. No GPIO is changed during construction.
    #[must_use]
    pub const fn new(spi: S, select: C, data_command: D) -> Self {
        Self {
            spi,
            select,
            data_command,
        }
    }
    /// Releases resources for reconfiguration or recovery.
    #[must_use]
    pub fn into_parts(self) -> (S, C, D) {
        (self.spi, self.select, self.data_command)
    }
}
impl<S: SpiWrite, C: OutputPin, D: OutputPin> SpiPanelBus<S, C, D> {
    fn command_inner(
        &mut self,
        command: u8,
        data: &[u8],
    ) -> Result<(), <Self as DisplayBus>::Error> {
        self.data_command
            .set_level(false)
            .map_err(SpiError::DataCommand)?;
        self.select.set_level(false).map_err(SpiError::Select)?;
        self.spi.write(&[command]).map_err(SpiError::Spi)?;
        if !data.is_empty() {
            self.data_command
                .set_level(true)
                .map_err(SpiError::DataCommand)?;
            self.spi.write(data).map_err(SpiError::Spi)?;
        }
        Ok(())
    }
    fn pixels_inner(&mut self, data: &[u8]) -> Result<(), <Self as DisplayBus>::Error> {
        self.data_command
            .set_level(true)
            .map_err(SpiError::DataCommand)?;
        self.select.set_level(false).map_err(SpiError::Select)?;
        self.spi.write(data).map_err(SpiError::Spi)
    }
}
impl<S: SpiWrite, C: OutputPin, D: OutputPin> DisplayBus for SpiPanelBus<S, C, D> {
    type Error = SpiError<S::Error, C::Error, D::Error>;
    fn command(&mut self, command: u8, data: &[u8]) -> Result<(), Self::Error> {
        let result = self.command_inner(command, data);
        let cleanup = self.select.set_level(true).map_err(SpiError::Select);
        result.and(cleanup)
    }
    fn pixels(&mut self, data: &[u8]) -> Result<(), Self::Error> {
        let result = self.pixels_inner(data);
        let cleanup = self.select.set_level(true).map_err(SpiError::Select);
        result.and(cleanup)
    }
}
