// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Streaming RGB565 driver for panels supporting the configured DCS commands.
//! Initialization and offsets belong to a verified panel profile, not a filename.
#![no_std]
#![forbid(unsafe_code)]

use moteframe_core::{DelayMs, Dimensions, Display, DisplayBus, Rect, Rgb565};

/// One operation in the caller's verified panel initialization sequence.
#[derive(Clone, Copy, Debug)]
pub enum InitStep<'a> {
    /// Command and parameters, sent synchronously.
    Command(u8, &'a [u8]),
    /// Panel settling time.
    DelayMs(u16),
}

/// Validation or transport error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error<E> {
    /// Initialization has not succeeded, or a previous I/O operation failed.
    NotInitialized,
    /// Empty, out of bounds or physically overflowing rectangle.
    InvalidRectangle,
    /// Pixel count does not equal rectangle area.
    PixelCount,
    /// Underlying bus failure. Reinitialize before writing again.
    Bus(E),
}

/// DCS address-window driver with a 64-byte streaming buffer.
///
/// Use only with panels accepting 0x2A/0x2B/0x2C, RGB565 MSB first, and
/// continued RAM writes across the transport's chunk boundaries.
/// The driver requires exclusive control of the panel command state. Unchanged
/// axes are cached; call `initialize` again after any external reset or command.
#[derive(Debug)]
pub struct DcsDisplay<B> {
    bus: B,
    dimensions: Dimensions,
    offset: (u16, u16),
    initialized: bool,
    window: Option<Rect>,
}
impl<B: DisplayBus> DcsDisplay<B> {
    /// Constructs an uninitialized driver. Board code must power/reset the panel.
    #[must_use]
    pub const fn new(bus: B, dimensions: Dimensions, offset: (u16, u16)) -> Self {
        Self {
            bus,
            dimensions,
            offset,
            initialized: false,
            window: None,
        }
    }
    /// Executes the complete caller-supplied initialization sequence.
    /// An empty sequence means the caller already initialized the panel.
    /// # Errors
    /// Propagates bus failures and keeps the driver unavailable on failure.
    pub fn initialize<D: DelayMs>(
        &mut self,
        sequence: &[InitStep<'_>],
        delay: &mut D,
    ) -> Result<(), Error<B::Error>> {
        self.initialized = false;
        self.window = None;
        for step in sequence {
            match step {
                InitStep::Command(command, data) => {
                    self.bus.command(*command, data).map_err(Error::Bus)?;
                }
                InitStep::DelayMs(milliseconds) => delay.delay_ms(*milliseconds),
            }
        }
        self.initialized = true;
        Ok(())
    }
    /// Releases the bus for board-specific power/reset or recovery.
    #[must_use]
    pub fn into_bus(self) -> B {
        self.bus
    }

    fn send(&mut self, rectangle: Rect, pixels: &[Rgb565]) -> Result<(), B::Error> {
        let left = rectangle.x + self.offset.0;
        let top = rectangle.y + self.offset.1;
        let right = left + (rectangle.width - 1);
        let bottom = top + (rectangle.height - 1);
        let [x0, x1] = left.to_be_bytes();
        let [x2, x3] = right.to_be_bytes();
        let [y0, y1] = top.to_be_bytes();
        let [y2, y3] = bottom.to_be_bytes();
        if self
            .window
            .is_none_or(|w| w.x != rectangle.x || w.width != rectangle.width)
        {
            self.bus.command(0x2a, &[x0, x1, x2, x3])?;
        }
        if self
            .window
            .is_none_or(|w| w.y != rectangle.y || w.height != rectangle.height)
        {
            self.bus.command(0x2b, &[y0, y1, y2, y3])?;
        }
        self.bus.command(0x2c, &[])?;
        let mut buffer = [0; 64];
        for chunk in pixels.chunks(32) {
            for (destination, pixel) in buffer.chunks_exact_mut(2).zip(chunk) {
                destination.copy_from_slice(&pixel.0.to_be_bytes());
            }
            self.bus.pixels(&buffer[..chunk.len() * 2])?;
        }
        self.window = Some(rectangle);
        Ok(())
    }
}
impl<B: DisplayBus> Display for DcsDisplay<B> {
    type Error = Error<B::Error>;
    fn dimensions(&self) -> Dimensions {
        self.dimensions
    }
    fn write(&mut self, rectangle: Rect, pixels: &[Rgb565]) -> Result<(), Self::Error> {
        if !self.initialized {
            return Err(Error::NotInitialized);
        }
        if !rectangle.fits(self.dimensions)
            || rectangle
                .x
                .checked_add(self.offset.0)
                .and_then(|n| n.checked_add(rectangle.width - 1))
                .is_none()
            || rectangle
                .y
                .checked_add(self.offset.1)
                .and_then(|n| n.checked_add(rectangle.height - 1))
                .is_none()
        {
            return Err(Error::InvalidRectangle);
        }
        if u32::try_from(pixels.len()).ok() != Some(rectangle.area()) {
            return Err(Error::PixelCount);
        }
        match self.send(rectangle, pixels) {
            Ok(()) => Ok(()),
            Err(error) => {
                self.initialized = false;
                self.window = None;
                Err(Error::Bus(error))
            }
        }
    }
}

mod spi;
pub use spi::{SpiError, SpiPanelBus};

mod shared;
pub use shared::SharedPanelBus;
