//! Logical body ownership strategies; fragment admission always remains in `EventPump`.
use crate::{EventOffer, EventSource, SourceFailure};
/// Source retaining one immutable, validated body until exact final admission or closure.
/// Retrying a view must expose identical bytes under the same token. Views may
/// not perform I/O or consume another credit, identifier or durable ACK.
pub trait RetainedEventSource: EventSource {
    /// Prepare only within a real first-fragment FIFO reservation.
    /// # Errors
    /// Capacity/NotReady must consume no credit or identifier.
    fn prepare_retained(&mut self, limit: usize) -> Result<EventOffer<Self::Token>, SourceFailure>;
    /// Borrow this exact frozen event for one synchronous fragment encoder.
    /// The callback cannot return a borrowed reference to its body.
    /// # Errors
    /// Missing/stale body or revoked scope fails explicitly.
    fn with_retained<T>(
        &self,
        token: Self::Token,
        inspect: impl for<'body> FnOnce(&'body [u8]) -> T,
    ) -> Result<T, SourceFailure>;
}
/// Storage used by a pump. Implementations must preserve immutable retries and
/// never admit a whole message outside the supplied first-slot boundary.
pub trait BodyStorage<S: EventSource>: core::fmt::Debug {
    /// Prepare the complete body without changing the negotiated limit.
    /// # Errors
    /// Source/preparation failures remain explicit.
    fn prepare(
        &mut self,
        source: &mut S,
        limit: usize,
    ) -> Result<EventOffer<S::Token>, SourceFailure>;
    /// Inspect the exact body for this token; no reference escapes.
    /// # Errors
    /// Missing/stale or revoked body fails explicitly.
    fn with_body<T>(
        &self,
        source: &S,
        token: S::Token,
        inspect: impl for<'body> FnOnce(&'body [u8]) -> T,
    ) -> Result<T, SourceFailure>;
    /// Clear any private copied bytes after completion or closure.
    fn clear(&mut self);
}
/// Compatibility strategy owning a complete 512-byte copy.
pub struct CopiedBody([u8; device_protocol::LOGICAL_MAX]);
impl Default for CopiedBody {
    fn default() -> Self {
        Self([0; device_protocol::LOGICAL_MAX])
    }
}
impl core::fmt::Debug for CopiedBody {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CopiedBody").finish_non_exhaustive()
    }
}
impl<S: EventSource> BodyStorage<S> for CopiedBody {
    fn prepare(
        &mut self,
        source: &mut S,
        limit: usize,
    ) -> Result<EventOffer<S::Token>, SourceFailure> {
        let out = self.0.get_mut(..limit).ok_or(SourceFailure::Capacity)?;
        source.prepare(out)
    }
    fn with_body<T>(
        &self,
        _: &S,
        _: S::Token,
        inspect: impl for<'body> FnOnce(&'body [u8]) -> T,
    ) -> Result<T, SourceFailure> {
        Ok(inspect(&self.0))
    }
    fn clear(&mut self) {
        self.0.fill(0);
    }
}
/// Zero-sized strategy borrowing the source's already retained immutable body.
#[derive(Debug, Default)]
pub struct RetainedBody;
impl<S: RetainedEventSource> BodyStorage<S> for RetainedBody {
    fn prepare(
        &mut self,
        source: &mut S,
        limit: usize,
    ) -> Result<EventOffer<S::Token>, SourceFailure> {
        source.prepare_retained(limit)
    }
    fn with_body<T>(
        &self,
        source: &S,
        token: S::Token,
        inspect: impl for<'body> FnOnce(&'body [u8]) -> T,
    ) -> Result<T, SourceFailure> {
        source.with_retained(token, inspect)
    }
    fn clear(&mut self) {}
}
