// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Synthetic resource catalog and a stable, format-aware scene.
use crate::{ASSET_BYTES, Snapshot, services::Damage};
use core::num::NonZeroU32;
use moteframe_assets::{AssetError, Mono8, Raster, Rgb565Be};
use moteframe_core::{Dimensions, ReadAt};
use moteframe_storage::{ReadRef, ReadWindow};
use moteframe_ui::graphics::geometry::Dimensions as _;
use moteframe_ui::{
    ConfigError, ResourceKey, ResourceMetadata, ResourceView, Tile,
    graphics::{
        pixelcolor::{BinaryColor, Rgb565, RgbColor},
        prelude::*,
        primitives::Rectangle,
    },
};
/// Fixed logical example geometry, independent of panel wiring.
pub const SIZE: Dimensions = Dimensions::new(32, 16);
/// Logical synthetic resource bytes; fixture storage has a four-byte prefix.
pub const RESOURCE_LENGTH: u32 = 31;
/// Immutable descriptors and identity; never owns a reader or loaded pixels.
#[derive(Clone, Copy, Debug)]
pub struct Catalog {
    glyph: Raster<Mono8>,
    strip: Raster<Rgb565Be>,
    key: ResourceKey,
    metadata: ResourceMetadata,
}
impl Catalog {
    /// Validates the two fixed synthetic descriptors with a caller resource revision.
    /// # Errors
    /// Returns a metadata validation failure before any source access.
    pub fn new(revision: u32) -> Result<Self, ConfigError> {
        Ok(Self {
            glyph: Raster::new(0, 3, 5).map_err(|_| ConfigError::InvalidResources)?,
            strip: Raster::new(15, 8, 1).map_err(|_| ConfigError::InvalidResources)?,
            key: ResourceKey::new(NonZeroU32::MIN, revision),
            metadata: ResourceMetadata::new(8, 5, 2)?,
        })
    }
    /// Immutable collection/revision identity.
    #[must_use]
    pub const fn key(&self) -> ResourceKey {
        self.key
    }
    /// Borrow the validated descriptors; mutable I/O remains a separate argument.
    /// # Errors
    /// Rejects inconsistent identity/metadata, without I/O.
    pub fn view(&self) -> Result<ResourceView<'_, Self>, ConfigError> {
        ResourceView::new(self.key, self.metadata, self)
    }
}
/// Caller-owned decoder storage, distinct from UI transfer scratch.
pub struct RenderScratch {
    mono: [BinaryColor; 3],
    rgb: [Rgb565; 8],
    bytes: [u8; ASSET_BYTES],
}
impl Default for RenderScratch {
    fn default() -> Self {
        Self::new()
    }
}
impl RenderScratch {
    /// Allocates only actual profile-selected arrays.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            mono: [BinaryColor::Off; 3],
            rgb: [Rgb565::BLACK; 8],
            bytes: [0; ASSET_BYTES],
        }
    }
}
/// Original window validation/read or stored-pixel decode failure.
pub type SceneError<E> = AssetError<moteframe_storage::ReadError<E>>;
/// One next-frame union; marker-only activity never reads resource pixels.
pub(crate) fn damage_rect(d: Damage) -> Option<Rectangle> {
    let (x, width) = match (d.full, d.rx, d.tx) {
        (true, _, _) => (0, 32),
        (false, true, true) => (1, 6),
        (false, true, false) => (1, 2),
        (false, false, true) => (5, 2),
        _ => return None,
    };
    Some(Rectangle::new(
        Point::new(x, if d.full { 0 } else { 14 }),
        Size::new(width, if d.full { 16 } else { 1 }),
    ))
}
/// Render a frozen snapshot into the current clip using real source-backed rasters.
/// The physical source exposes a four-byte prefix; `ReadWindow` restricts all
/// raster reads to the 31-byte collection. The caller keeps contents stable for
/// this complete frame, including across service callbacks.
/// # Errors
/// Original window/read/decode error; discard this tile and retry full pending work.
pub fn render<R: ReadAt + ?Sized>(
    tile: &mut Tile<'_, Rgb565>,
    snapshot: Snapshot,
    catalog: &Catalog,
    reader: &mut R,
    scratch: &mut RenderScratch,
) -> Result<(), SceneError<R::Error>> {
    let clip = tile.bounding_box();
    let y = clip.top_left.y;
    if (1..6).contains(&y)
        && clip.top_left.x < 4
        && clip.top_left.x + i32::try_from(clip.size.width).unwrap_or(i32::MAX) > 1
    {
        let mut source = ReadWindow::new(ReadRef::new(&mut *reader), 4, RESOURCE_LENGTH)
            .map_err(|_| AssetError::Bounds)?;
        catalog.glyph.read_span(
            &mut source,
            0,
            u16::try_from(y - 1).map_err(|_| AssetError::Bounds)?,
            &mut scratch.mono,
            &mut scratch.bytes,
        )?;
        let pixels = scratch.mono.iter().enumerate().map(|(x, p)| {
            Pixel(
                Point::new(1 + i32::try_from(x).unwrap_or(0), y),
                if *p == BinaryColor::On {
                    Rgb565::WHITE
                } else {
                    Rgb565::BLACK
                },
            )
        });
        let _ = tile.draw_iter(pixels);
    }
    if y == 1
        && clip.top_left.x < 16
        && clip.top_left.x + i32::try_from(clip.size.width).unwrap_or(i32::MAX) > 8
    {
        let mut source = ReadWindow::new(ReadRef::new(reader), 4, RESOURCE_LENGTH)
            .map_err(|_| AssetError::Bounds)?;
        catalog
            .strip
            .read_span(&mut source, 0, 0, &mut scratch.rgb, &mut scratch.bytes)?;
        let _ = tile.draw_iter(
            scratch
                .rgb
                .iter()
                .enumerate()
                .map(|(x, p)| Pixel(Point::new(8 + i32::try_from(x).unwrap_or(0), 1), *p)),
        );
    }
    let selected = [Rgb565::RED, Rgb565::GREEN, Rgb565::BLUE, Rgb565::WHITE]
        [usize::from(snapshot.selection % 4)];
    let _ = tile.fill_solid(
        &Rectangle::new(Point::new(1, 8), Size::new(30, 4)),
        selected,
    );
    for (x, active, color) in [
        (1, snapshot.rx_active, Rgb565::GREEN),
        (5, snapshot.tx_active, Rgb565::BLUE),
        (9, snapshot.connected, Rgb565::WHITE),
    ] {
        let _ = tile.fill_solid(
            &Rectangle::new(Point::new(x, 14), Size::new(2, 1)),
            if active { color } else { Rgb565::BLACK },
        );
    }
    Ok(())
}

impl core::fmt::Debug for RenderScratch {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RenderScratch")
            .field("byte_capacity", &ASSET_BYTES)
            .finish_non_exhaustive()
    }
}
