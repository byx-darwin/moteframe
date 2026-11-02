// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! A standalone firmware consumer with compile-time 32/8-pixel RAM profiles.
//!
//! The application owns its real workspace. The same [`UI_PIXELS`] constant is
//! the stream run capacity; no library defaults or global caches are changed.
//! This library builds for a bare-metal target without a board or allocator.
#![no_std]
#![forbid(unsafe_code)]

use core::convert::Infallible;
use moteframe_core::{Dimensions, Display, Rect};
use moteframe_ui::{
    self as ui,
    graphics::{geometry::Dimensions as _, pixelcolor::Rgb565, prelude::*},
};

/// Actual scratch-array length and stream run capacity, selected by this app.
pub const UI_PIXELS: usize = if cfg!(feature = "ui-low-memory") {
    8
} else {
    32
};

/// Caller-owned UI state and the actual RGB565 workspace; no full framebuffer.
#[derive(Debug)]
pub struct App {
    state: ui::UiState<Rgb565>,
    scratch: [Rgb565; UI_PIXELS],
}

impl App {
    /// Creates the example appearance and an actual capacity-selected array.
    ///
    /// # Errors
    /// Returns the original UI configuration error for invalid geometry.
    pub fn new(dimensions: Dimensions) -> Result<Self, ui::ConfigError> {
        let palette = ui::Palette::new(
            Rgb565::BLACK,
            Rgb565::WHITE,
            Rgb565::RED,
            Rgb565::GREEN,
            Rgb565::BLUE,
        );
        let state = ui::UiState::new(ui::UiSettings::new(palette), dimensions, resources())?;
        Ok(Self {
            state,
            scratch: [palette.background(); UI_PIXELS],
        })
    }

    /// Borrows the real array, permitting a caller to inspect its byte budget.
    #[must_use]
    pub const fn workspace(&self) -> &[Rgb565; UI_PIXELS] {
        &self.scratch
    }

    /// Borrows state and scratch for a persistent session with explicit recovery.
    ///
    /// Keep the session across frames for idle/deferred requests and retry. A
    /// fresh session starts with full invalidation. `Auto` uses actual capacity;
    /// `Fixed` must fit it. Dropping the session releases both borrows.
    ///
    /// # Errors
    /// Returns the original configuration error without changing the app.
    pub fn session(
        &mut self,
        policy: ui::RefreshSettings,
    ) -> Result<ui::RefreshSession<'_, Rgb565>, ui::ConfigError> {
        ui::RefreshSession::new(&mut self.state, &mut self.scratch, policy)
    }
}

/// Empty resource view shared by the example's state and frame calls.
#[must_use]
pub fn resources() -> ui::ResourceView<'static, ()> {
    ui::ResourceView::empty(&())
}

fn color(seed: u32, x: u16, y: u16) -> Rgb565 {
    Rgb565::new(
        ((u32::from(x) ^ seed) & 31) as u8,
        (seed.wrapping_add(u32::from(y) * 3) & 63) as u8,
        (seed.wrapping_add(u32::from(x) + u32::from(y)) & 31) as u8,
    )
}

/// Draws the deterministic example scene using absolute tile coordinates.
///
/// # Errors
/// Infallible: the tile accepts clipped, typed-color pixels in caller scratch.
pub fn draw_scene(
    seed: u32,
    tile: &mut ui::Tile<'_, Rgb565>,
    _: &ui::UiContext<'_, Rgb565, ()>,
) -> Result<(), Infallible> {
    let bounds = tile.bounding_box();
    tile.draw_iter(
        bounds
            .points()
            .map(|point| Pixel(point, color(seed, point.x as u16, point.y as u16))),
    )
}

/// Streams the first logical row using the same capacity as the real workspace.
///
/// # Errors
/// Returns the first raw display error; uncertain writes are never retried.
pub fn stream_row<D: Display<Color = Rgb565>>(display: &mut D, seed: u32) -> Result<(), D::Error> {
    let width = display.dimensions().width;
    ui::StreamTarget::<_, UI_PIXELS>::with_run(display)
        .draw_iter((0..width).map(|x| Pixel(Point::new(i32::from(x), 0), color(seed, x, 0))))
}

/// Distinct stages of [`render_once`], retaining every original error cause.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RenderError<D, Y> {
    /// State/session construction failed before any drawing.
    Config(ui::ConfigError),
    /// The immediate first-row stream failed; no hidden retry or refresh.
    Stream(D),
    /// Original refresh/session/display/service failure with its typed cause.
    Session(ui::SessionError<D, Infallible, Y>),
}

/// One-shot failure with the full logical request available for explicit retry.
///
/// Configuration rejection has no accepted session/request. Stream and session
/// failures retain the session's complete pending region, including any future
/// invalidations merged after a failed refresh. No error is converted or retried.
#[derive(Debug, PartialEq, Eq)]
pub struct RenderFailure<D, Y> {
    /// Original stage and typed cause.
    pub error: RenderError<D, Y>,
    /// Complete unconsumed logical request; `None` for configuration rejection.
    pub pending: Option<Rect>,
}

/// Result of one-shot drawing, including work posted for a future frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameReport {
    /// Result of this frame attempt.
    pub outcome: ui::DrawOutcome,
    /// Unconsumed future request; the caller must arrange a subsequent frame.
    pub pending: Option<Rect>,
}

/// Validates, streams one row, then attempts exactly one full example frame.
///
/// This convenience entry point demonstrates complete firmware assembly. Use
/// [`App::session`] directly when a session must survive failures or frames.
/// The service callback runs after each successful refresh write, and can post
/// future requests. This one-shot call reports unconsumed future work in
/// [`FrameReport::pending`]; it does not retain a session or draw a second frame.
///
/// # Errors
/// Distinguishes configuration, immediate stream, and session errors without
/// replacing display or service causes. [`RenderFailure::pending`] reports the
/// full logical request on stream/session failure. No stage retries on failure.
pub fn render_once<D, Y>(
    display: &mut D,
    seed: u32,
    policy: ui::RefreshSettings,
    service: impl FnMut(&mut ui::DeferredInvalidation) -> Result<(), Y>,
) -> Result<FrameReport, RenderFailure<D::Error, Y>>
where
    D: Display<Color = Rgb565>,
{
    let mut app = App::new(display.dimensions()).map_err(|error| RenderFailure {
        error: RenderError::Config(error),
        pending: None,
    })?;
    let mut session = app.session(policy).map_err(|error| RenderFailure {
        error: RenderError::Config(error),
        pending: None,
    })?;
    stream_row(display, seed).map_err(|error| RenderFailure {
        error: RenderError::Stream(error),
        pending: session.pending(),
    })?;
    let outcome = session
        .draw(
            ui::DisplayBinding::new(display, 1),
            resources(),
            |tile, context| draw_scene(seed, tile, context),
            service,
        )
        .map_err(|error| RenderFailure {
            error: RenderError::Session(error),
            pending: session.pending(),
        })?;
    Ok(FrameReport {
        outcome,
        pending: session.pending(),
    })
}
