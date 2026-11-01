// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

use moteframe_core::{Dimensions, Rect};

/// Direction in which a stack allocates rectangular slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Slots advance from left to right and span the parent's height.
    Horizontal,
    /// Slots advance from top to bottom and span the parent's width.
    Vertical,
}

/// A layout request that cannot be represented or does not fit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayoutError {
    /// Parent is empty or its exclusive endpoint exceeds the coordinate range.
    InvalidBounds,
    /// Requested slot has zero extent.
    ZeroExtent,
    /// Requested slot and its preceding gap do not fit.
    NoSpace,
}

/// Checked fixed-extent stack layout with no widget or allocation policy.
#[derive(Clone, Copy, Debug)]
pub struct Stack {
    parent: Rect,
    axis: Axis,
    gap: u16,
    cursor: u32,
}

impl Stack {
    /// Creates an empty stack inside a nonempty, representable parent.
    /// # Errors
    /// Returns `InvalidBounds` for empty or overflowing parents.
    pub fn new(parent: Rect, axis: Axis, gap: u16) -> Result<Self, LayoutError> {
        if !parent.fits(Dimensions::new(u16::MAX, u16::MAX)) {
            return Err(LayoutError::InvalidBounds);
        }
        Ok(Self {
            parent,
            axis,
            gap,
            cursor: 0,
        })
    }

    /// Allocates a slot, adding the configured gap before subsequent slots.
    ///
    /// Failed requests leave the cursor unchanged; trailing gaps need not fit.
    /// # Errors
    /// Returns `ZeroExtent` or `NoSpace` when the request cannot fit.
    pub fn take(&mut self, extent: u16) -> Result<Rect, LayoutError> {
        if extent == 0 {
            return Err(LayoutError::ZeroExtent);
        }
        let available = match self.axis {
            Axis::Horizontal => self.parent.width,
            Axis::Vertical => self.parent.height,
        };
        if self.cursor + u32::from(extent) > u32::from(available) {
            return Err(LayoutError::NoSpace);
        }
        let offset = u16::try_from(self.cursor).map_err(|_| LayoutError::NoSpace)?;
        let slot = match self.axis {
            Axis::Horizontal => Rect::new(
                self.parent.x + offset,
                self.parent.y,
                extent,
                self.parent.height,
            ),
            Axis::Vertical => Rect::new(
                self.parent.x,
                self.parent.y + offset,
                self.parent.width,
                extent,
            ),
        };
        self.cursor += u32::from(extent) + u32::from(self.gap);
        Ok(slot)
    }
}
