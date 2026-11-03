// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Example assembly with independent service progress and stable frames.
use crate::{
    Catalog, Link, Services, UI_PIXELS,
    scene::{self, RenderScratch, SceneError},
    services::ServiceReport,
};
use core::{convert::Infallible, fmt};
use moteframe_core::{Display, ReadAt, Rect};
use moteframe_ui::{
    ConfigError, DisplayBinding, DrawOutcome, Palette, RefreshSession, RefreshSettings,
    SessionError, UiSettings, UiState, UpdateImpact,
    graphics::pixelcolor::{Rgb565, RgbColor},
};
/// UI state and real selected transfer array, owned outside the borrowed session.
pub struct UiMemory {
    state: UiState<Rgb565>,
    pixels: [Rgb565; UI_PIXELS],
}
impl UiMemory {
    /// Validate initial fixed geometry and immutable resource identity.
    /// # Errors
    /// Propagates the original UI metadata/configuration error.
    pub fn new(catalog: &Catalog) -> Result<Self, ConfigError> {
        let settings = UiSettings::new(Palette::new(
            Rgb565::BLACK,
            Rgb565::WHITE,
            Rgb565::BLUE,
            Rgb565::GREEN,
            Rgb565::WHITE,
        ))
        .with_resources(catalog.key());
        Ok(Self {
            state: UiState::new(settings, scene::SIZE, catalog.view()?)?,
            pixels: [Rgb565::BLACK; UI_PIXELS],
        })
    }
}
/// One outer attempt with independent first service errors and the original UI result.
#[derive(Debug)]
pub struct StepReport<B, D, R> {
    /// All attempted service lanes and first failures.
    pub services: ServiceReport<B>,
    /// At most one frame; partial writes are not rolled back.
    pub draw: Result<DrawOutcome, SessionError<D, SceneError<R>, Infallible>>,
}
/// Long-lived consumer. Does not retain the display or reader.
pub struct Demo<'a, B: Link> {
    session: RefreshSession<'a, Rgb565>,
    services: Services<B>,
    catalog: Catalog,
    scratch: RenderScratch,
}
impl<B: Link> fmt::Debug for Demo<'_, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Demo")
            .field("pending", &self.pending())
            .field("services", &self.services)
            .finish_non_exhaustive()
    }
}
impl<'a, B: Link> Demo<'a, B> {
    /// Borrow actual UI storage and own the independent services and descriptors.
    /// # Errors
    /// Invalid resources/workspace cannot create a partly configured consumer.
    pub fn new(
        memory: &'a mut UiMemory,
        services: Services<B>,
        catalog: Catalog,
        policy: RefreshSettings,
    ) -> Result<Self, ConfigError> {
        if memory.state.settings().resources() != catalog.key() {
            return Err(ConfigError::ResourceMismatch);
        }
        Ok(Self {
            session: RefreshSession::new(&mut memory.state, &mut memory.pixels, policy)?,
            services,
            catalog,
            scratch: RenderScratch::new(),
        })
    }
    /// Inspect the independent service owner.
    #[must_use]
    pub const fn services(&self) -> &Services<B> {
        &self.services
    }
    /// Supply/recover input and adapter work at a frame boundary.
    pub const fn services_mut(&mut self) -> &mut Services<B> {
        &mut self.services
    }
    /// Current logical redraw union.
    #[must_use]
    pub const fn pending(&self) -> Option<Rect> {
        self.session.pending()
    }
    /// Effective resource revision, never inferred from storage bytes.
    #[must_use]
    pub const fn resource_revision(&self) -> u32 {
        self.catalog.key().revision()
    }
    /// Explicitly identify repaired/replaced resource content at a frame boundary.
    /// Revisions must increase during this consumer's lifetime; failed updates are atomic.
    /// # Errors
    /// Stale revision or invalid candidate leaves current settings/catalog unchanged.
    pub fn update_resources(&mut self, revision: u32) -> Result<UpdateImpact, ConfigError> {
        if revision <= self.resource_revision() {
            return Err(ConfigError::ResourceMismatch);
        }
        let next = Catalog::new(revision)?;
        let impact = self.session.try_update(
            self.session.settings().with_resources(next.key()),
            self.session.policy(),
            next.view()?,
        )?;
        self.catalog = next;
        Ok(impact)
    }
    /// Explicitly bind a reinitialized display. The example scene stays 32x16.
    /// # Errors
    /// Wrong geometry or configuration cannot silently change scene coordinates.
    pub fn rebind<D: Display<Color = Rgb565>>(
        &mut self,
        display: &mut D,
        generation: u64,
    ) -> Result<UpdateImpact, ConfigError> {
        if display.dimensions() != scene::SIZE {
            return Err(ConfigError::InvalidDimensions);
        }
        self.session.try_rebind(
            *self.session.settings(),
            self.session.policy(),
            DisplayBinding::new(display, generation),
            self.catalog.view()?,
        )
    }
    /// One pre-round, at most one stable frame with tile rounds, then one post-round.
    /// Calls are synchronous and adapters must bound their own execution time.
    /// UI Idle, pacing, binding and scene/display errors never suppress outer rounds.
    pub fn step<D: Display<Color = Rgb565>, R: ReadAt + ?Sized>(
        &mut self,
        display: &mut D,
        generation: u64,
        reader: &mut R,
        now_us: u64,
    ) -> StepReport<B::Error, D::Error, R::Error> {
        let mut report = ServiceReport::default();
        self.services.round(&mut report);
        if let Some(area) = scene::damage_rect(self.services.take_damage()) {
            self.session.invalidate(area);
        }
        let snapshot = self.services.snapshot();
        let services = &mut self.services;
        let scratch = &mut self.scratch;
        let draw = match self.catalog.view() {
            Err(e) => Err(SessionError::Config(e)),
            Ok(view) => self.session.draw_timed(
                DisplayBinding::new(display, generation),
                view,
                || now_us,
                |tile, _| scene::render(tile, snapshot, &self.catalog, reader, scratch),
                |future| {
                    services.round(&mut report);
                    if let Some(area) = scene::damage_rect(services.take_damage()) {
                        future.invalidate(area);
                    }
                    Ok::<(), Infallible>(())
                },
            ),
        };
        self.services.round(&mut report);
        if let Some(area) = scene::damage_rect(self.services.take_damage()) {
            self.session.invalidate(area);
        }
        StepReport {
            services: report,
            draw,
        }
    }
}

impl fmt::Debug for UiMemory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UiMemory")
            .field("pixel_capacity", &UI_PIXELS)
            .finish_non_exhaustive()
    }
}
