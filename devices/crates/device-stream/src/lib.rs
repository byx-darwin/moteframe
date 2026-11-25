//! Stream adapters joining portable protocol services without hardware or heap.
#![no_std]
#[cfg(feature = "raw-data")]
mod raw;
#[cfg(feature = "raw-data")]
pub use raw::{RawLaneAdapter, encode_raw_batch};
