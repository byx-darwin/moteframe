// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

use core::convert::Infallible;

use embedded_graphics::{pixelcolor::Rgb565, prelude::*, primitives::Rectangle};
use moteframe_core::{Dimensions as DisplaySize, Display, Rect, Rgb565 as Packed};

use crate::geometry::{clip, contiguous_pixels, visible};

/// Constant-memory union of invalidated rectangles for one logical display size.
#[derive(Clone, Copy, Debug)]
pub struct DirtyRegion {
    dimensions: DisplaySize,
    pending: Option<Rect>,
}

impl DirtyRegion {
    /// Starts clean. Zero dimensions admit no visible invalidations.
    #[must_use]
    pub const fn new(dimensions: DisplaySize) -> Self {
        Self {
            dimensions,
            pending: None,
        }
    }

    /// Adds the visible portion of a rectangle to the pending union.
    pub fn invalidate(&mut self, area: Rectangle) {
        let Some(next) = clip(area, self.dimensions) else {
            return;
        };
        self.pending = Some(match self.pending {
            None => next,
            Some(previous) => {
                let x = previous.x.min(next.x);
                let y = previous.y.min(next.y);
                let right = (previous.x + previous.width).max(next.x + next.width);
                let bottom = (previous.y + previous.height).max(next.y + next.height);
                Rect::new(x, y, right - x, bottom - y)
            }
        });
    }

    /// Invalidates the entire display, if it has a nonzero area.
    pub fn invalidate_all(&mut self) {
        self.invalidate(Rectangle::new(
            Point::zero(),
            Size::new(
                u32::from(self.dimensions.width),
                u32::from(self.dimensions.height),
            ),
        ));
    }

    /// Returns the current visible union, or `None` when clean.
    #[must_use]
    pub const fn pending(&self) -> Option<Rect> {
        self.pending
    }
}

/// Failure during a partial redraw. Pending invalidation is retained on errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefreshError<D, S> {
    /// Dirty pixels need at least one scratch pixel.
    EmptyScratch,
    /// Display dimensions differ from those used to track invalidation.
    DimensionsChanged,
    /// Scene renderer rejected the draw before the current tile was written.
    Scene(S),
    /// Display write failed; previously written tiles cannot be rolled back.
    Display(D),
}

/// A borrowed one-row draw target positioned in global scene coordinates.
///
/// The bounding box is the current clip. Use separately supplied display bounds
/// for layout. Pixels outside this tile are ignored, including negative points.
#[derive(Debug)]
pub struct Tile<'a> {
    area: Rect,
    pixels: &'a mut [Packed],
}

impl Dimensions for Tile<'_> {
    fn bounding_box(&self) -> Rectangle {
        Rectangle::new(
            Point::new(i32::from(self.area.x), i32::from(self.area.y)),
            Size::new(u32::from(self.area.width), 1),
        )
    }
}

impl Tile<'_> {
    fn intersection(&self, area: Rectangle) -> Option<Rect> {
        let clipped = clip(
            area,
            DisplaySize::new(self.area.x + self.area.width, self.area.y + 1),
        )?;
        let left = clipped.x.max(self.area.x);
        let right = clipped.x + clipped.width;
        if left >= right || clipped.y > self.area.y || clipped.y + clipped.height <= self.area.y {
            return None;
        }
        Some(Rect::new(left, self.area.y, right - left, 1))
    }
}

impl DrawTarget for Tile<'_> {
    type Color = Rgb565;
    type Error = Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Rgb565>>,
    {
        let bounds = DisplaySize::new(self.area.x + self.area.width, self.area.y + 1);
        for Pixel(point, color) in pixels {
            if let Some((x, y)) = visible(point, bounds)
                && x >= self.area.x
                && y == self.area.y
            {
                self.pixels[usize::from(x - self.area.x)] = Packed(color.into_storage());
            }
        }
        Ok(())
    }

    fn fill_contiguous<I>(&mut self, area: &Rectangle, colors: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Rgb565>,
    {
        if self.intersection(*area).is_some() {
            self.draw_iter(contiguous_pixels(*area, colors))?;
        }
        Ok(())
    }

    fn fill_solid(&mut self, area: &Rectangle, color: Rgb565) -> Result<(), Self::Error> {
        if let Some(clipped) = self.intersection(*area) {
            let start = usize::from(clipped.x - self.area.x);
            self.pixels[start..start + usize::from(clipped.width)]
                .fill(Packed(color.into_storage()));
        }
        Ok(())
    }
}

/// Repaints the pending region with caller-owned bounded scratch storage.
///
/// Scratch may be narrower than the dirty region. Each tile is initialized to
/// `background`, rendered, then synchronously written through `Display`.
/// `scene` must render a stable snapshot: it can be called once per horizontal
/// tile per row. There is no call when clean, and no full-screen allocation.
/// Dirty state clears only after every tile succeeds. Recovery/reinitialization
/// after transport failure belongs to the caller before retrying the whole area.
/// # Errors
/// Returns `EmptyScratch`, `DimensionsChanged`, or the original scene/display
/// error, preserving the entire pending region for a later retry.
pub fn repaint<D, S, F>(
    display: &mut D,
    dirty: &mut DirtyRegion,
    scratch: &mut [Packed],
    background: Rgb565,
    scene: F,
) -> Result<(), RefreshError<D::Error, S>>
where
    D: Display,
    F: FnMut(&mut Tile<'_>) -> Result<(), S>,
{
    match repaint_cooperative(display, dirty, scratch, background, scene, || {
        Ok::<(), Infallible>(())
    }) {
        Ok(()) => Ok(()),
        Err(CooperativeError::Refresh(error)) => Err(error),
        Err(CooperativeError::Service(never)) => match never {},
    }
}

/// Rendering or caller service failed; the complete original dirty region remains pending.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CooperativeError<D, S, Y> {
    /// Geometry, scratch, scene or display error.
    Refresh(RefreshError<D, S>),
    /// The service callback failed after the last successful tile write.
    Service(Y),
}

/// Repaints a stable scene and services other work after each successful tile.
///
/// The callback runs without a live tile/scratch borrow, including after the
/// final write, and may pump a cooperative executor or a polling time driver.
/// No callback runs for clean/invalid input or after a failed scene/display
/// operation. This is synchronous: callers bound scratch size, scene execution,
/// display latency and service work to meet their own scheduling deadlines.
/// A failure retains the whole dirty area, including already transmitted tiles.
/// # Errors
/// Returns the original refresh error or service failure, before another tile.
pub fn repaint_cooperative<D, S, Y, F, P>(
    display: &mut D,
    dirty: &mut DirtyRegion,
    scratch: &mut [Packed],
    background: Rgb565,
    mut scene: F,
    mut service: P,
) -> Result<(), CooperativeError<D::Error, S, Y>>
where
    D: Display,
    F: FnMut(&mut Tile<'_>) -> Result<(), S>,
    P: FnMut() -> Result<(), Y>,
{
    let Some(area) = dirty.pending else {
        return Ok(());
    };
    if display.dimensions() != dirty.dimensions {
        return Err(CooperativeError::Refresh(RefreshError::DimensionsChanged));
    }
    if scratch.is_empty() {
        return Err(CooperativeError::Refresh(RefreshError::EmptyScratch));
    }
    // Oversized scratch is valid: only the first display-width pixels are used.
    let capacity = u16::try_from(scratch.len()).unwrap_or(u16::MAX);
    for y in area.y..area.y + area.height {
        let mut x = area.x;
        let end = area.x + area.width;
        while x < end {
            let width = (end - x).min(capacity);
            let pixels = &mut scratch[..usize::from(width)];
            pixels.fill(Packed(background.into_storage()));
            let tile_area = Rect::new(x, y, width, 1);
            scene(&mut Tile {
                area: tile_area,
                pixels,
            })
            .map_err(|error| CooperativeError::Refresh(RefreshError::Scene(error)))?;
            display
                .write(tile_area, pixels)
                .map_err(|error| CooperativeError::Refresh(RefreshError::Display(error)))?;
            service().map_err(CooperativeError::Service)?;
            x += width;
        }
    }
    dirty.pending = None;
    Ok(())
}

/// Failure while comparing two stable scenes; no display writes are performed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DamageError<S, Y> {
    /// Two complete display rows are required.
    ShortScratch,
    /// A scene failed before this row could be compared.
    Scene(S),
    /// Cooperative service failed after a completed comparison row.
    Service(Y),
}

/// Finds the exact bounding rectangle of changed RGB565 pixels using two rows.
///
/// Both scenes must remain stable for the complete scan. Service runs after each
/// compared row, including the last. Identical or empty scenes return `None`.
/// Scratch beyond two display rows is unused. No display is accessed.
/// # Errors
/// Rejects insufficient scratch before invoking either scene; propagates scene
/// and service failures without reporting a partial damage rectangle.
pub fn damage_cooperative<S, Y>(
    dimensions: DisplaySize,
    scratch: &mut [Packed],
    background: Rgb565,
    mut previous: impl FnMut(&mut Tile<'_>) -> Result<(), S>,
    mut next: impl FnMut(&mut Tile<'_>) -> Result<(), S>,
    mut service: impl FnMut() -> Result<(), Y>,
) -> Result<Option<Rect>, DamageError<S, Y>> {
    if dimensions.width == 0 || dimensions.height == 0 {
        return Ok(None);
    }
    let width = usize::from(dimensions.width);
    let required = width.checked_mul(2).ok_or(DamageError::ShortScratch)?;
    let scratch = scratch
        .get_mut(..required)
        .ok_or(DamageError::ShortScratch)?;
    let (old, new) = scratch.split_at_mut(width);
    let mut dirty = DirtyRegion::new(dimensions);
    for y in 0..dimensions.height {
        old.fill(Packed(background.into_storage()));
        new.fill(Packed(background.into_storage()));
        let area = Rect::new(0, y, dimensions.width, 1);
        previous(&mut Tile { area, pixels: old }).map_err(DamageError::Scene)?;
        next(&mut Tile { area, pixels: new }).map_err(DamageError::Scene)?;
        let first = old.iter().zip(new.iter()).position(|(a, b)| a != b);
        if let Some(first) = first {
            let last = old
                .iter()
                .zip(new.iter())
                .rposition(|(a, b)| a != b)
                .unwrap_or(first);
            // Both indices originate from a row whose width is a u16.
            let left = u16::try_from(first).map_err(|_| DamageError::ShortScratch)?;
            let count = u16::try_from(last - first + 1).map_err(|_| DamageError::ShortScratch)?;
            dirty.invalidate(Rectangle::new(
                Point::new(i32::from(left), i32::from(y)),
                Size::new(u32::from(count), 1),
            ));
        }
        service().map_err(DamageError::Service)?;
    }
    Ok(dirty.pending())
}
