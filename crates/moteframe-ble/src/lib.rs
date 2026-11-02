// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Portable, bounded GATT value handling with caller-supplied backends.
//!
//! This crate provides neither a BLE stack nor a radio driver. Notification
//! success means queue acceptance only. Receive slots hold one complete,
//! already committed write and preserve its original target identity.
#![no_std]
#![forbid(unsafe_code)]

use core::fmt;

// Bluetooth Core 5.4, Vol 3, Part F, section 3.2.9: maximum attribute value size.
const MAX_ATTRIBUTE_VALUE: usize = 512;

/// Adapter for notification capacity and synchronous queue acceptance.
///
/// All calls must finish within a finite, platform-defined budget. Implementors
/// must not silently retry uncertain submissions. No operation may retain the
/// input slice after returning, including on error.
pub trait NotificationBackend {
    /// Complete routing identity, including connection generation,
    /// characteristic/database identity, and the applicable ATT bearer.
    /// Reused connection handles alone cannot identify a target safely.
    type Target: Copy + Eq;
    /// Original adapter failure, retained by [`NotifyError::Backend`].
    type Error;

    /// Queries current capacity without mutating the notification queue.
    ///
    /// Validate the target and notification subscription (CCCD). The capacity
    /// includes bearer, characteristic, and stack limits; it is not a cached
    /// global MTU. Valid capacities are in `0..=512`.
    ///
    /// # Errors
    /// Returns the adapter failure for stale, unsubscribed, or unavailable
    /// targets, or when capacity cannot be determined.
    fn notification_capacity(&self, target: Self::Target) -> Result<usize, Self::Error>;

    /// Accepts or copies one complete notification before returning success.
    ///
    /// Revalidate target identity, subscription, and capacity at submission,
    /// because they may change after the capacity query. Acceptance does not
    /// promise transmission, delivery, or peer acknowledgement. After any
    /// return the caller may immediately reuse `value`; no retry is implied
    /// by an error, which can leave submission state uncertain.
    ///
    /// # Errors
    /// Returns the original adapter error if the value cannot be accepted.
    fn try_queue_notification(
        &mut self,
        target: Self::Target,
        value: &[u8],
    ) -> Result<(), Self::Error>;
}

/// Explicit mutable reference adapter, including for `dyn` backends.
///
/// No blanket implementation for references is installed.
pub struct NotificationRef<'a, B: ?Sized> {
    inner: &'a mut B,
}

impl<'a, B: ?Sized> NotificationRef<'a, B> {
    /// Borrows a backend without calling it.
    #[must_use]
    pub fn new(inner: &'a mut B) -> Self {
        Self { inner }
    }

    /// Returns the identical backend reference without calling it.
    #[must_use]
    pub fn into_inner(self) -> &'a mut B {
        self.inner
    }
}

impl<B: ?Sized> fmt::Debug for NotificationRef<'_, B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NotificationRef").finish_non_exhaustive()
    }
}

impl<B: NotificationBackend + ?Sized> NotificationBackend for NotificationRef<'_, B> {
    type Target = B::Target;
    type Error = B::Error;

    fn notification_capacity(&self, target: Self::Target) -> Result<usize, Self::Error> {
        self.inner.notification_capacity(target)
    }

    fn try_queue_notification(
        &mut self,
        target: Self::Target,
        value: &[u8],
    ) -> Result<(), Self::Error> {
        self.inner.try_queue_notification(target, value)
    }
}

/// Input was accepted/copied by the backend and may be immediately reused.
///
/// This is not evidence of transmission, delivery, or peer acknowledgement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueAccepted;

/// Capacity validation or original backend failure.
#[derive(Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum NotifyError<E> {
    /// Original capacity-query or submission failure.
    Backend(E),
    /// Backend advertised more than the maximum ATT attribute value size.
    InvalidCapacity {
        /// Advertised capacity, which exceeds 512.
        capacity: usize,
    },
    /// Entire value cannot fit; no submission was attempted.
    TooLong {
        /// Requested number of bytes.
        length: usize,
        /// Current backend capacity.
        capacity: usize,
    },
}

/// Stateless notification checks around an owned or explicitly borrowed backend.
///
/// Retains neither payload storage nor a cached MTU. A call makes exactly one
/// capacity query and at most one submission, with no retry.
pub struct CheckedNotifier<B> {
    inner: B,
}

impl<B> CheckedNotifier<B> {
    /// Takes ownership without calling the backend.
    #[must_use]
    pub const fn new(inner: B) -> Self {
        Self { inner }
    }

    /// Recovers the backend without calling it or submitting pending work.
    #[must_use]
    pub fn into_inner(self) -> B {
        self.inner
    }
}

impl<B> fmt::Debug for CheckedNotifier<B> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CheckedNotifier").finish_non_exhaustive()
    }
}

impl<B: NotificationBackend> CheckedNotifier<B> {
    /// Queries this target's current capacity and submits one complete value.
    ///
    /// Zero capacity permits an empty value if the backend accepts it. The
    /// backend must revalidate races at submission. The returned marker only
    /// establishes queue acceptance, never over-air completion.
    ///
    /// # Errors
    /// Returns the raw backend error, an invalid capacity greater than 512,
    /// or a value length greater than current capacity. Validation failures
    /// do not submit; adapter errors are never retried.
    pub fn try_notify(
        &mut self,
        target: B::Target,
        value: &[u8],
    ) -> Result<QueueAccepted, NotifyError<B::Error>> {
        let capacity = self
            .inner
            .notification_capacity(target)
            .map_err(NotifyError::Backend)?;
        if capacity > MAX_ATTRIBUTE_VALUE {
            return Err(NotifyError::InvalidCapacity { capacity });
        }
        if value.len() > capacity {
            return Err(NotifyError::TooLong {
                length: value.len(),
                capacity,
            });
        }
        self.inner
            .try_queue_notification(target, value)
            .map_err(NotifyError::Backend)?;
        Ok(QueueAccepted)
    }
}

/// Original routing identity and complete length of a committed write event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteMeta<T> {
    /// Target identity supplied with the event, never relabeled by the slot.
    pub target: T,
    /// Complete event length in bytes, including a legitimate zero length.
    pub length: usize,
}

/// Nondestructive rejection of a write-slot operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum WriteBufferError {
    /// One event is already pending; checked before incoming value length.
    Full,
    /// Complete incoming value exceeds the usable capacity.
    TooLong {
        /// Requested number of bytes.
        length: usize,
        /// Usable slot capacity, `min(N, 512)`.
        capacity: usize,
    },
    /// Destination cannot contain the whole pending event.
    OutputTooSmall {
        /// Number of pending bytes.
        length: usize,
        /// Destination length in bytes.
        capacity: usize,
    },
}

/// Caller-sized storage for one complete, already committed receive write.
///
/// The adapter supplies writes after any protocol assembly or commit. The
/// slot performs no vendor calls, allocation, background work, or concurrency
/// coordination. `N` controls the actual owned array; values are additionally
/// limited to 512 bytes. No default 512-byte allocation is selected.
///
/// An empty event remains distinct from an empty slot, even when `N == 0`.
/// Clearing or taking invalidates bytes logically; it does not zero memory.
#[derive(Debug)]
pub struct WriteSlot<T: Copy, const N: usize> {
    bytes: [u8; N],
    pending: Option<WriteMeta<T>>,
}

impl<T: Copy, const N: usize> WriteSlot<T, N> {
    /// Creates an empty slot with exactly `N` bytes of owned payload storage.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bytes: [0; N],
            pending: None,
        }
    }

    /// Copies one complete committed value and its original target identity.
    ///
    /// # Errors
    /// Rejects a full slot first, then values longer than `min(N, 512)`.
    /// Rejection leaves all slot state unchanged and never truncates values.
    pub fn push(&mut self, target: T, value: &[u8]) -> Result<(), WriteBufferError> {
        if self.pending.is_some() {
            return Err(WriteBufferError::Full);
        }
        let capacity = N.min(MAX_ATTRIBUTE_VALUE);
        if value.len() > capacity {
            return Err(WriteBufferError::TooLong {
                length: value.len(),
                capacity,
            });
        }
        self.bytes[..value.len()].copy_from_slice(value);
        self.pending = Some(WriteMeta {
            target,
            length: value.len(),
        });
        Ok(())
    }

    /// Borrows only the valid prefix and copies the pending metadata.
    #[must_use]
    pub fn peek(&self) -> Option<(WriteMeta<T>, &[u8])> {
        self.pending.map(|meta| (meta, &self.bytes[..meta.length]))
    }

    /// Copies the complete pending event, then clears the slot.
    ///
    /// Returns `None` for no event and `Some` for an event of any length.
    /// Bytes beyond the event length in `output` remain untouched.
    ///
    /// # Errors
    /// A too-small destination leaves both slot and destination unchanged.
    pub fn take_into(
        &mut self,
        output: &mut [u8],
    ) -> Result<Option<WriteMeta<T>>, WriteBufferError> {
        let Some(meta) = self.pending else {
            return Ok(None);
        };
        if output.len() < meta.length {
            return Err(WriteBufferError::OutputTooSmall {
                length: meta.length,
                capacity: output.len(),
            });
        }
        output[..meta.length].copy_from_slice(&self.bytes[..meta.length]);
        self.pending = None;
        Ok(Some(meta))
    }

    /// Explicitly discards any pending event and returns its original metadata.
    ///
    /// This does not zero the payload storage or relabel connection events.
    pub fn clear(&mut self) -> Option<WriteMeta<T>> {
        self.pending.take()
    }
}

impl<T: Copy, const N: usize> Default for WriteSlot<T, N> {
    fn default() -> Self {
        Self::new()
    }
}
