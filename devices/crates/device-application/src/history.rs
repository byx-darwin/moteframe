//! The core credit/dataset bridge borrows exactly the actor used by history dispatch and DATA.
use crate::{CreditPort, DatasetPort, ServiceCell};
use device_history_service::{HistoryFailure, HistoryHandler, RecordStore};
use device_protocol::DataKind;
use device_services::{DatasetIdentity, ResourceFailure};
/// A short-borrow bridge; multiple handles share one history actor, never clone its lease or lane.
pub struct HistoryPort<'a, 'store, R: RecordStore, const D: usize, const P: usize> {
    owner: &'a ServiceCell<HistoryHandler<'store, R, D, P>>,
}
impl<R: RecordStore, const D: usize, const P: usize> core::fmt::Debug
    for HistoryPort<'_, '_, R, D, P>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HistoryPort").finish_non_exhaustive()
    }
}
impl<'a, 'store, R: RecordStore, const D: usize, const P: usize> HistoryPort<'a, 'store, R, D, P> {
    /// Bind the one connection-owned history actor already using the actual shared DATA lane.
    pub const fn new(owner: &'a ServiceCell<HistoryHandler<'store, R, D, P>>) -> Self {
        Self { owner }
    }
}
impl<R: RecordStore, const D: usize, const P: usize> CreditPort for HistoryPort<'_, '_, R, D, P> {
    fn authorized(&self) -> bool {
        self.owner
            .with(HistoryHandler::grant_authorized)
            .unwrap_or(false)
    }
    fn validate(&self, stream: u16, total: u32) -> Result<(), ResourceFailure> {
        self.owner
            .with(|h| h.validate_grant(DataKind::History, stream, total))
            .map_err(|_| ResourceFailure::Busy)?
            .map_err(failure)
    }
    fn grant(&mut self, stream: u16, total: u32) -> Result<u32, ResourceFailure> {
        self.owner
            .with_mut(|h| h.grant(DataKind::History, stream, total))
            .map_err(|_| ResourceFailure::Busy)?
            .map_err(failure)?;
        Ok(total)
    }
    fn replay_valid(&self, stream: u16, accepted: u32) -> bool {
        self.owner
            .with(|h| h.grant_authorized() && h.grant_replay_valid(stream, accepted))
            .unwrap_or(false)
    }
}
impl<R: RecordStore, const D: usize, const P: usize> DatasetPort for HistoryPort<'_, '_, R, D, P> {
    fn authorized(&self) -> bool {
        self.owner
            .with(HistoryHandler::dataset_authorized)
            .unwrap_or(false)
    }
    fn identity(&self) -> Result<DatasetIdentity, ResourceFailure> {
        let id = self
            .owner
            .with(HistoryHandler::dataset)
            .map_err(|_| ResourceFailure::Busy)?
            .map_err(failure)?;
        DatasetIdentity::new(id)
    }
}
fn failure(error: HistoryFailure) -> ResourceFailure {
    match error {
        HistoryFailure::Invalid => ResourceFailure::InvalidArgument,
        HistoryFailure::Denied => ResourceFailure::AccessDenied,
        HistoryFailure::Busy => ResourceFailure::Busy,
        HistoryFailure::Capacity => ResourceFailure::LimitExceeded,
        HistoryFailure::NotFound | HistoryFailure::Identity => ResourceFailure::NotFound,
        HistoryFailure::Storage => ResourceFailure::StorageFailure,
        HistoryFailure::Conflict => ResourceFailure::Conflict,
        HistoryFailure::StoredIntegrity => ResourceFailure::StoredIntegrity,
        HistoryFailure::InvalidState
        | HistoryFailure::CursorExpired
        | HistoryFailure::Closed
        | HistoryFailure::Clock
        | HistoryFailure::Timeout
        | HistoryFailure::TransportUnavailable => ResourceFailure::InvalidState,
    }
}

/// Trusted actual clock shared with the connection/service executor, never supplied by peer data.
pub trait HistoryEventClock {
    /// Actual monotonic milliseconds.
    /// # Errors
    /// Unavailable/regressed clock is explicit; no fabricated zero fallback is allowed.
    fn now_ms(&self) -> Result<u64, HistoryFailure>;
}
/// Opaque exact history event context; no payload, dataset identity or physical address is logged.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HistoryEventToken {
    kind: device_history_service::HistoryPendingEvent,
    message: Option<u16>,
}
impl core::fmt::Debug for HistoryEventToken {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HistoryEventToken").finish_non_exhaustive()
    }
}
/// One source view of the same checked actor used by control/GRANT/DATASET; owns no extra stream.
pub struct HistoryEvents<'a, 'store, R: RecordStore, const D: usize, const P: usize> {
    owner: &'a ServiceCell<HistoryHandler<'store, R, D, P>>,
    clock: &'a dyn HistoryEventClock,
    selected: core::cell::Cell<Option<device_history_service::HistoryPendingEvent>>,
}
impl<R: RecordStore, const D: usize, const P: usize> core::fmt::Debug
    for HistoryEvents<'_, '_, R, D, P>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HistoryEvents").finish_non_exhaustive()
    }
}
impl<'a, 'store, R: RecordStore, const D: usize, const P: usize>
    HistoryEvents<'a, 'store, R, D, P>
{
    /// Bind the real actor and clock. The outer `EventPump` owns this source for the connection lifetime.
    pub const fn new(
        owner: &'a ServiceCell<HistoryHandler<'store, R, D, P>>,
        clock: &'a dyn HistoryEventClock,
    ) -> Self {
        Self {
            owner,
            clock,
            selected: core::cell::Cell::new(None),
        }
    }
}
impl<R: RecordStore, const D: usize, const P: usize> device_connection::EventSource
    for HistoryEvents<'_, '_, R, D, P>
{
    type Token = HistoryEventToken;
    fn route(
        &self,
    ) -> Result<Option<device_connection::EventRoute>, device_connection::SourceFailure> {
        use device_history_service::HistoryPendingEvent as Pending;
        let now = self.clock.now_ms().map_err(event_failure)?;
        let pending = self
            .owner
            .with_mut(|history| {
                history.advance(now)?;
                history.pending_event()
            })
            .map_err(|_| device_connection::SourceFailure::NotReady)?
            .map_err(event_failure)?;
        self.selected.set(pending);
        Ok(pending.map(|kind| device_connection::EventRoute {
            module: 0x10,
            opcode: match kind {
                Pending::Record { .. } => 0x80,
                Pending::Done { .. } => 0x81,
            },
            plane: match kind {
                Pending::Record { .. } => device_connection::EventPlane::Data,
                Pending::Done { .. } => device_connection::EventPlane::Control,
            },
        }))
    }
    fn prepare(
        &mut self,
        out: &mut [u8],
    ) -> Result<device_connection::EventOffer<Self::Token>, device_connection::SourceFailure> {
        use device_connection::SourceFailure;
        use device_history_service::HistoryPendingEvent as Pending;
        let selected = self.selected.get().ok_or(SourceFailure::NotReady)?;
        self.owner
            .with_mut(|history| {
                if history.pending_event().map_err(event_failure)? != Some(selected) {
                    return Err(SourceFailure::NotReady);
                }
                match selected {
                    Pending::Record { .. } => {
                        let event = history
                            .next_data(out)
                            .map_err(event_failure)?
                            .ok_or(SourceFailure::NotReady)?;
                        let message = core::num::NonZeroU16::new(event.message_id)
                            .ok_or(SourceFailure::Invalid)?;
                        Ok(device_connection::EventOffer {
                            token: HistoryEventToken {
                                kind: selected,
                                message: Some(message.get()),
                            },
                            length: event.length,
                            data_message: Some(message),
                        })
                    }
                    Pending::Done { .. } => {
                        let length = history
                            .done(out)
                            .map_err(event_failure)?
                            .ok_or(SourceFailure::NotReady)?;
                        Ok(device_connection::EventOffer {
                            token: HistoryEventToken {
                                kind: selected,
                                message: None,
                            },
                            length,
                            data_message: None,
                        })
                    }
                }
            })
            .map_err(|_| SourceFailure::NotReady)?
    }
    fn authorized(&self, token: Self::Token) -> bool {
        let Ok(now) = self.clock.now_ms() else {
            return false;
        };
        self.owner
            .with_mut(|history| {
                history.advance(now).is_ok() && history.event_valid(token.kind, token.message)
            })
            .unwrap_or(false)
    }
    fn must_end_session(&self) -> bool {
        self.owner
            .with(device_services::ServiceHandler::must_end_session)
            .unwrap_or(true)
    }
    fn admitted(&mut self, token: Self::Token) -> Result<(), device_connection::SourceFailure> {
        use device_history_service::HistoryPendingEvent as Pending;
        self.owner
            .with_mut(|history| {
                if !history.event_valid(token.kind, token.message) {
                    return Err(HistoryFailure::Identity);
                }
                match token.kind {
                    Pending::Record { .. } => {
                        history.finish_data(token.message.ok_or(HistoryFailure::Identity)?)
                    }
                    Pending::Done { stream_id } => history.done_admitted(stream_id),
                }
            })
            .map_err(|_| device_connection::SourceFailure::Backend)?
            .map_err(event_failure)
    }
    fn connection_closed(&mut self) -> Result<(), device_connection::SourceFailure> {
        self.selected.set(None);
        self.owner
            .with_mut(device_services::ServiceHandler::connection_closed)
            .map_err(|_| device_connection::SourceFailure::Backend)?
            .map_err(|_| device_connection::SourceFailure::Backend)
    }
}
fn event_failure(error: HistoryFailure) -> device_connection::SourceFailure {
    use device_connection::SourceFailure;
    match error {
        HistoryFailure::Denied => SourceFailure::Denied,
        HistoryFailure::Busy => SourceFailure::NotReady,
        HistoryFailure::Capacity => SourceFailure::Capacity,
        HistoryFailure::Storage | HistoryFailure::StoredIntegrity => SourceFailure::Backend,
        _ => SourceFailure::Invalid,
    }
}
