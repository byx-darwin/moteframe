// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Checked read-only byte sources and fixed logical windows.
//!
//! These views add no allocation, retries, physical Flash operations, or
//! persistence. Providers must complete each operation within a finite,
//! platform-defined budget, as required by [`ReadAt`]. On a provider error,
//! discard the output: it may contain a partial read.
#![no_std]
#![forbid(unsafe_code)]

use core::fmt;
use moteframe_core::ReadAt;

/// A byte range cannot be represented or is outside its source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RangeError {
    /// A slice or output length exceeds the `u32` address space.
    LengthTooLarge,
    /// The range overflows or extends beyond the accessible byte count.
    OutOfBounds,
}

/// A window rejected a range or its provider failed an exact read.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReadError<E> {
    /// Validation failed before any provider read or output mutation.
    Range(RangeError),
    /// The provider's original failure; output may already be partially changed.
    Source(E),
}

fn checked_length(length: usize) -> Result<u32, RangeError> {
    u32::try_from(length).map_err(|_| RangeError::LengthTooLarge)
}

fn checked_range(capacity: u32, offset: u32, length: u32) -> Result<u32, RangeError> {
    let end = offset.checked_add(length).ok_or(RangeError::OutOfBounds)?;
    if end > capacity {
        return Err(RangeError::OutOfBounds);
    }
    Ok(end)
}

/// An exact-read view over an immutable slice of at most `u32::MAX` bytes.
pub struct SliceSource<'a> {
    bytes: &'a [u8],
}

impl fmt::Debug for SliceSource<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SliceSource")
            .field("length", &self.bytes.len())
            .finish_non_exhaustive()
    }
}

impl<'a> SliceSource<'a> {
    /// Wraps a slice after validating its capacity is representable as `u32`.
    ///
    /// # Errors
    /// Returns [`RangeError::LengthTooLarge`] for an unrepresentable capacity.
    pub fn new(bytes: &'a [u8]) -> Result<Self, RangeError> {
        checked_length(bytes.len())?;
        Ok(Self { bytes })
    }

    /// Returns the identical original slice without reading or copying it.
    #[must_use]
    pub fn into_inner(self) -> &'a [u8] {
        self.bytes
    }
}

impl ReadAt for SliceSource<'_> {
    type Error = RangeError;

    #[allow(clippy::cast_possible_truncation)] // Constructor checked this immutable length.
    fn capacity(&self) -> u32 {
        self.bytes.len() as u32
    }

    fn read_exact(&mut self, offset: u32, out: &mut [u8]) -> Result<(), Self::Error> {
        let length = checked_length(out.len())?;
        let end = checked_range(self.capacity(), offset, length)?;
        // The range is within the original slice, so both indices fit usize.
        let start = usize::try_from(offset).map_err(|_| RangeError::OutOfBounds)?;
        let end = usize::try_from(end).map_err(|_| RangeError::OutOfBounds)?;
        out.copy_from_slice(&self.bytes[start..end]);
        Ok(())
    }
}

/// Explicitly forwards reads through a mutable borrow, including a `dyn` source.
///
/// This wrapper adds no validation or retry and retains no output borrow.
pub struct ReadRef<'a, R: ?Sized> {
    inner: &'a mut R,
}

impl<R: ?Sized> fmt::Debug for ReadRef<'_, R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReadRef").finish_non_exhaustive()
    }
}

impl<'a, R: ?Sized> ReadRef<'a, R> {
    /// Wraps the original mutable borrow without calling the provider.
    #[must_use]
    pub fn new(inner: &'a mut R) -> Self {
        Self { inner }
    }

    /// Returns the original mutable borrow without calling the provider.
    #[must_use]
    pub fn into_inner(self) -> &'a mut R {
        self.inner
    }
}

impl<R: ReadAt + ?Sized> ReadAt for ReadRef<'_, R> {
    type Error = R::Error;

    fn capacity(&self) -> u32 {
        self.inner.capacity()
    }

    fn read_exact(&mut self, offset: u32, out: &mut [u8]) -> Result<(), Self::Error> {
        self.inner.read_exact(offset, out)
    }
}

/// A rejected window construction retaining the reason and original provider.
///
/// Its [`Debug`](fmt::Debug) implementation omits the provider and imposes no
/// `Debug` bound on it. Extraction only transfers ownership; it performs no I/O.
#[non_exhaustive]
pub struct WindowCreateError<R> {
    error: RangeError,
    inner: R,
}

impl<R> WindowCreateError<R> {
    /// Borrows the validation failure while retaining the original provider.
    #[must_use]
    pub const fn error(&self) -> &RangeError {
        &self.error
    }

    /// Returns the validation failure and original provider without any I/O.
    #[must_use]
    pub fn into_parts(self) -> (RangeError, R) {
        (self.error, self.inner)
    }
}

impl<R> fmt::Debug for WindowCreateError<R> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("WindowCreateError")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

/// A fixed logical subrange of a provider, rechecking physical bounds per read.
///
/// Growth never enlarges this view. Shrink rejects affected nonempty reads
/// before provider I/O; valid empty reads still succeed anywhere through the
/// logical end, even when the provider has shrunk below the window's start.
pub struct ReadWindow<R: ReadAt> {
    inner: R,
    start: u32,
    length: u32,
}

impl<R: ReadAt> fmt::Debug for ReadWindow<R> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ReadWindow")
            .field("start", &self.start)
            .field("length", &self.length)
            .finish_non_exhaustive()
    }
}

impl<R: ReadAt> ReadWindow<R> {
    /// Validates the full initial range and takes ownership of the provider.
    ///
    /// Calls [`ReadAt::capacity`] once and never reads the provider.
    ///
    /// # Errors
    /// Returns [`RangeError::OutOfBounds`] and the original provider if the
    /// initial range overflows or exceeds its current capacity.
    pub fn new(inner: R, start: u32, length: u32) -> Result<Self, WindowCreateError<R>> {
        if let Err(error) = checked_range(inner.capacity(), start, length) {
            return Err(WindowCreateError { error, inner });
        }
        Ok(Self {
            inner,
            start,
            length,
        })
    }

    /// Returns the original provider without a read, capacity query, or recovery.
    #[must_use]
    pub fn into_inner(self) -> R {
        self.inner
    }
}

impl<R: ReadAt> ReadAt for ReadWindow<R> {
    type Error = ReadError<R::Error>;

    fn capacity(&self) -> u32 {
        self.length
    }

    fn read_exact(&mut self, offset: u32, out: &mut [u8]) -> Result<(), Self::Error> {
        let length = checked_length(out.len()).map_err(ReadError::Range)?;
        let local_end = checked_range(self.length, offset, length).map_err(ReadError::Range)?;
        if length == 0 {
            return Ok(());
        }
        let mapped = self
            .start
            .checked_add(offset)
            .ok_or(ReadError::Range(RangeError::OutOfBounds))?;
        let mapped_end = self
            .start
            .checked_add(local_end)
            .ok_or(ReadError::Range(RangeError::OutOfBounds))?;
        if mapped_end > self.inner.capacity() {
            return Err(ReadError::Range(RangeError::OutOfBounds));
        }
        self.inner
            .read_exact(mapped, out)
            .map_err(ReadError::Source)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    #[cfg(target_pointer_width = "64")]
    fn oversized_length_rejected_without_allocating() {
        assert_eq!(
            super::checked_length(usize::MAX),
            Err(super::RangeError::LengthTooLarge)
        );
    }
}
