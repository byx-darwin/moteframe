// SPDX-License-Identifier: Apache-2.0
//! Board-supplied operations on two exclusively reserved physical slots.

/// The two slots have identical geometry and independent failure domains.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Slot {
    /// Initial destination.
    A,
    /// Alternate destination.
    B,
}

/// Exclusive, board-authorized region. The board must check layout ownership,
/// disjoint erase/failure units and updater reservations before granting access.
/// All operations are bounded. No DMA may retain borrowed memory on return.
/// No other writer may change these slots while the region is borrowed.
pub trait SlotRegion<const PAGE: usize> {
    /// Driver error, including busy and timeout.
    type Error;
    /// Stable usable byte count of each slot. Both start on page boundaries.
    fn slot_bytes(&self) -> usize;
    /// Confirm controller and medium idle, reconcile previous operations and
    /// restore coherent reads without mutating persistent bytes.
    ///
    /// # Errors
    /// Return an error while any previous operation is unresolved.
    fn ready(&mut self) -> Result<(), Self::Error>;
    /// Read exactly the specified range from the medium.
    ///
    /// # Errors
    /// Report any incomplete or untrustworthy read.
    fn read(&mut self, slot: Slot, offset: usize, out: &mut [u8]) -> Result<(), Self::Error>;
    /// Prepare the entire target slot to 0xff. On rewritable media invalidate
    /// and verify the whole last page before changing any earlier page.
    ///
    /// # Errors
    /// Report failures, including unknown outcomes. Enforce the board write gate.
    fn prepare(&mut self, slot: Slot) -> Result<(), Self::Error>;
    /// Program exactly one aligned page and await completion.
    ///
    /// # Errors
    /// Report failures, including unknown outcomes. Enforce the board write gate.
    fn program_page(
        &mut self,
        slot: Slot,
        offset: usize,
        page: &[u8; PAGE],
    ) -> Result<(), Self::Error>;
}
