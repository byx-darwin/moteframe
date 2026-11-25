//! Hardware independent display and board contracts. No heap, RTOS or vendor types.
#![no_std]

pub use moteframe_core::{Dimensions, Rect};

/// A packed RGB565 pixel. Transport byte order is specified by the driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb565(pub u16);

/// Synchronous panel transport. Implementations control CS and DC internally.
///
/// Commands finish before returning. Consecutive `pixels` calls continue the same
/// RAM write, including across CS boundaries supported by the selected panel.
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
pub trait DelayMs {
    /// Waits for at least this many milliseconds.
    fn delay_ms(&mut self, milliseconds: u16);
}

/// Replaceable display interface; no framebuffer or business UI is required.
pub trait Display {
    /// Validation or device failure.
    type Error;
    /// Logical geometry.
    fn dimensions(&self) -> Dimensions;
    /// Writes exactly one pixel per rectangle position, in row-major order.
    /// # Errors
    /// Rejects invalid rectangles, lengths, unavailable devices or I/O failures.
    fn write(&mut self, rectangle: Rect, pixels: &[Rgb565]) -> Result<(), Self::Error>;
    /// Accept native logical colors without changing the legacy word API.
    /// Native-backed adapters override this to avoid a second pixel buffer.
    /// # Errors
    /// Invalid full input is rejected before I/O; actual partial-write failures are retained.
    #[cfg(feature = "native-colors")]
    fn write_rgb565(
        &mut self,
        rectangle: Rect,
        pixels: &[embedded_graphics_core::pixelcolor::Rgb565],
    ) -> Result<(), NativeWriteError<Self::Error>> {
        native::write(self, rectangle, pixels)
    }
}

/// Board startup interface. Display reset/power and BLE boot remain board concerns.
pub trait Board {
    /// A board-specific initialization error.
    type Error;
    /// Prepares clocks, GPIO and buses before peripheral initialization.
    /// # Errors
    /// Returns the board failure rather than treating failed setup as success.
    fn initialize(&mut self) -> Result<(), Self::Error>;
}

/// Minimal synchronous SPI write contract. A board may bridge its own HAL here.
pub trait SpiWrite {
    /// SPI failure.
    type Error;
    /// Completes all transfers before returning, so CS may be released safely.
    /// # Errors
    /// Reports transfer or bus failure.
    fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error>;
}

/// Minimal GPIO output contract for CS/DC wiring.
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

mod read;
pub use read::ReadAt;

#[cfg(feature = "native-colors")]
mod native;
#[cfg(feature = "native-colors")]
pub use native::NativeWriteError;
