//! Explicit wire and lifecycle failures.
/// Failure independent of transport and hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    /// Header or fixed payload is incomplete.
    Truncated,
    /// Unsupported wire version.
    InvalidVersion,
    /// Unknown message kind.
    InvalidKind,
    /// A reserved field is nonzero.
    Reserved,
    /// A required identifier is zero.
    InvalidId,
    /// Length or offset violates the contract.
    InvalidLength,
    /// Caller supplied insufficient capacity.
    Capacity,
    /// Fragment offset is not contiguous.
    Sequence,
    /// Fragments have different identities.
    Identity,
    /// A receive deadline expired.
    Timeout,
    /// Operation is invalid for current lifecycle.
    InvalidState,
    /// Content integrity does not match.
    Integrity,
}
