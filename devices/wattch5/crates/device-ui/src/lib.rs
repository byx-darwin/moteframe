//! Portable, allocation-free UI infrastructure over the device display contract.
#![no_std]
#![forbid(unsafe_code)]

/// Shared drawing, font, text and image contracts from embedded-graphics.
pub use embedded_graphics as graphics;
pub use moteframe_ui::{Axis, LayoutError, Stack, clip};

mod geometry;
mod refresh;
mod stream;

pub use refresh::{
    CooperativeError, DamageError, DirtyRegion, RefreshError, Tile, damage_cooperative, repaint,
    repaint_cooperative,
};
pub use stream::StreamTarget;
