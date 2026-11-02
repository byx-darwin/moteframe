// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

use crate::InputError;
use core::fmt;

/// Fixed-capacity FIFO with no overwrite or implicit allocation.
/// Not a synchronization primitive; adapters coordinate cross-context access.
pub struct EventQueue<T: Copy, const N: usize> {
    slots: [Option<T>; N],
    read: usize,
    len: usize,
}
impl<T: Copy, const N: usize> fmt::Debug for EventQueue<T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventQueue")
            .field("capacity", &N)
            .field("length", &self.len)
            .finish_non_exhaustive()
    }
}
impl<T: Copy, const N: usize> Default for EventQueue<T, N> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T: Copy, const N: usize> EventQueue<T, N> {
    /// Creates N real slots, including a valid always-full zero-slot queue.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: [None; N],
            read: 0,
            len: 0,
        }
    }
    /// Actual number of retained events.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }
    /// Whether no event is retained.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
    /// Actual inline slot capacity, not a scheduling budget.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        N
    }
    /// Publishes one whole event.
    /// # Errors
    /// Full leaves every queued event and cursor unchanged.
    pub fn push(&mut self, event: T) -> Result<(), InputError> {
        if self.len == N {
            return Err(InputError::Full);
        }
        self.slots[(self.read + self.len) % N] = Some(event);
        self.len += 1;
        Ok(())
    }
    /// Removes the oldest event. Logical removal does not promise secure erasure.
    pub fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        let event = self.slots[self.read].take();
        self.read = (self.read + 1) % N;
        self.len -= 1;
        event
    }
}
