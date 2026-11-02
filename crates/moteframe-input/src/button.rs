// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

use crate::{EventQueue, InputError, check_time};

/// Configurable normalized-button policy, in monotonic microseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ButtonConfig {
    debounce_us: u32,
    hold_us: Option<u32>,
}
impl ButtonConfig {
    /// Zero debounce is immediate; None disables hold events.
    /// # Errors
    /// A configured zero hold threshold is ambiguous and rejected.
    pub const fn new(debounce_us: u32, hold_us: Option<u32>) -> Result<Self, InputError> {
        if matches!(hold_us, Some(0)) {
            return Err(InputError::InvalidConfig);
        }
        Ok(Self {
            debounce_us,
            hold_us,
        })
    }
}
/// One debounced normalized-button transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ButtonEvent {
    /// Button became stably pressed.
    Pressed,
    /// Button became stably released, including after a hold.
    Released,
    /// Once per press after configured duration from debounced Pressed.
    Held,
}
#[derive(Clone, Copy, Debug)]
struct State {
    candidate: bool,
    stable: bool,
    changed: u64,
    pressed: u64,
    held: bool,
    last: Option<u64>,
}
/// One initially released button. No hardware handle or event buffer is owned.
#[derive(Debug)]
pub struct Button {
    config: ButtonConfig,
    state: State,
}
impl Button {
    /// Starts a new boot epoch in released state.
    #[must_use]
    pub const fn new(config: ButtonConfig) -> Self {
        Self {
            config,
            state: State {
                candidate: false,
                stable: false,
                changed: 0,
                pressed: 0,
                held: false,
                last: None,
            },
        }
    }
    /// Samples normalized pressed state and publishes at most one event.
    ///
    /// Time equality is allowed. Queue Full rolls back the attempted transition,
    /// including held/debounce state; caller retains/retries this sample first.
    /// Continue sampling while held to drive the hold timer. A debounced release
    /// takes priority over a hold becoming due in the same call; no hold is
    /// backfilled after release. There is no background timer.
    /// # Errors
    /// Returns `ClockRegression` or `Full` without changing state or existing events.
    pub fn sample<const N: usize>(
        &mut self,
        pressed: bool,
        now_us: u64,
        queue: &mut EventQueue<ButtonEvent, N>,
    ) -> Result<(), InputError> {
        check_time(self.state.last, now_us)?;
        let mut next = self.state;
        let mut event = None;
        if pressed != next.candidate {
            next.candidate = pressed;
            next.changed = now_us;
        }
        if pressed != next.stable && now_us - next.changed >= u64::from(self.config.debounce_us) {
            next.stable = pressed;
            if pressed {
                next.pressed = now_us;
                next.held = false;
                event = Some(ButtonEvent::Pressed);
            } else {
                event = Some(ButtonEvent::Released);
            }
        } else if next.stable
            && !next.held
            && self
                .config
                .hold_us
                .is_some_and(|hold| now_us - next.pressed >= u64::from(hold))
        {
            next.held = true;
            event = Some(ButtonEvent::Held);
        }
        if let Some(event) = event {
            queue.push(event)?;
        }
        next.last = Some(now_us);
        self.state = next;
        Ok(())
    }
}
