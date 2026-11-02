// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

use crate::{ResourceKey, ResourceView};
use moteframe_core::{Dimensions, Rect};

/// Validation failure. Failed updates retain all effective configuration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConfigError {
    /// Logical width and height must be nonzero.
    InvalidDimensions,
    /// Palette equality must have stable, reflexive value semantics.
    InvalidPalette,
    /// Padding leaves no content area or overflows.
    InvalidPadding,
    /// Directory summary or identity is invalid.
    InvalidResources,
    /// This frame/candidate requires a different resource collection.
    ResourceMismatch,
    /// Sessions require at least one actual scratch pixel.
    EmptyWorkspace,
    /// Fixed tile size exceeds the actual scratch capacity.
    WorkspaceTooSmall,
}

/// Effect of an atomic update; applying settings is not drawing completion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum UpdateImpact {
    /// Effective values are unchanged; previous pending work remains pending.
    Unchanged,
    /// Only refresh scheduling/chunk policy changed; no new draw request.
    PolicyOnly,
    /// Appearance, resource identity or display binding requires a full redraw.
    FullRedraw,
}

/// Explicit color roles, with no encoding or `Default` requirement on `C`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette<C> {
    background: C,
    foreground: C,
    accent: C,
    muted: C,
    border: C,
}
impl<C: Copy> Palette<C> {
    /// Defines the five roles. Explicit drawing colors always take precedence.
    #[must_use]
    pub const fn new(background: C, foreground: C, accent: C, muted: C, border: C) -> Self {
        Self {
            background,
            foreground,
            accent,
            muted,
            border,
        }
    }
    /// Tile initialization color used by refresh sessions.
    #[must_use]
    pub const fn background(&self) -> C {
        self.background
    }
    /// Default foreground supplied to scenes.
    #[must_use]
    pub const fn foreground(&self) -> C {
        self.foreground
    }
    /// Default emphasis supplied to scenes.
    #[must_use]
    pub const fn accent(&self) -> C {
        self.accent
    }
    /// Default secondary color supplied to scenes.
    #[must_use]
    pub const fn muted(&self) -> C {
        self.muted
    }
    /// Default boundary color supplied to scenes.
    #[must_use]
    pub const fn border(&self) -> C {
        self.border
    }
}

/// Candidate style. Effective settings are read-only through `UiState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiSettings<C> {
    palette: Palette<C>,
    resources: ResourceKey,
    #[cfg(feature = "layout")]
    layout: LayoutSettings,
}
impl<C: Copy> UiSettings<C> {
    /// Starts with explicit colors, empty resources and optional zero-gap layout.
    #[must_use]
    pub const fn new(palette: Palette<C>) -> Self {
        Self {
            palette,
            resources: ResourceKey::EMPTY,
            #[cfg(feature = "layout")]
            layout: LayoutSettings::new(),
        }
    }
    /// Replaces candidate colors. Validation happens when applying the candidate.
    #[must_use]
    pub const fn with_palette(mut self, palette: Palette<C>) -> Self {
        self.palette = palette;
        self
    }
    /// Selects a resource collection to validate on application.
    #[must_use]
    pub const fn with_resources(mut self, resources: ResourceKey) -> Self {
        self.resources = resources;
        self
    }
    /// Explicit role colors.
    #[must_use]
    pub const fn palette(&self) -> &Palette<C> {
        &self.palette
    }
    /// Required resource identity.
    #[must_use]
    pub const fn resources(&self) -> ResourceKey {
        self.resources
    }
    /// Sets candidate layout defaults, without changing explicit drawing geometry.
    #[cfg(feature = "layout")]
    #[must_use]
    pub const fn with_layout(mut self, layout: LayoutSettings) -> Self {
        self.layout = layout;
        self
    }
    /// Layout defaults consumed by the context helper.
    #[cfg(feature = "layout")]
    #[must_use]
    pub const fn layout(&self) -> LayoutSettings {
        self.layout
    }
}

/// Effective settings and logical geometry; no device, resource or scratch borrow.
#[derive(Clone, Copy, Debug)]
pub struct UiState<C> {
    settings: UiSettings<C>,
    dimensions: Dimensions,
}
impl<C: Copy + PartialEq> UiState<C> {
    /// Validates an initial candidate and its immediately available resource metadata.
    /// # Errors
    /// Returns an explicit geometry, palette, layout or resource error.
    pub fn new<R: ?Sized>(
        settings: UiSettings<C>,
        dimensions: Dimensions,
        resources: ResourceView<'_, R>,
    ) -> Result<Self, ConfigError> {
        Self::validate(&settings, dimensions, resources)?;
        Ok(Self {
            settings,
            dimensions,
        })
    }
    pub(crate) fn validate<R: ?Sized>(
        settings: &UiSettings<C>,
        dimensions: Dimensions,
        resources: ResourceView<'_, R>,
    ) -> Result<(), ConfigError> {
        if dimensions.width == 0 || dimensions.height == 0 {
            return Err(ConfigError::InvalidDimensions);
        }
        // Reject NaN-like colors; the caller additionally guarantees stable Eq semantics.
        let p = settings.palette;
        if [p.background, p.foreground, p.accent, p.muted, p.border]
            .iter()
            .any(|value| value.ne(value))
        {
            return Err(ConfigError::InvalidPalette);
        }
        #[cfg(feature = "layout")]
        settings.layout.padding.content(dimensions)?;
        if settings.resources != resources.key() {
            return Err(ConfigError::ResourceMismatch);
        }
        Ok(())
    }
    /// Effective settings, not a mutable update bypass.
    #[must_use]
    pub const fn settings(&self) -> &UiSettings<C> {
        &self.settings
    }
    /// Logical screen dimensions.
    #[must_use]
    pub const fn dimensions(&self) -> Dimensions {
        self.dimensions
    }
    /// Applies a whole candidate only after validation succeeds.
    /// # Errors
    /// Leaves state unchanged on validation errors.
    pub fn try_update<R: ?Sized>(
        &mut self,
        candidate: UiSettings<C>,
        resources: ResourceView<'_, R>,
    ) -> Result<UpdateImpact, ConfigError> {
        Self::validate(&candidate, self.dimensions, resources)?;
        let impact = if self.settings == candidate {
            UpdateImpact::Unchanged
        } else {
            UpdateImpact::FullRedraw
        };
        self.settings = candidate;
        Ok(impact)
    }
    /// Validates and replaces logical geometry; successful rebind always redraws.
    /// # Errors
    /// Leaves state unchanged on validation errors.
    pub fn try_rebind<R: ?Sized>(
        &mut self,
        candidate: UiSettings<C>,
        dimensions: Dimensions,
        resources: ResourceView<'_, R>,
    ) -> Result<UpdateImpact, ConfigError> {
        Self::validate(&candidate, dimensions, resources)?;
        self.settings = candidate;
        self.dimensions = dimensions;
        Ok(UpdateImpact::FullRedraw)
    }
    /// Borrows one stable settings/resource snapshot for a frame.
    /// # Errors
    /// Rejects resource identity mismatches before scene execution.
    pub fn context<'a, R: ?Sized>(
        &'a self,
        resources: ResourceView<'a, R>,
    ) -> Result<UiContext<'a, C, R>, ConfigError> {
        if self.settings.resources != resources.key() {
            return Err(ConfigError::ResourceMismatch);
        }
        Ok(UiContext {
            settings: &self.settings,
            dimensions: self.dimensions,
            resources,
        })
    }
}

/// Stable whole-frame geometry and borrowed defaults. Tile bounds are clips only.
#[derive(Debug)]
pub struct UiContext<'a, C, R: ?Sized> {
    settings: &'a UiSettings<C>,
    dimensions: Dimensions,
    resources: ResourceView<'a, R>,
}
impl<C: Copy, R: ?Sized> UiContext<'_, C, R> {
    /// Effective style for this frame.
    #[must_use]
    pub const fn settings(&self) -> &UiSettings<C> {
        self.settings
    }
    /// Entire logical screen, unchanged across horizontal/vertical tile iteration.
    #[must_use]
    pub const fn display_bounds(&self) -> Rect {
        Rect::new(0, 0, self.dimensions.width, self.dimensions.height)
    }
    /// The borrowed provider. Resource reads may fail through the scene's error type.
    #[must_use]
    pub const fn resources(&self) -> &R {
        self.resources.provider()
    }
    /// Validated resource summary and identity for this frame.
    #[must_use]
    pub const fn resource_view(&self) -> ResourceView<'_, R> {
        self.resources
    }
    /// Stable screen area after subtracting validated default padding.
    #[cfg(feature = "layout")]
    #[must_use]
    pub fn content_bounds(&self) -> Rect {
        self.settings.layout.padding.bounds(self.dimensions)
    }
    /// Allocates using default content bounds and gap; use `Stack::new` to override.
    /// # Errors
    /// Propagates layout geometry errors.
    #[cfg(feature = "layout")]
    pub fn stack(&self, axis: crate::Axis) -> Result<crate::Stack, crate::LayoutError> {
        crate::Stack::new(self.content_bounds(), axis, self.settings.layout.gap)
    }
}

/// Four logical inset distances, in pixels; no implicit coordinate transform.
#[cfg(feature = "layout")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Padding {
    left: u16,
    top: u16,
    right: u16,
    bottom: u16,
}
#[cfg(feature = "layout")]
impl Padding {
    /// Inset distances in left, top, right, bottom order.
    #[must_use]
    pub const fn new(left: u16, top: u16, right: u16, bottom: u16) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }
    /// Insets in left, top, right, bottom order.
    #[must_use]
    pub const fn insets(self) -> [u16; 4] {
        [self.left, self.top, self.right, self.bottom]
    }
    fn content(self, d: Dimensions) -> Result<Rect, ConfigError> {
        if u32::from(self.left) + u32::from(self.right) >= u32::from(d.width)
            || u32::from(self.top) + u32::from(self.bottom) >= u32::from(d.height)
        {
            return Err(ConfigError::InvalidPadding);
        }
        Ok(self.bounds(d))
    }
    fn bounds(self, d: Dimensions) -> Rect {
        Rect::new(
            self.left,
            self.top,
            d.width - self.left - self.right,
            d.height - self.top - self.bottom,
        )
    }
}

/// Layout defaults, present only when `layout` is enabled.
#[cfg(feature = "layout")]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LayoutSettings {
    gap: u16,
    padding: Padding,
}
#[cfg(feature = "layout")]
impl LayoutSettings {
    /// Zero gap and padding.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            gap: 0,
            padding: Padding::new(0, 0, 0, 0),
        }
    }
    /// Default spacing between stack items.
    #[must_use]
    pub const fn with_gap(mut self, gap: u16) -> Self {
        self.gap = gap;
        self
    }
    /// Candidate content insets, checked when applying to dimensions.
    #[must_use]
    pub const fn with_padding(mut self, padding: Padding) -> Self {
        self.padding = padding;
        self
    }
    /// Default item gap.
    #[must_use]
    pub const fn gap(self) -> u16 {
        self.gap
    }
    /// Default content insets.
    #[must_use]
    pub const fn padding(self) -> Padding {
        self.padding
    }
}
