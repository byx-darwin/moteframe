// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Bounded exact reads; no write, erase or mutable memory interface.
/// Offset-based read-only byte source. An errored output must be discarded.
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
