// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Portable, allocation-free UI infrastructure over the device display contract.
#![no_std]
#![forbid(unsafe_code)]

/// Shared drawing, font, text and image contracts from embedded-graphics.
pub use embedded_graphics as graphics;

mod geometry;
mod layout;
mod refresh;
mod stream;

pub use geometry::clip;
pub use layout::{Axis, LayoutError, Stack};
pub use refresh::{
    CooperativeError, DamageError, DirtyRegion, RefreshError, Tile, damage_cooperative, repaint,
    repaint_cooperative,
};
pub use stream::StreamTarget;
