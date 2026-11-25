//! Allocation-free, bounded, read-only resource contracts and bitmap spans.
#![no_std]
#![forbid(unsafe_code)]
mod bitmap;
mod source;
pub use bitmap::{Bitmap, Format};
pub use device_core::ReadAt;
pub use device_spi_nor::{NorError, ReadOnlyNor};
pub use source::{Bounds, Memory, ReadError, Region};
