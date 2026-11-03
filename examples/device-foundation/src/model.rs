// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Explicit synthetic ports for offline replay and measurement, not devices.
use crate::{Link, VALUE_BYTES, services::ReceiveError};
use core::fmt;
use moteframe_ble::{NotificationBackend, WriteBufferError, WriteSlot};

/// Complete fixture route; a bare reused handle is insufficient.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    /// Nonreused connection generation.
    pub generation: u32,
    /// Characteristic/database identity.
    pub characteristic: u16,
    /// Applicable ATT bearer.
    pub bearer: u8,
}
/// Synthetic, distinguishable original failures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Definitely no queue admission.
    Full,
    /// Submission result deliberately unknown to caller.
    Uncertain,
    /// Poll failed without a verified route.
    Poll,
    /// Identity/CCCD no longer current.
    Stale,
    /// Current capacity changed or cannot contain value.
    Capacity,
    /// Synthetic route identity cannot be reused after exhaustion.
    GenerationExhausted,
    /// Synthetic exact-read failure.
    Read,
    /// Synthetic logical display write failure.
    Display,
}
/// Single-route model, with bounded committed-write storage and observed counters.
pub struct LinkModel {
    /// Actual currently configured route identity.
    pub target: Target,
    /// Whether a route is connected and subscribed.
    enabled: bool,
    /// Per-target capacity, checked again at submission.
    pub capacity: usize,
    /// Number of upcoming definitely-full submissions.
    pub blocks: u32,
    /// Next submission returns uncertainty; bool is hidden acceptance outcome.
    pub uncertain: Option<bool>,
    /// Number of upcoming failing polls.
    pub poll_faults: u32,
    /// Change capacity to one at the next submission.
    pub shrink_on_submit: bool,
    /// Observed poll count.
    pub polls: u32,
    /// Observed actual submission calls.
    pub submissions: u32,
    /// Observed backend acceptances, not peer acknowledgements.
    pub accepted: u32,
    /// Rolling trace of actually accepted bytes, for offline comparison.
    pub fingerprint: u32,
    incoming: WriteSlot<Target, VALUE_BYTES>,
    last: [u8; VALUE_BYTES],
    length: usize,
}
impl fmt::Debug for LinkModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LinkModel")
            .field("polls", &self.polls)
            .field("accepted", &self.accepted)
            .finish_non_exhaustive()
    }
}
impl Default for LinkModel {
    fn default() -> Self {
        Self::new()
    }
}
impl LinkModel {
    /// New explicitly synthetic connected/subscribed route.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            target: Target {
                generation: 1,
                characteristic: 1,
                bearer: 0,
            },
            enabled: true,
            capacity: VALUE_BYTES,
            blocks: 0,
            uncertain: None,
            poll_faults: 0,
            shrink_on_submit: false,
            polls: 0,
            submissions: 0,
            accepted: 0,
            fingerprint: 0,
            incoming: WriteSlot::new(),
            last: [0; VALUE_BYTES],
            length: 0,
        }
    }
    /// Inject a complete committed write without vendor or radio calls.
    /// # Errors
    /// Slot Full/TooLong leaves the old value intact.
    pub fn inject(&mut self, target: Target, value: &[u8]) -> Result<(), WriteBufferError> {
        self.incoming.push(target, value)
    }
    /// Observe actual last accepted bytes in the fixture only; Debug never prints them.
    #[must_use]
    pub fn last_value(&self) -> &[u8] {
        &self.last[..self.length]
    }
    /// Observe whether a synthetic route is currently subscribed.
    #[must_use]
    pub const fn enabled(&self) -> bool {
        self.enabled
    }
    /// Retire subscription data immediately, including transitions between polls.
    /// Re-enabling allocates a new route generation; it never relabels old values.
    /// # Errors
    /// Generation exhaustion keeps the route disabled rather than wrapping.
    pub fn set_enabled(&mut self, enabled: bool) -> Result<(), Error> {
        if self.enabled == enabled {
            return Ok(());
        }
        if enabled {
            self.target.generation = self
                .target
                .generation
                .checked_add(1)
                .ok_or(Error::GenerationExhausted)?;
        }
        self.incoming.clear();
        self.enabled = enabled;
        Ok(())
    }
    fn valid(&self, target: Target) -> bool {
        self.enabled && target == self.target
    }
    fn accept(&mut self, value: &[u8]) {
        self.last[..value.len()].copy_from_slice(value);
        self.length = value.len();
        self.accepted = self.accepted.saturating_add(1);
        for byte in value {
            self.fingerprint = self
                .fingerprint
                .wrapping_mul(31)
                .wrapping_add(u32::from(*byte));
        }
    }
}
impl NotificationBackend for LinkModel {
    type Target = Target;
    type Error = Error;
    fn notification_capacity(&self, target: Target) -> Result<usize, Error> {
        if self.valid(target) {
            Ok(self.capacity)
        } else {
            Err(Error::Stale)
        }
    }
    fn try_queue_notification(&mut self, target: Target, value: &[u8]) -> Result<(), Error> {
        self.submissions = self.submissions.saturating_add(1);
        if self.shrink_on_submit {
            self.shrink_on_submit = false;
            self.capacity = 1;
        }
        if !self.valid(target) {
            return Err(Error::Stale);
        }
        if value.len() > self.capacity {
            return Err(Error::Capacity);
        }
        if self.blocks > 0 {
            self.blocks -= 1;
            return Err(Error::Full);
        }
        if let Some(accepted) = self.uncertain.take() {
            if accepted {
                self.accept(value);
            }
            return Err(Error::Uncertain);
        }
        self.accept(value);
        Ok(())
    }
}
impl Link for LinkModel {
    fn poll(&mut self) -> Result<Option<Target>, Error> {
        self.polls = self.polls.saturating_add(1);
        if self.poll_faults > 0 {
            self.poll_faults -= 1;
            return Err(Error::Poll);
        }
        Ok(self.enabled.then_some(self.target))
    }
    fn receive(
        &mut self,
        slot: &mut WriteSlot<Target, VALUE_BYTES>,
    ) -> Result<(), ReceiveError<Error>> {
        if let Some((meta, bytes)) = self.incoming.peek() {
            slot.push(meta.target, bytes).map_err(ReceiveError::Slot)?;
            self.incoming.clear();
        }
        Ok(())
    }
    fn definitely_not_queued(&self, error: &Error) -> bool {
        !matches!(error, Error::Uncertain)
    }
}

/// Fixed original synthetic collection bytes, never a vendor/product resource.
pub const RESOURCE_BYTES: [u8; 35] = [
    0xa0, 0xb0, 0xc0, 0xd0, 1, 0, 1, 0, 1, 0, 1, 1, 1, 1, 0, 1, 1, 0, 1, 0xf8, 0, 0x07, 0xe0, 0,
    0x1f, 0xff, 0xff, 0, 0, 0xff, 0xe0, 0x07, 0xff, 0xf8, 0x1f,
];
/// Bounded read-only source with observable address/read behavior and fault injection.
#[derive(Debug)]
pub struct SourceModel {
    /// Exact-read call count.
    pub reads: u32,
    /// Smallest physical read offset.
    pub lowest: u32,
    /// Fail this numbered read once, before returning bytes.
    pub fail_at: Option<u32>,
    /// Deliberately invalidate the first mono pixel.
    pub corrupt: bool,
}
impl Default for SourceModel {
    fn default() -> Self {
        Self::new()
    }
}
impl SourceModel {
    /// Create the immutable synthetic collection, with no initial read.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            reads: 0,
            lowest: u32::MAX,
            fail_at: None,
            corrupt: false,
        }
    }
}
impl moteframe_core::ReadAt for SourceModel {
    type Error = Error;
    fn capacity(&self) -> u32 {
        35
    }
    fn read_exact(&mut self, offset: u32, out: &mut [u8]) -> Result<(), Error> {
        self.reads = self.reads.saturating_add(1);
        self.lowest = self.lowest.min(offset);
        if self.fail_at == Some(self.reads) {
            return Err(Error::Read);
        }
        let start = usize::try_from(offset).map_err(|_| Error::Read)?;
        let end = start.checked_add(out.len()).ok_or(Error::Read)?;
        let src = RESOURCE_BYTES.get(start..end).ok_or(Error::Read)?;
        out.copy_from_slice(src);
        if self.corrupt && offset == 4 && !out.is_empty() {
            out[0] = 2;
        }
        Ok(())
    }
}
/// Framebuffer-free synthetic display. Models logical writes, never optical output.
#[derive(Debug)]
pub struct Sink {
    /// Current observed geometry.
    pub size: moteframe_core::Dimensions,
    /// Attempted writes.
    pub writes: u32,
    /// Successfully written pixels.
    pub pixels: u32,
    /// Rolling accepted pixel trace; tile ordering affects this value.
    pub fingerprint: u32,
    /// Fail this numbered write, before accepting pixels.
    pub fail_at: Option<u32>,
}
impl Default for Sink {
    fn default() -> Self {
        Self::new()
    }
}
impl Sink {
    /// Start a clean, fixed-geometry logical sink.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            size: moteframe_core::Dimensions::new(32, 16),
            writes: 0,
            pixels: 0,
            fingerprint: 0,
            fail_at: None,
        }
    }
}
impl moteframe_core::Display for Sink {
    type Color = moteframe_ui::graphics::pixelcolor::Rgb565;
    type Error = Error;
    fn dimensions(&self) -> moteframe_core::Dimensions {
        self.size
    }
    fn write(&mut self, area: moteframe_core::Rect, pixels: &[Self::Color]) -> Result<(), Error> {
        use moteframe_ui::graphics::prelude::IntoStorage;
        self.writes = self.writes.saturating_add(1);
        if self.fail_at == Some(self.writes) {
            return Err(Error::Display);
        }
        if area.height != 1
            || usize::from(area.width) != pixels.len()
            || u32::from(area.x) + u32::from(area.width) > u32::from(self.size.width)
            || area.y >= self.size.height
        {
            return Err(Error::Display);
        }
        for p in pixels {
            self.fingerprint = self
                .fingerprint
                .wrapping_mul(31)
                .wrapping_add(u32::from(p.into_storage()));
            self.pixels = self.pixels.saturating_add(1);
        }
        Ok(())
    }
}
