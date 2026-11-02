// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Atomic settings, resource metadata and layout defaults.
use core::num::NonZeroU32;
use moteframe_core::{Dimensions, Rect};
use moteframe_ui::{
    ConfigError, Palette, ResourceKey, ResourceMetadata, ResourceView, UiSettings, UiState,
    UpdateImpact,
};

fn settings() -> UiSettings<u8> {
    UiSettings::new(Palette::new(0, 1, 2, 3, 4))
}
fn empty() -> ResourceView<'static, ()> {
    ResourceView::empty(&())
}

#[test]
fn atomic_settings_and_resource_revision() -> Result<(), ConfigError> {
    let mut ui = UiState::new(settings(), Dimensions::new(12, 8), empty())?;
    assert_eq!(ui.try_update(settings(), empty())?, UpdateImpact::Unchanged);
    let revised = settings().with_palette(Palette::new(5, 1, 2, 3, 4));
    assert_eq!(ui.try_update(revised, empty())?, UpdateImpact::FullRedraw);
    let id = NonZeroU32::new(3).ok_or(ConfigError::InvalidResources)?;
    let key = ResourceKey::new(id, 1);
    let candidate = revised.with_resources(key);
    assert_eq!(
        ui.try_update(candidate, empty()),
        Err(ConfigError::ResourceMismatch)
    );
    assert_eq!(ui.settings().palette().background(), 5);
    assert_eq!(ui.settings().resources(), ResourceKey::EMPTY);
    let data = [9_u8];
    let meta = ResourceMetadata::new(4, 6, 1)?;
    let view = ResourceView::new(key, meta, &data)?;
    assert_eq!(ui.try_update(candidate, view)?, UpdateImpact::FullRedraw);
    let ctx = ui.context(view)?;
    assert_eq!(ctx.display_bounds(), Rect::new(0, 0, 12, 8));
    assert_eq!(ctx.resources(), &[9]);
    assert_eq!(
        ui.context(empty()).err(),
        Some(ConfigError::ResourceMismatch)
    );
    let v2 = ResourceView::new(ResourceKey::new(id, 2), meta, &data)?;
    assert_eq!(
        ui.try_update(candidate.with_resources(v2.key()), v2)?,
        UpdateImpact::FullRedraw
    );
    Ok(())
}

#[test]
fn rejects_invalid_geometry_and_metadata() -> Result<(), ConfigError> {
    for dims in [Dimensions::new(0, 4), Dimensions::new(4, 0)] {
        assert_eq!(
            UiState::new(settings(), dims, empty()).err(),
            Some(ConfigError::InvalidDimensions)
        );
    }
    assert_eq!(
        ResourceMetadata::new(65536, 1, 1),
        Err(ConfigError::InvalidResources)
    );
    assert_eq!(
        ResourceMetadata::new(0, 1, 1),
        Err(ConfigError::InvalidResources)
    );
    assert_eq!(
        ResourceMetadata::new(1, 1, 0),
        Err(ConfigError::InvalidResources)
    );
    assert_eq!(
        ResourceView::new(ResourceKey::EMPTY, ResourceMetadata::new(1, 1, 1)?, &()).err(),
        Some(ConfigError::InvalidResources)
    );
    let mut ui = UiState::new(settings(), Dimensions::new(12, 8), empty())?;
    assert_eq!(
        ui.try_rebind(settings(), Dimensions::new(0, 2), empty()),
        Err(ConfigError::InvalidDimensions)
    );
    assert_eq!(ui.dimensions(), Dimensions::new(12, 8));
    assert_eq!(
        ui.try_rebind(settings(), Dimensions::new(3, 2), empty())?,
        UpdateImpact::FullRedraw
    );
    Ok(())
}

#[cfg(feature = "layout")]
#[test]
fn default_layout_uses_padding_and_gap_explicit_stack_can_override() -> Result<(), ConfigError> {
    use moteframe_ui::{Axis, LayoutSettings, Padding, Stack};
    let s = settings().with_layout(
        LayoutSettings::new()
            .with_gap(2)
            .with_padding(Padding::new(1, 2, 3, 1)),
    );
    let mut ui = UiState::new(s, Dimensions::new(12, 8), empty())?;
    let context = ui.context(empty())?;
    assert_eq!(context.content_bounds(), Rect::new(1, 2, 8, 5));
    let mut stack = context
        .stack(Axis::Horizontal)
        .map_err(|_| ConfigError::InvalidPadding)?;
    assert_eq!(stack.take(2), Ok(Rect::new(1, 2, 2, 5)));
    assert_eq!(stack.take(2), Ok(Rect::new(5, 2, 2, 5)));
    let mut explicit = Stack::new(context.display_bounds(), Axis::Horizontal, 0)
        .map_err(|_| ConfigError::InvalidPadding)?;
    assert_eq!(explicit.take(2), Ok(Rect::new(0, 0, 2, 8)));
    let invalid =
        s.with_layout(LayoutSettings::new().with_padding(Padding::new(u16::MAX, 0, 1, 0)));
    assert_eq!(
        ui.try_update(invalid, empty()),
        Err(ConfigError::InvalidPadding)
    );
    assert_eq!(*ui.settings(), s);
    Ok(())
}

#[test]
fn non_reflexive_palette_is_rejected() {
    assert_eq!(
        UiState::new(
            UiSettings::new(Palette::new(f32::NAN, 1.0, 2.0, 3.0, 4.0)),
            Dimensions::new(1, 1),
            empty()
        )
        .err(),
        Some(ConfigError::InvalidPalette)
    );
}
