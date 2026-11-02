// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

use crate::{EventQueue, InputError, check_time};

/// Logical coordinate supplied after the adapter's rotation/mirroring.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    /// Horizontal logical pixel.
    pub x: u16,
    /// Vertical logical pixel.
    pub y: u16,
}
/// Normalized single-contact sample; no vendor gesture/register values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TouchSample {
    /// Begins a new contact.
    Down(Point),
    /// Updates the active contact.
    Move(Point),
    /// Ends the active contact at the supplied coordinate.
    Up(Point),
    /// Explicitly cancels an active contact; idle cancellation is a no-op.
    Cancel,
}
/// Dominant swipe axis; equal displacement chooses the horizontal axis.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Direction {
    /// Decreasing x.
    Left,
    /// Increasing x.
    Right,
    /// Decreasing y.
    Up,
    /// Increasing y.
    Down,
}
/// Recognized generic gesture; page actions belong to the application.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum TouchEvent {
    /// Released within slop before hold threshold; coordinate is the initial point.
    Tap(Point),
    /// Released beyond the configured swipe threshold on the dominant axis.
    Swipe(Direction),
    /// Once while stationary within slop; coordinate is the initial point.
    Held(Point),
    /// Active contact explicitly cancelled.
    Cancelled,
}
/// Logical bounds and caller-selected thresholds; no screen or chip defaults.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TouchConfig {
    width: u16,
    height: u16,
    tap_slop: u16,
    swipe: u16,
    hold_us: u32,
}
impl TouchConfig {
    /// Creates a policy; threshold equality fires at the exact deadline/distance.
    /// # Errors
    /// Rejects empty bounds, zero hold or swipe threshold no greater than slop.
    pub const fn new(
        width: u16,
        height: u16,
        tap_slop: u16,
        swipe: u16,
        hold_us: u32,
    ) -> Result<Self, InputError> {
        if width == 0 || height == 0 || hold_us == 0 || swipe <= tap_slop {
            return Err(InputError::InvalidConfig);
        }
        Ok(Self {
            width,
            height,
            tap_slop,
            swipe,
            hold_us,
        })
    }
    fn validate(self, point: Point) -> Result<(), InputError> {
        if point.x >= self.width || point.y >= self.height {
            Err(InputError::OutOfBounds)
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, Copy, Debug)]
struct Contact {
    start: Point,
    position: Point,
    started: u64,
    travel: u16,
    held: bool,
}
impl Contact {
    fn moved(&mut self, point: Point) {
        self.position = point;
        self.travel = self
            .travel
            .max(self.start.x.abs_diff(point.x))
            .max(self.start.y.abs_diff(point.y));
    }
    fn hold(&mut self, now: u64, config: TouchConfig) -> Option<TouchEvent> {
        if !self.held
            && self.travel <= config.tap_slop
            && now - self.started >= u64::from(config.hold_us)
        {
            self.held = true;
            Some(TouchEvent::Held(self.start))
        } else {
            None
        }
    }
    fn finish(mut self, now: u64, config: TouchConfig) -> Option<TouchEvent> {
        if self.held {
            return None;
        }
        let dx = self.start.x.abs_diff(self.position.x);
        let dy = self.start.y.abs_diff(self.position.y);
        if dx.max(dy) >= config.swipe {
            let dir = if dx >= dy {
                if self.position.x >= self.start.x {
                    Direction::Right
                } else {
                    Direction::Left
                }
            } else if self.position.y >= self.start.y {
                Direction::Down
            } else {
                Direction::Up
            };
            Some(TouchEvent::Swipe(dir))
        } else if self.travel <= config.tap_slop {
            self.hold(now, config).or(Some(TouchEvent::Tap(self.start)))
        } else {
            None
        }
    }
}
/// Single-contact recognizer, independent of buses, UI, and event storage.
#[derive(Debug)]
pub struct Touch {
    config: TouchConfig,
    contact: Option<Contact>,
    last: Option<u64>,
}
impl Touch {
    /// Creates an idle recognizer in a new monotonic boot epoch.
    #[must_use]
    pub const fn new(config: TouchConfig) -> Self {
        Self {
            config,
            contact: None,
            last: None,
        }
    }
    /// Applies one normalized sample; publishes at most one complete event.
    /// # Errors
    /// Invalid time, sequence, position or a full queue leaves all state intact.
    pub fn update<const N: usize>(
        &mut self,
        sample: TouchSample,
        now_us: u64,
        queue: &mut EventQueue<TouchEvent, N>,
    ) -> Result<(), InputError> {
        check_time(self.last, now_us)?;
        let mut next = self.contact;
        let event = match sample {
            TouchSample::Down(point) => {
                self.config.validate(point)?;
                if next.is_some() {
                    return Err(InputError::Sequence);
                }
                next = Some(Contact {
                    start: point,
                    position: point,
                    started: now_us,
                    travel: 0,
                    held: false,
                });
                None
            }
            TouchSample::Move(point) => {
                self.config.validate(point)?;
                let contact = next.as_mut().ok_or(InputError::Sequence)?;
                contact.moved(point);
                None
            }
            TouchSample::Up(point) => {
                self.config.validate(point)?;
                let mut contact = next.take().ok_or(InputError::Sequence)?;
                contact.moved(point);
                contact.finish(now_us, self.config)
            }
            TouchSample::Cancel => next.take().map(|_| TouchEvent::Cancelled),
        };
        if let Some(event) = event {
            queue.push(event)?;
        }
        self.contact = next;
        self.last = Some(now_us);
        Ok(())
    }
    /// Advances stationary hold timing without inventing a hardware sample.
    /// # Errors
    /// `ClockRegression` or `Full` leaves contact/hold state and existing events intact.
    pub fn tick<const N: usize>(
        &mut self,
        now_us: u64,
        queue: &mut EventQueue<TouchEvent, N>,
    ) -> Result<(), InputError> {
        check_time(self.last, now_us)?;
        let mut next = self.contact;
        if let Some(event) = next.as_mut().and_then(|c| c.hold(now_us, self.config)) {
            queue.push(event)?;
        }
        self.contact = next;
        self.last = Some(now_us);
        Ok(())
    }
}
