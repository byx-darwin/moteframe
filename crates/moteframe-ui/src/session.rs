// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

use crate::{
    ConfigError, CooperativeError, DirtyRegion, ResourceView, Tile, UiContext, UiSettings, UiState,
    UpdateImpact, repaint_cooperative,
};
#[cfg(feature = "frame-pacing")]
use crate::{FrameInterval, PacingState};
use core::num::NonZeroU16;
use embedded_graphics::{
    pixelcolor::PixelColor,
    prelude::{Point, Size},
    primitives::Rectangle,
};
use moteframe_core::{Display, Rect};

/// Requested tile width limit; the actual scratch slice remains caller-owned.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TilePixels {
    /// Uses at most 32 pixels, bounded by the actual supplied capacity.
    #[default]
    Auto,
    /// Uses this nonzero maximum, rejecting undersized workspaces.
    Fixed(NonZeroU16),
}
/// How a pending request selects a logical region; not a panel commit protocol.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RedrawMode {
    /// Draws the union of pending visible rectangles.
    #[default]
    Dirty,
    /// Promotes any pending request to the full display; clean remains idle.
    FullOnRequest,
}
/// Candidate synchronous refresh policy, separate from appearance.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RefreshSettings {
    tile_pixels: TilePixels,
    redraw_mode: RedrawMode,
    #[cfg(feature = "frame-pacing")]
    frame_interval: FrameInterval,
}
impl RefreshSettings {
    /// Automatic tiles and dirty-region refresh.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            tile_pixels: TilePixels::Auto,
            redraw_mode: RedrawMode::Dirty,
            #[cfg(feature = "frame-pacing")]
            frame_interval: FrameInterval::Unlimited,
        }
    }
    /// Selects a candidate tile limit, validated against the actual workspace.
    #[must_use]
    pub const fn with_tile_pixels(mut self, tile_pixels: TilePixels) -> Self {
        self.tile_pixels = tile_pixels;
        self
    }
    /// Selects a candidate redraw policy.
    #[must_use]
    pub const fn with_redraw_mode(mut self, redraw_mode: RedrawMode) -> Self {
        self.redraw_mode = redraw_mode;
        self
    }
    /// Configured tile capacity policy.
    #[must_use]
    pub const fn tile_pixels(self) -> TilePixels {
        self.tile_pixels
    }
    /// Configured region policy.
    #[must_use]
    pub const fn redraw_mode(self) -> RedrawMode {
        self.redraw_mode
    }
    /// Sets a frame-start interval; only enable/disable transitions clear anchors.
    #[cfg(feature = "frame-pacing")]
    #[must_use]
    pub const fn with_frame_interval(mut self, interval: FrameInterval) -> Self {
        self.frame_interval = interval;
        self
    }
    /// Effective frame-start interval.
    #[cfg(feature = "frame-pacing")]
    #[must_use]
    pub const fn frame_interval(self) -> FrameInterval {
        self.frame_interval
    }
    fn capacity(self, available: usize) -> Result<usize, ConfigError> {
        if available == 0 {
            return Err(ConfigError::EmptyWorkspace);
        }
        let limit = match self.tile_pixels {
            TilePixels::Auto => available.min(32),
            TilePixels::Fixed(n) => usize::from(n.get()),
        };
        if limit > available {
            return Err(ConfigError::WorkspaceTooSmall);
        }
        Ok(limit)
    }
}

/// Device and caller-maintained generation token for one synchronous operation.
/// Change generation on device replacement, reset, rotation or reinitialization,
/// even at the same size. Keep device dimensions/command state stable per frame.
#[derive(Debug)]
pub struct DisplayBinding<'a, D> {
    display: &'a mut D,
    generation: u64,
}
impl<'a, D: Display> DisplayBinding<'a, D> {
    /// Borrows an already initialized synchronous display.
    pub const fn new(display: &'a mut D, generation: u64) -> Self {
        Self {
            display,
            generation,
        }
    }
}
impl<'a, D> DisplayBinding<'a, D> {
    fn into_parts(self) -> (&'a mut D, u64) {
        (self.display, self.generation)
    }
}
/// Logical device binding status; recovery requires an explicit valid rebind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BindingState {
    /// No successful initial device/environment validation yet.
    Unbound,
    /// Accepted generation token.
    Bound(u64),
    /// An observed device geometry/generation mismatch blocks further drawing.
    NeedsRebind,
}
/// Result of one bounded frame attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DrawOutcome {
    /// Clean; no scene/write/service or clock sample.
    Idle,
    /// This frame completed; future requests may still be pending.
    Drawn,
    /// Interval has not elapsed; no scene, write or service was called.
    #[cfg(feature = "frame-pacing")]
    Deferred {
        /// Remaining microseconds until the next eligible start.
        remaining_us: u64,
    },
}
/// Validation/recovery or rendering failure. Pending logical work is retained.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SessionError<D, S, Y> {
    /// Invalid resources/configuration before rendering.
    Config(ConfigError),
    /// Observed device changes require an explicit successful rebind.
    NeedsRebind,
    /// Clock moved backwards; this attempt did not render and began a new epoch.
    #[cfg(feature = "frame-pacing")]
    ClockRegressed,
    /// A paced policy needs `draw_timed`; ordinary `draw` cannot bypass it.
    #[cfg(feature = "frame-pacing")]
    ClockRequired,
    /// Original rendering, synchronous transport or service error.
    Refresh(CooperativeError<D, S, Y>),
}

/// Limited sink for requests arriving after the current frame snapshot.
/// It cannot mutate the session, effective settings or current-frame request.
#[derive(Debug)]
pub struct DeferredInvalidation {
    dirty: DirtyRegion,
}
impl DeferredInvalidation {
    /// Records only the visible portion, using a constant-memory rectangle union.
    pub fn invalidate(&mut self, area: Rectangle) {
        self.dirty.invalidate(area);
    }
    /// Records a future full-screen request.
    pub fn invalidate_all(&mut self) {
        self.dirty.invalidate_all();
    }
}

/// Exclusive state/workspace borrow with transactional updates and recovery.
/// Drop the session before sharing scratch; a new session conservatively starts
/// with full invalidation. No permanent device or resource borrow is retained.
#[derive(Debug)]
pub struct RefreshSession<'a, C> {
    ui: &'a mut UiState<C>,
    scratch: &'a mut [C],
    policy: RefreshSettings,
    dirty: DirtyRegion,
    binding: BindingState,
    #[cfg(feature = "frame-pacing")]
    pacing: PacingState,
}
impl<'a, C: Copy + PartialEq> RefreshSession<'a, C> {
    /// Binds actual nonempty workspace and validates policy, starting unbound/full.
    /// # Errors
    /// Returns `EmptyWorkspace` or `WorkspaceTooSmall` immediately.
    pub fn new(
        ui: &'a mut UiState<C>,
        scratch: &'a mut [C],
        policy: RefreshSettings,
    ) -> Result<Self, ConfigError> {
        policy.capacity(scratch.len())?;
        let mut dirty = DirtyRegion::new(ui.dimensions());
        dirty.invalidate_all();
        Ok(Self {
            ui,
            scratch,
            policy,
            dirty,
            binding: BindingState::Unbound,
            #[cfg(feature = "frame-pacing")]
            pacing: PacingState::default(),
        })
    }
    /// Current device binding state.
    #[must_use]
    pub const fn binding(&self) -> BindingState {
        self.binding
    }
    /// Current clock anchors; absent from builds without frame pacing.
    #[cfg(feature = "frame-pacing")]
    #[must_use]
    pub const fn pacing(&self) -> PacingState {
        self.pacing
    }
    /// Current request union; failed frames retain the entire attempted region.
    #[must_use]
    pub const fn pending(&self) -> Option<Rect> {
        self.dirty.pending()
    }
    /// Effective appearance, without mutable access.
    #[must_use]
    pub const fn settings(&self) -> &UiSettings<C> {
        self.ui.settings()
    }
    /// Effective refresh policy.
    #[must_use]
    pub const fn policy(&self) -> RefreshSettings {
        self.policy
    }
    /// Records visible damage in logical coordinates at a frame boundary.
    pub fn invalidate(&mut self, area: Rectangle) {
        self.dirty.invalidate(area);
    }
    /// Requests a complete logical redraw.
    pub fn invalidate_all(&mut self) {
        self.dirty.invalidate_all();
    }
    /// Validates settings/resources/workspace as one transaction.
    /// # Errors
    /// On error, settings, policy, dirty and scheduling anchors are unchanged.
    pub fn try_update<R: ?Sized>(
        &mut self,
        candidate: UiSettings<C>,
        policy: RefreshSettings,
        resources: ResourceView<'_, R>,
    ) -> Result<UpdateImpact, ConfigError> {
        policy.capacity(self.scratch.len())?;
        let mut impact = self.ui.try_update(candidate, resources)?;
        if impact == UpdateImpact::Unchanged && policy != self.policy {
            impact = UpdateImpact::PolicyOnly;
        }
        #[cfg(feature = "frame-pacing")]
        self.pacing
            .transition(self.policy.frame_interval, policy.frame_interval);
        self.policy = policy;
        if impact == UpdateImpact::FullRedraw {
            self.dirty.invalidate_all();
        }
        Ok(impact)
    }
    fn observe<D: Display<Color = C>>(&mut self, binding: &DisplayBinding<'_, D>) -> bool {
        let mismatch = binding.display.dimensions() != self.ui.dimensions()
            || match self.binding {
                BindingState::Bound(generation) => generation != binding.generation,
                BindingState::Unbound => false,
                BindingState::NeedsRebind => true,
            };
        if mismatch {
            self.binding = BindingState::NeedsRebind;
        }
        mismatch
    }
    /// Replaces geometry/binding only after validating the whole candidate.
    /// # Errors
    /// Retains logical work on error. An observed external mismatch stays latched;
    /// invalid candidates alone do not invalidate an otherwise unchanged binding.
    pub fn try_rebind<R: ?Sized, D: Display<Color = C>>(
        &mut self,
        candidate: UiSettings<C>,
        policy: RefreshSettings,
        binding: DisplayBinding<'_, D>,
        resources: ResourceView<'_, R>,
    ) -> Result<UpdateImpact, ConfigError> {
        self.observe(&binding);
        let (display, generation) = binding.into_parts();
        policy.capacity(self.scratch.len())?;
        self.ui
            .try_rebind(candidate, display.dimensions(), resources)?;
        #[cfg(feature = "frame-pacing")]
        self.pacing
            .transition(self.policy.frame_interval, policy.frame_interval);
        self.policy = policy;
        self.binding = BindingState::Bound(generation);
        self.dirty = DirtyRegion::new(self.ui.dimensions());
        self.dirty.invalidate_all();
        Ok(UpdateImpact::FullRedraw)
    }
    /// Validates the environment and renders at most one stable frame.
    ///
    /// The scene must keep its own inputs stable across tiles. Service receives
    /// only a next-frame request sink, including after the last successful write.
    /// For enabled minimum intervals use `draw_timed` to supply the clock.
    /// This signature stays unchanged when other dependency paths enable pacing.
    /// Requests are merged after success or failure, never consumed in this call.
    /// # Errors
    /// Returns original errors; all current and future work survives failure.
    pub fn draw<D, R: ?Sized, S, Y>(
        &mut self,
        binding: DisplayBinding<'_, D>,
        resources: ResourceView<'_, R>,
        scene: impl FnMut(&mut Tile<'_, C>, &UiContext<'_, C, R>) -> Result<(), S>,
        service: impl FnMut(&mut DeferredInvalidation) -> Result<(), Y>,
    ) -> Result<DrawOutcome, SessionError<D::Error, S, Y>>
    where
        D: Display<Color = C>,
        C: PixelColor,
    {
        self.draw_inner(
            binding,
            resources,
            #[cfg(feature = "frame-pacing")]
            None::<fn() -> u64>,
            scene,
            service,
        )
    }
    /// Runs one frame with an infallible microsecond clock, sampled only when
    /// a validated dirty request uses a minimum interval. The clock is bounded
    /// by the caller; no timer or executor is retained by the session.
    /// # Errors
    /// In addition to frame errors, reports clock regression with dirty retained.
    #[cfg(feature = "frame-pacing")]
    pub fn draw_timed<D, R: ?Sized, S, Y>(
        &mut self,
        binding: DisplayBinding<'_, D>,
        resources: ResourceView<'_, R>,
        clock: impl FnMut() -> u64,
        scene: impl FnMut(&mut Tile<'_, C>, &UiContext<'_, C, R>) -> Result<(), S>,
        service: impl FnMut(&mut DeferredInvalidation) -> Result<(), Y>,
    ) -> Result<DrawOutcome, SessionError<D::Error, S, Y>>
    where
        D: Display<Color = C>,
        C: PixelColor,
    {
        self.draw_inner(binding, resources, Some(clock), scene, service)
    }
    fn draw_inner<D, R: ?Sized, S, Y>(
        &mut self,
        binding: DisplayBinding<'_, D>,
        resources: ResourceView<'_, R>,
        #[cfg(feature = "frame-pacing")] clock: Option<impl FnMut() -> u64>,
        mut scene: impl FnMut(&mut Tile<'_, C>, &UiContext<'_, C, R>) -> Result<(), S>,
        mut service: impl FnMut(&mut DeferredInvalidation) -> Result<(), Y>,
    ) -> Result<DrawOutcome, SessionError<D::Error, S, Y>>
    where
        D: Display<Color = C>,
        C: PixelColor,
    {
        if self.observe(&binding) {
            return Err(SessionError::NeedsRebind);
        }
        let (display, generation) = binding.into_parts();
        self.policy
            .capacity(self.scratch.len())
            .map_err(SessionError::Config)?;
        let context = self.ui.context(resources).map_err(SessionError::Config)?;
        self.binding = BindingState::Bound(generation);
        if self.dirty.pending().is_none() {
            return Ok(DrawOutcome::Idle);
        }
        #[cfg(feature = "frame-pacing")]
        if matches!(
            self.policy.frame_interval,
            FrameInterval::MinimumInterval(_)
        ) {
            let clock = clock.ok_or(SessionError::ClockRequired)?;
            if let Some(remaining_us) = self
                .pacing
                .start(self.policy.frame_interval, clock)
                .map_err(|()| SessionError::ClockRegressed)?
            {
                return Ok(DrawOutcome::Deferred { remaining_us });
            }
        }
        if self.policy.redraw_mode == RedrawMode::FullOnRequest {
            self.dirty.invalidate_all();
        }
        let mut future = DeferredInvalidation {
            dirty: DirtyRegion::new(self.ui.dimensions()),
        };
        let capacity = self
            .policy
            .capacity(self.scratch.len())
            .map_err(SessionError::Config)?;
        let result = repaint_cooperative(
            display,
            &mut self.dirty,
            &mut self.scratch[..capacity],
            context.settings().palette().background(),
            |tile| scene(tile, &context),
            || service(&mut future),
        );
        if let Some(rect) = future.dirty.pending() {
            self.dirty.invalidate(Rectangle::new(
                Point::new(i32::from(rect.x), i32::from(rect.y)),
                Size::new(u32::from(rect.width), u32::from(rect.height)),
            ));
        }
        result.map_err(SessionError::Refresh)?;
        Ok(DrawOutcome::Drawn)
    }
}
