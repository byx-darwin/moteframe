// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

use core::num::NonZeroU32;
/// Frame-start interval in microseconds, independent of any timer or executor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum FrameInterval {
    /// No clock callbacks or retained timestamps.
    #[default]
    Unlimited,
    /// Minimum distance between actual starts, including failed attempts.
    MinimumInterval(NonZeroU32),
}
/// Observable scheduling anchors. Zero is a valid timestamp; regressions start
/// a new clock epoch without waiting for the old epoch to catch up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PacingState {
    last_start: Option<u64>,
    last_observed: Option<u64>,
}
impl PacingState {
    /// Most recent frame start in this epoch, even when rendering failed.
    #[must_use]
    pub const fn last_start(self) -> Option<u64> {
        self.last_start
    }
    /// Most recent sampled time, including deferred attempts and regression.
    #[must_use]
    pub const fn last_observed(self) -> Option<u64> {
        self.last_observed
    }
    pub(crate) fn transition(&mut self, old: FrameInterval, new: FrameInterval) {
        if matches!(old, FrameInterval::Unlimited) != matches!(new, FrameInterval::Unlimited) {
            *self = Self::default();
        }
    }
    pub(crate) fn start(
        &mut self,
        interval: FrameInterval,
        mut clock: impl FnMut() -> u64,
    ) -> Result<Option<u64>, ()> {
        let FrameInterval::MinimumInterval(interval) = interval else {
            return Ok(None);
        };
        let now = clock();
        if self.last_observed.is_some_and(|last| now < last) {
            self.last_observed = Some(now);
            self.last_start = None;
            return Err(());
        }
        self.last_observed = Some(now);
        if let Some(last) = self.last_start {
            let elapsed = now - last;
            let limit = u64::from(interval.get());
            if elapsed < limit {
                return Ok(Some(limit - elapsed));
            }
        }
        self.last_start = Some(now);
        Ok(None)
    }
}
