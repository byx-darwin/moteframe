//! Explicit platform and owner boundaries.
use device_connection::FirstFragmentQueue;
use device_services::{Config, ServiceHandler};

/// Maximum owned characteristic value (the GATT attribute value limit).
pub const RX_CAPACITY: usize = 512;
/// Explicit port failure; adapters retain their detailed diagnostics externally.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PortError {
    /// The requested backend is unavailable.
    Unavailable,
    /// Invalid adapter input or configuration.
    InvalidInput,
    /// A backend operation failed.
    Fault,
    /// The backend cannot determine whether an operation completed.
    Uncertain,
}
/// Actual subscribed transport generation and current maximum GATT value length.
/// Generations advance on both physical reconnect and CCCD off/on. They are not peer claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Scope {
    generation: u64,
    frame_max: u16,
}
impl Scope {
    /// Validate a nonzero actual generation and current ATT/GATT capacity.
    /// # Errors
    /// Zero generations and capacities outside 20..=512 are invalid.
    pub fn new(generation: u64, frame_max: u16) -> Result<Self, PortError> {
        if generation == 0 || !(20..=512).contains(&frame_max) {
            return Err(PortError::InvalidInput);
        }
        Ok(Self {
            generation,
            frame_max,
        })
    }
    /// Actual monotonically increasing transport generation.
    #[must_use]
    pub const fn generation(self) -> u64 {
        self.generation
    }
    /// Current characteristic value capacity, after ATT overhead.
    #[must_use]
    pub const fn frame_max(self) -> u16 {
        self.frame_max
    }
}
/// One owned, scope-tagged incoming characteristic value; Debug redacts bytes.
pub struct OwnedRx {
    scope: Scope,
    bytes: [u8; RX_CAPACITY],
    length: usize,
}
impl core::fmt::Debug for OwnedRx {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("OwnedRx")
            .field("scope", &self.scope)
            .field("length", &self.length)
            .finish_non_exhaustive()
    }
}
impl OwnedRx {
    /// Copy actual RX into bounded ownership before a vendor buffer is released.
    /// # Errors
    /// Empty or over-capacity values are rejected, never truncated or overwritten.
    pub fn new(scope: Scope, bytes: &[u8]) -> Result<Self, PortError> {
        if bytes.is_empty() || bytes.len() > usize::from(scope.frame_max) {
            return Err(PortError::InvalidInput);
        }
        let mut packet = Self {
            scope,
            bytes: [0; RX_CAPACITY],
            length: bytes.len(),
        };
        packet.bytes[..bytes.len()].copy_from_slice(bytes);
        Ok(packet)
    }
    /// Scope at the time the platform took ownership of this value.
    #[must_use]
    pub const fn scope(&self) -> Scope {
        self.scope
    }
    pub(crate) fn bytes(&self) -> &[u8] {
        &self.bytes[..self.length]
    }
}
/// Persistent owners/configuration source, borrowed for the entire runtime lifetime.
/// Handlers borrow external durable owners and trusted authorization. A fresh nonzero,
/// increasing application session ID and truthful immutable metadata are required on every open.
/// Unbound event/DATA features must not be advertised: this runtime owns only CONTROL.
pub trait SessionFactory {
    /// Fresh per-subscription actor borrowing this factory's stable owners.
    type Handler<'a>: ServiceHandler
    where
        Self: 'a;
    /// Construct fresh configuration and application handler for the actual transport scope.
    /// # Errors
    /// Report unavailable/corrupt/untrusted owners explicitly, with no fallback defaults.
    /// On failure the factory must clean up anything it began before returning.
    fn open(&self, scope: Scope) -> Result<(Config<'_>, Self::Handler<'_>), PortError>;
}
/// Real BLE executor, RX owner and ordered queue boundary. No successful defaults are provided.
pub trait Transport {
    /// Guard over the existing actual queue, bound to the requested scope.
    type Queue<'a>: FirstFragmentQueue
    where
        Self: 'a;
    /// Service Host/Controller futures with at most `budget` bounded work units.
    /// # Errors
    /// Explicit host/controller failure; runtime retires its old application scope.
    fn poll(&mut self, now_ms: u64, budget: u8) -> Result<(), PortError>;
    /// Read the actual currently subscribed scope, or None while disconnected/CCCD disabled.
    /// # Errors
    /// An unavailable or uncertain actual scope must fail closed.
    fn scope(&self) -> Result<Option<Scope>, PortError>;
    /// Transfer at most one owned packet; caller refrains from polling while its Busy slot is occupied.
    /// Overflow must be rejected explicitly or close the scope, never overwrite an older value.
    /// # Errors
    /// RX/overflow/ownership failure must be explicit.
    fn receive(&mut self) -> Result<Option<OwnedRx>, PortError>;
    /// Bind a real queue guard to expected scope, without reserving a slot yet.
    /// The guard retains the serialized actor boundary until it is dropped, including the
    /// caller's `response_admitted` callback. Scope revocation, fill and publication MUST be
    /// serialized together. The guard validates
    /// expected scope again at reservation/publication; a change rejects publication with Contract.
    /// `NoSlot` never calls fill. Transport/Contract uncertainty closes the old scope, never retries.
    /// The returned guard must not consume any independent DATA producer token.
    /// # Errors
    /// Guard acquisition/scope failure; no publication may have happened on a clean error.
    fn queue(&mut self, expected: Scope) -> Result<Self::Queue<'_>, PortError>;
    /// Mute/drop old scoped RX/TX and any platform work before another actor opens.
    /// Must not revoke a newer actual scope. Repeated calls are idempotent.
    /// # Errors
    /// Failed/uncertain cleanup blocks reuse until explicit `Runtime::retry_cleanup` succeeds.
    fn close_scope(&mut self, old: Scope) -> Result<(), PortError>;
}
/// One bounded, nonblocking UI/render or background/storage slice, with no successful default.
pub trait Work {
    /// Perform one slice; retain unfinished work and damage on failure.
    /// # Errors
    /// Actual unavailable, invalid, failed or uncertain backend work is surfaced to the caller.
    fn step(&mut self, now_ms: u64) -> Result<(), PortError>;
}
