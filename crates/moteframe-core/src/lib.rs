// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Hardware independent display and board contracts. No heap, RTOS or vendor types.
#![no_std]
#![forbid(unsafe_code)]

/// Panel size in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dimensions {
    /// Width in pixels.
    pub width: u16,
    /// Height in pixels.
    pub height: u16,
}
impl Dimensions {
    /// Creates panel dimensions. Zero dimensions admit no valid rectangle.
    #[must_use]
    pub const fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }
}

/// A rectangle with an exclusive end, expressed by origin and extent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    /// Left coordinate.
    pub x: u16,
    /// Top coordinate.
    pub y: u16,
    /// Width in pixels.
    pub width: u16,
    /// Height in pixels.
    pub height: u16,
}
impl Rect {
    /// Creates a rectangle; drivers validate it before I/O.
    #[must_use]
    pub const fn new(x: u16, y: u16, width: u16, height: u16) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
    /// Whether the nonempty rectangle fits without coordinate overflow.
    #[must_use]
    pub fn fits(self, dimensions: Dimensions) -> bool {
        self.width > 0
            && self.height > 0
            && self
                .x
                .checked_add(self.width)
                .is_some_and(|end| end <= dimensions.width)
            && self
                .y
                .checked_add(self.height)
                .is_some_and(|end| end <= dimensions.height)
    }
    /// Number of pixels; multiplication uses a wide intermediate on 32-bit targets.
    #[must_use]
    pub const fn area(self) -> u32 {
        self.width as u32 * self.height as u32
    }
}

/// Synchronous panel transport. Implementations control CS and DC internally.
///
/// Commands finish before returning. Consecutive `pixels` calls continue the same
/// RAM write, including across CS boundaries supported by the selected panel.
/// This is an 8-bit command/data byte protocol, not every display transport.
/// All I/O and cleanup must have finite platform-defined waits. Errors may leave
/// panel state uncertain; an implementation must not silently resend writes.
pub trait DisplayBus {
    /// Board transport failure.
    type Error;
    /// Writes a command and its data with the appropriate DC levels.
    /// # Errors
    /// Returns a transport error if any byte cannot be sent.
    fn command(&mut self, command: u8, data: &[u8]) -> Result<(), Self::Error>;
    /// Writes data synchronously. The slice may be reused after return.
    /// # Errors
    /// Returns a transport error if any byte cannot be sent.
    fn pixels(&mut self, data: &[u8]) -> Result<(), Self::Error>;
}

/// Millisecond delays supplied by the board, without assuming an RTOS.
/// Implementations must return after a finite platform-defined wait.
pub trait DelayMs {
    /// Waits for at least this many milliseconds.
    fn delay_ms(&mut self, milliseconds: u16);
}

/// Replaceable display interface; no framebuffer or business UI is required.
///
/// After any return, the caller may immediately reuse the input pixels.
/// A successful call finishes its logical write and any synchronous transport
/// it initiates. This does not promise optical visibility or scan completion,
/// nor define a separate present/refresh lifecycle for a framebuffer adapter.
/// Errors may follow partial output; atomic rollback and safe retries are not
/// implied. Keep dimensions and external command state stable for a frame.
/// All I/O must return within a finite platform-defined budget.
pub trait Display {
    /// Logical pixel color. The driver owns its physical encoding.
    type Color: Copy;
    /// Validation or device failure.
    type Error;
    /// Logical geometry.
    fn dimensions(&self) -> Dimensions;
    /// Writes exactly one pixel per rectangle position, in row-major order.
    /// # Errors
    /// Rejects invalid rectangles, lengths, unavailable devices or I/O failures.
    fn write(&mut self, rectangle: Rect, pixels: &[Self::Color]) -> Result<(), Self::Error>;
}

/// Optional board startup glue. Peripheral reset/power remain board concerns.
pub trait Board {
    /// A board-specific initialization error.
    type Error;
    /// Prepares clocks, GPIO and buses before peripheral initialization.
    /// # Errors
    /// Returns the board failure rather than treating failed setup as success.
    fn initialize(&mut self) -> Result<(), Self::Error>;
}

/// Minimal synchronous SPI write contract. A board may bridge its own HAL here.
///
/// Unlike a HAL `SpiBus::write`, successful writes must physically complete
/// before returning. All operations and cleanup must have finite platform-defined
/// waits. An error must not cause a hidden resend or retain access to `bytes`.
pub trait SpiWrite {
    /// SPI failure.
    type Error;
    /// Completes all transfers before returning, so CS may be released safely.
    /// # Errors
    /// Reports transfer or bus failure.
    fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;
}

/// Minimal GPIO output contract for CS/DC wiring.
/// Level changes must return within a finite platform-defined budget.
pub trait OutputPin {
    /// GPIO failure.
    type Error;
    /// Drives the output level; configuration belongs to the board.
    /// # Errors
    /// Reports inaccessible or failed GPIO writes.
    fn set_level(&mut self, high: bool) -> Result<(), Self::Error>;
}

/// Shared SPI ownership using embedded-hal1 bus/device contracts.
pub mod spi;

/// Explicit embedded-hal 1.0 bridges preserving the portable contracts.
pub mod hal;

mod read;
pub use read::ReadAt;
