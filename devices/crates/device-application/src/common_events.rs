//! Typed common event adapter; real observations and committed revisions come only from its port.
use crate::{ControlEvent, EventOffer, EventPlane, EventRoute, EventSource, SourceFailure};
use core::cell::Cell;
/// A product's actual committed-event journal/observer for this physical connection.
/// Its token must bind the exact pending generation, dataset/resource and live scope; returning an
/// event proves an actual observation or committed mutation, never a synthesized successful value.
pub trait CommonEventPort {
    /// Opaque nonreused pending generation/resource identity for this connection.
    type Token: Copy + Eq;
    /// Side-effect-free peek. No event may be removed here or because a FIFO is full.
    /// # Errors
    /// Real observation/store failures must be explicit.
    fn pending(&self) -> Result<Option<(Self::Token, ControlEvent)>, SourceFailure>;
    /// Fresh exact-event scope and committed-generation check, including per-resource revocation.
    fn authorized(&self, token: Self::Token) -> bool;
    /// Whether live connection/scopes require ending this old application session permanently.
    fn must_end_session(&self) -> bool;
    /// Consume/acknowledge the exact event after its actual final fragment enters the real FIFO.
    /// This is never proof of peer durable storage.
    /// # Errors
    /// Actual generation/journal failure must be reported, not retried as a new event.
    fn admitted(&mut self, token: Self::Token) -> Result<(), SourceFailure>;
    /// Mute the same old observer and release only its connection-owned state.
    /// # Errors
    /// Failed cleanup is explicit and must be safe to retry.
    fn connection_closed(&mut self) -> Result<(), SourceFailure>;
}
/// Permanently bound typed source; the application never invents state, revision or battery data.
pub struct CommonEvents<P: CommonEventPort> {
    port: P,
    selected: Cell<Option<(P::Token, ControlEvent)>>,
    pending: Option<P::Token>,
    closed: bool,
    cleaned: bool,
}
impl<P: CommonEventPort> core::fmt::Debug for CommonEvents<P> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CommonEvents")
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}
impl<P: CommonEventPort> CommonEvents<P> {
    /// Bind the real connection-scoped observer. No event payload is logged.
    pub const fn new(port: P) -> Self {
        Self {
            port,
            selected: Cell::new(None),
            pending: None,
            closed: false,
            cleaned: false,
        }
    }
}
impl<P: CommonEventPort> EventSource for CommonEvents<P> {
    type Token = P::Token;
    fn route(&self) -> Result<Option<EventRoute>, SourceFailure> {
        if self.must_end_session() {
            return Err(SourceFailure::Denied);
        }
        if self.pending.is_some() {
            return Err(SourceFailure::Invalid);
        }
        self.selected.set(None);
        let Some((token, event)) = self.port.pending()? else {
            return Ok(None);
        };
        if !self.port.authorized(token) {
            return Err(SourceFailure::Denied);
        }
        let (module, opcode, _) = event
            .encode_body(&mut [0; 9])
            .map_err(|_| SourceFailure::Invalid)?;
        self.selected.set(Some((token, event)));
        Ok(Some(EventRoute {
            module,
            opcode,
            plane: EventPlane::Control,
        }))
    }
    fn prepare(&mut self, out: &mut [u8]) -> Result<EventOffer<Self::Token>, SourceFailure> {
        if self.must_end_session() {
            return Err(SourceFailure::Denied);
        }
        if self.pending.is_some() {
            return Err(SourceFailure::Invalid);
        }
        let (token, event) = self.selected.take().ok_or(SourceFailure::NotReady)?;
        if !self.port.authorized(token) {
            return Err(SourceFailure::Denied);
        }
        let (_, _, length) = event.encode_body(out).map_err(|error| match error {
            device_protocol::ProtocolError::Capacity => SourceFailure::Capacity,
            _ => SourceFailure::Invalid,
        })?;
        self.pending = Some(token);
        Ok(EventOffer {
            token,
            length: usize::from(length),
            data_message: None,
        })
    }
    fn authorized(&self, token: Self::Token) -> bool {
        !self.must_end_session() && self.pending == Some(token) && self.port.authorized(token)
    }
    fn must_end_session(&self) -> bool {
        self.closed || self.port.must_end_session()
    }
    fn admitted(&mut self, token: Self::Token) -> Result<(), SourceFailure> {
        if !self.authorized(token) {
            return Err(SourceFailure::Denied);
        }
        self.port.admitted(token)?;
        self.pending = None;
        Ok(())
    }
    fn connection_closed(&mut self) -> Result<(), SourceFailure> {
        self.closed = true;
        self.pending = None;
        self.selected.set(None);
        if !self.cleaned {
            self.port.connection_closed()?;
            self.cleaned = true;
        }
        Ok(())
    }
}
