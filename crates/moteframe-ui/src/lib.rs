// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Portable, allocation-free UI infrastructure over the device display contract.
#![no_std]
#![forbid(unsafe_code)]

/// Shared drawing, font, text and image contracts from embedded-graphics.
pub use embedded_graphics as graphics;

mod configuration;
mod geometry;
#[cfg(feature = "layout")]
mod layout;
#[cfg(feature = "refresh")]
mod refresh;
mod resources;
#[cfg(feature = "refresh")]
mod session;
mod stream;

pub use geometry::clip;
#[cfg(feature = "layout")]
pub use layout::{Axis, LayoutError, Stack};
#[cfg(feature = "refresh")]
pub use refresh::{
    CooperativeError, DirtyRegion, RefreshError, Tile, repaint, repaint_cooperative,
};
#[cfg(feature = "damage")]
pub use refresh::{DamageError, damage_cooperative};
pub use stream::StreamTarget;

pub use configuration::{ConfigError, Palette, UiContext, UiSettings, UiState, UpdateImpact};
#[cfg(feature = "layout")]
pub use configuration::{LayoutSettings, Padding};
pub use resources::{ResourceKey, ResourceMetadata, ResourceView};

#[cfg(feature = "refresh")]
pub use session::{
    BindingState, DeferredInvalidation, DisplayBinding, DrawOutcome, RedrawMode, RefreshSession,
    RefreshSettings, SessionError, TilePixels,
};

#[cfg(feature = "frame-pacing")]
mod pacing;
#[cfg(feature = "frame-pacing")]
pub use pacing::{FrameInterval, PacingState};
