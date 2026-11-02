// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Bounded exact reads; no write, erase or mutable memory interface.
/// Offset-based read-only byte source. An errored output must be discarded.
///
/// Validate the entire range, including length conversion and offset overflow,
/// before I/O. Empty reads at `offset <= capacity()` succeed without I/O; an
/// offset beyond capacity is invalid even for an empty read. Return within a
/// finite platform-defined budget; do not retain the output slice after return.
pub trait ReadAt {
    /// Source failure.
    type Error;
    /// Accessible byte count.
    fn capacity(&self) -> u32;
    /// Reads exactly the output length; validates the entire range before I/O.
    /// # Errors
    /// Reports invalid ranges or incomplete/failed reads.
    fn read_exact(&mut self, offset: u32, out: &mut [u8]) -> Result<(), Self::Error>;
}
