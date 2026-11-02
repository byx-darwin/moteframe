// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Portable normalized input. Timestamps are monotonic microseconds in one boot
//! epoch, not UTC. Adapters own hardware sampling, coordinate mapping and ISR
//! synchronization. Retry a rejected sample before advancing the input stream.
#![no_std]
#![forbid(unsafe_code)]

mod button;
mod queue;
mod touch;
pub use button::{Button, ButtonConfig, ButtonEvent};
pub use queue::EventQueue;
pub use touch::{Direction, Point, Touch, TouchConfig, TouchEvent, TouchSample};

/// Rejected input operation. State and queued events remain unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum InputError {
    /// Invalid dimensions or thresholds.
    InvalidConfig,
    /// Time moved backward within this instance's boot epoch.
    ClockRegression,
    /// Queue cannot accept the complete event; retain and retry this sample.
    Full,
    /// Point is outside configured logical bounds.
    OutOfBounds,
    /// Down while active, or Move/Up without a contact.
    Sequence,
}

fn check_time(last: Option<u64>, now: u64) -> Result<(), InputError> {
    if last.is_some_and(|last| now < last) {
        Err(InputError::ClockRegression)
    } else {
        Ok(())
    }
}
