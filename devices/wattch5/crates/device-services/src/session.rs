//! Synchronous core dispatch over already reassembled logical requests.
//!
//! Status values follow the v0.5 common status/reason appendix
//! (`build_v05.py:325-326` in the protocol review). Transport owns fragmentation,
//! deadlines and delivery. A successful dispatch returns one complete logical
//! response body, including the four-byte common status and no transport header.

use core::fmt;
use device_protocol::{
    Frame, Header, HelloRequest, HelloResponse, Kind, LOGICAL_MAX, ModuleEntry, ProtocolError,
    ProtocolLimits,
};

const STATUS_LENGTH: usize = 4;
const MODULE_PAGE_HEADER: usize = STATUS_LENGTH + 7;

/// Build identity, independent of authentication or resource permissions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum BuildProfile {
    /// Commercial build.
    Commercial = 0,
    /// Development build.
    Development = 1,
    /// Factory build.
    Factory = 2,
}

/// Informational access state; this is never a resource authorization token.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AccessState {
    /// Explicit experimental state supplied by a trusted integration.
    Experimental = 0,
    /// Anonymous or otherwise restricted access.
    Restricted = 1,
    /// Trusted integration reports authorization; individual scopes still apply.
    Authorized = 2,
}

/// Trusted access state and its separate informational reason.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessSnapshot {
    /// Reported state; does not grant a role or a dataset/stream scope.
    pub state: AccessState,
    /// ACCESS-specific reason, independent of the common response reason.
    pub reason: u16,
}

/// Integration-owned source of access state, never decoded from request bytes.
///
/// This interface only reports state. It deliberately cannot invent stream or
/// dataset contexts. Those services must later verify trusted roles and resource
/// scopes independently; reporting `Authorized` alone cannot unlock 00.07/00.08.
pub trait TrustedAccessProvider {
    /// Read access state from trusted connection/security integration.
    fn access_snapshot(&self) -> AccessSnapshot;
}

/// Default provider exposing only anonymous minimal core information.
#[derive(Debug, Clone, Copy, Default)]
pub struct AnonymousAccess;
impl TrustedAccessProvider for AnonymousAccess {
    fn access_snapshot(&self) -> AccessSnapshot {
        AccessSnapshot {
            state: AccessState::Restricted,
            reason: 0,
        }
    }
}

/// Fixed fourteen-byte INFO success data, excluding the common status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeviceInfo {
    /// Product number supplied by the actual product integration.
    pub product_id: u32,
    /// Firmware major version.
    pub fw_major: u16,
    /// Firmware minor version.
    pub fw_minor: u16,
    /// Firmware patch version.
    pub fw_patch: u16,
    /// Build identity supplied by the actual firmware build.
    pub build_id: u32,
}
impl DeviceInfo {
    /// Exact encoded success-data length.
    pub const LENGTH: usize = 14;

    /// Decode exact little-endian INFO success data.
    /// # Errors
    /// Returns `InvalidLength` for missing or additional bytes.
    pub fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() != Self::LENGTH {
            return Err(ProtocolError::InvalidLength);
        }
        Ok(Self {
            product_id: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            fw_major: u16::from_le_bytes([bytes[4], bytes[5]]),
            fw_minor: u16::from_le_bytes([bytes[6], bytes[7]]),
            fw_patch: u16::from_le_bytes([bytes[8], bytes[9]]),
            build_id: u32::from_le_bytes([bytes[10], bytes[11], bytes[12], bytes[13]]),
        })
    }

    /// Encode fourteen little-endian bytes into caller storage.
    /// # Errors
    /// Returns `Capacity` without writing if the output is shorter than fourteen.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, ProtocolError> {
        if out.len() < Self::LENGTH {
            return Err(ProtocolError::Capacity);
        }
        out[..4].copy_from_slice(&self.product_id.to_le_bytes());
        out[4..6].copy_from_slice(&self.fw_major.to_le_bytes());
        out[6..8].copy_from_slice(&self.fw_minor.to_le_bytes());
        out[8..10].copy_from_slice(&self.fw_patch.to_le_bytes());
        out[10..14].copy_from_slice(&self.build_id.to_le_bytes());
        Ok(Self::LENGTH)
    }
}

/// Immutable connection configuration and actual compiled-module snapshot.
///
/// Borrowing the sorted module slice prevents safe mutation during the session.
/// Entries are explicit integration assertions, never inferred from SDK sources.
/// An availability of zero/two describes an actually registered unavailable/
/// faulty module; omitted modules do not occupy indices. Changing the snapshot
/// requires a new connection and a new nonzero capability revision in this core.
#[derive(Debug, Clone, Copy)]
pub struct Config<'a> {
    /// Nonzero connection-local session identifier, never an access token.
    pub session: u32,
    /// Nonzero revision for this immutable capability snapshot.
    pub capability_rev: u32,
    /// Build identity; cannot grant access.
    pub profile: BuildProfile,
    /// Real build/product information to return through INFO.
    pub info: DeviceInfo,
    /// Actual registered modules, strictly ascending and unique, including core.
    pub modules: &'a [ModuleEntry],
    /// Actual transport/control/data capacities and fixed contract deadlines.
    /// A zero data capacity explicitly means no registered data backend.
    pub limits: ProtocolLimits,
}

/// Invalid integration configuration, rejected before the session can open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    /// Session identifier is zero.
    InvalidSession,
    /// Capability revision is zero.
    InvalidCapabilityRevision,
    /// Transport/logical capacities or fixed deadlines violate the contract.
    InvalidLimits,
    /// Catalogue order, core identity, entry fields or declared maxima are invalid.
    InvalidModules,
}

/// Local dispatch/lifecycle failure; no response bytes are produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionError {
    /// Control notifications have not been enabled for this connection.
    NotificationsRequired,
    /// Notifications were disabled or the connection was disconnected.
    Closed,
    /// A previous request identity/structure violation terminated this session.
    Faulted,
    /// Input was not one complete nonzero-ID request; session becomes faulted.
    InvalidFrame,
    /// The last request ID was reused with different identity or payload bytes.
    RequestConflict,
    /// The first ID was not one, or a completed older ID was reused/wrapped.
    RequestOrder,
    /// Caller response buffer is too short; no request is consumed or executed.
    Capacity,
    /// Local response gate is waiting for actual resource drain; never encode wire Pending.
    /// Poll the identical complete request later. Another identity faults the session.
    Deferred,
    /// A validated protocol codec could not encode its reply.
    Codec(ProtocolError),
    /// Live business scope was revoked; session/cache were closed and cleared.
    AuthorizationRevoked,
    /// Handler returned an invalid or overlarge response after execution.
    InvalidHandlerResponse,
}

/// Integration-owned business dispatcher with an independent trusted scope provider.
/// It must never derive permission from informational access state or build profile.
pub trait ServiceHandler {
    /// Advance actual connection-executor time before dispatch or queue attempts.
    /// The default is a true stateless noop; services owning deadlines must override it.
    /// It does not claim that physical timers, acquisition or backend work progressed.
    /// # Errors
    /// Clock, scope or real owned-resource failure requires ending this connection.
    fn advance(&mut self, _now_ms: u64) -> Result<(), SessionError> {
        Ok(())
    }
    /// Optional trusted informational snapshot; never grants any per-resource scope.
    fn informational_access(&self) -> Option<AccessSnapshot> {
        None
    }
    /// Recheck a cached/queued reply's resource context without repeating side effects.
    /// Stateful ephemeral resources may reject stale stream/dataset bindings. Ordinary snapshots
    /// may replay their historical reply while authorization remains valid.
    fn replay_valid(&self, _request: Frame<'_>, _response: &[u8]) -> bool {
        true
    }
    /// Entire successful response was transport-admitted for an original request, never its replay.
    /// Deferred side effects/events that require response-first may start here. Synchronous
    /// handlers whose commits completed in dispatch have no additional admission action.
    /// # Errors
    /// Actual/uncertain backend failure; caller must end the application connection.
    fn response_admitted(
        &mut self,
        _request: Frame<'_>,
        _response: &[u8],
    ) -> Result<(), SessionError> {
        Ok(())
    }
    /// Invalidate connection-owned work after close/CCCD revocation. Stateful implementations
    /// must discard staging and mute old work. Repeated cleanup must be harmless.
    /// # Errors
    /// Actual cleanup failure; the connection still stays closed.
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        Ok(())
    }
    /// Poll a nonblocking prerequisite before dispatch, after identity, scope and capacity checks.
    /// Stateful implementations must validate the full business body before staging anything,
    /// keep repeated polls idempotent, and return false until real started packets/resources drained.
    /// Synchronous handlers need no prerequisite. A false result never produces a wire response.
    /// # Errors
    /// Real prerequisite/authorization failure; Deferred is generated by Session, not returned here.
    fn response_ready(&mut self, _request: Frame<'_>) -> Result<bool, SessionError> {
        Ok(true)
    }
    /// Whether an owned resource/scope revocation permanently ended this handler's session.
    /// Synchronous handlers with no retained work need no close signal. Stateful handlers must
    /// return true after discarding revoked staging; informational access state is not sufficient.
    fn must_end_session(&self) -> bool {
        false
    }
    /// Return required declared feature bits only for a compiled compatible
    /// module version/opcode; None means unsupported. No backend side effects.
    fn required_features(&self, module: &ModuleEntry, opcode: u8) -> Option<u32>;
    /// Check current module/resource scope, including request key as applicable.
    /// Called again before releasing cached or newly generated responses.
    fn authorized(&self, request: Frame<'_>) -> bool;
    /// Execute against an actual backend and return exact status+body length.
    /// Capacity failures must occur before side effects. A nonzero wire status
    /// must contain only its four-byte prefix; Pending is forbidden.
    /// # Errors
    /// Report capacity or live revocation; other local failures fault the session.
    fn dispatch(&mut self, request: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError>;
    /// Execute after this exact request passed `response_ready` in the same serialized scope.
    /// The default preserves existing handlers. Stateful overrides must recheck authority
    /// and reject missing/consumed readiness before any successful side effects.
    /// # Errors
    /// Same capacity, scope and backend failures as `dispatch`.
    fn dispatch_ready(
        &mut self,
        request: Frame<'_>,
        out: &mut [u8],
    ) -> Result<usize, SessionError> {
        self.dispatch(request, out)
    }
}

/// Connection-local lifecycle. Closed and faulted states cannot reopen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    /// Created for a new physical connection, before notification subscription.
    AwaitingNotifications,
    /// Notification subscription opened, awaiting successful HELLO.
    AwaitingHello,
    /// HELLO negotiated; only independently authorized operations may execute.
    Active,
    /// Connection ended or control notifications were disabled.
    Closed,
    /// Malformed or conflicting request requires a fresh physical connection.
    Faulted,
}

/// Common status numbers from the v0.5 appendix; `Pending` is never emitted here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Status {
    /// Success.
    Ok = 0x0000,
    /// Invalid format, value or length.
    InvalidArgument = 0x0001,
    /// Module was not registered.
    UnsupportedModule = 0x0002,
    /// Command was not provided by the module.
    UnsupportedOpcode = 0x0003,
    /// Operation is invalid in the current lifecycle.
    InvalidState = 0x0004,
    /// Trusted resource authorization is absent.
    AccessDenied = 0x0005,
    /// Resource is busy.
    Busy = 0x0006,
    /// Capacity or declared maximum exceeded.
    LimitExceeded = 0x0007,
    /// Target or result does not exist.
    NotFound = 0x0008,
    /// Durable storage failed.
    StorageFailure = 0x0009,
    /// Deadline expired.
    Timeout = 0x000a,
    /// Version or duplicate request conflicts.
    Conflict = 0x000b,
    /// Cursor or snapshot expired.
    CursorExpired = 0x000c,
    /// Wire/contract versions do not match.
    VersionMismatch = 0x000d,
    /// Real hardware failed.
    HardwareFailure = 0x000e,
    /// Operation was cancelled.
    Cancelled = 0x000f,
    /// Reserved by the contract and forbidden as a response.
    Pending = 0x0010,
    /// Internal operation failed.
    InternalFailure = 0x0011,
    /// Content integrity check failed.
    IntegrityFailure = 0x0012,
}

/// Global reasons from the v0.5 appendix; encoded as little-endian `u16`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u16)]
pub enum Reason {
    /// No additional reason.
    None = 0x0000,
    /// CRC mismatch; accompanies integrity failure.
    CrcMismatch = 0xff01,
    /// Stored CRC mismatch; accompanies integrity failure.
    StoredCrcMismatch = 0xff02,
    /// Required trusted role or resource scope is absent; accompanies access denied.
    RoleRequired = 0xff03,
    /// Dangerous-write scope is absent; accompanies access denied.
    DangerousScopeRequired = 0xff04,
    /// Filtered snapshot ACK forbidden; accompanies invalid state.
    FilteredAckForbidden = 0xff05,
    /// Job binding/state mismatch; accompanies invalid state.
    JobBindingMismatch = 0xff06,
    /// Conflicting terminal result; accompanies conflict.
    TerminalConflict = 0xff07,
    /// Unsupported enumeration; accompanies invalid argument.
    EnumUnsupported = 0xff08,
}

/// Synchronous, allocation-free core service for one physical connection.
///
/// The exclusive mutable borrow permits one dispatch at a time. Only the last
/// complete request and logical response are cached. Transport must finish the
/// response's delivery before accepting another request; this core does not own
/// an asynchronous GATT transmit queue or an in-flight timeout timer.
/// Debug intentionally omits catalogue contents, INFO, request and response data.
pub type Session<'a> = BoundedSession<'a, LOGICAL_MAX>;
/// CONTROL session with explicitly bounded replay buffers; DATA limits are independent.
pub struct BoundedSession<'a, const N: usize> {
    config: Config<'a>,
    state: SessionState,
    limits: ProtocolLimits,
    last_header: Option<Header>,
    pending_header: Option<Header>,
    cached_request: [u8; N],
    cached_response: [u8; N],
    response_length: usize,
}
impl<const N: usize> fmt::Debug for BoundedSession<'_, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("state", &self.state)
            .field(
                "last_message_id",
                &self.last_header.map(|header| header.message_id),
            )
            .finish_non_exhaustive()
    }
}

impl<'a, const N: usize> BoundedSession<'a, N> {
    /// Validate connection configuration without constructing replay caches.
    /// # Errors
    /// Rejects zero IDs, invalid limits and invalid or inconsistent catalogues.
    pub fn validate_config(config: &Config<'_>) -> Result<(), ConfigError> {
        validate_config(config)
    }

    /// Create a fresh connection, initially with notifications disabled.
    /// # Errors
    /// Rejects zero IDs, invalid limits and invalid or inconsistent catalogues.
    pub fn new(config: Config<'a>) -> Result<Self, ConfigError> {
        validate_config(&config)?;
        if usize::from(config.limits.control_max) > N || N > LOGICAL_MAX {
            return Err(ConfigError::InvalidLimits);
        }
        Ok(Self {
            config,
            state: SessionState::AwaitingNotifications,
            limits: config.limits,
            last_header: None,
            pending_header: None,
            cached_request: [0; N],
            cached_response: [0; N],
            response_length: 0,
        })
    }

    /// Enable the control subscription on this same connection.
    /// Repeated enable notifications are idempotent while still open.
    /// # Errors
    /// Closed/faulted sessions require a new physical connection and new instance.
    pub fn control_notifications_enabled(&mut self) -> Result<(), SessionError> {
        match self.state {
            SessionState::AwaitingNotifications => self.state = SessionState::AwaitingHello,
            SessionState::AwaitingHello | SessionState::Active => {}
            SessionState::Closed => return Err(SessionError::Closed),
            SessionState::Faulted => return Err(SessionError::Faulted),
        }
        Ok(())
    }

    /// Disable notifications and permanently end this connection's app session.
    pub fn control_notifications_disabled(&mut self) {
        self.end(SessionState::Closed);
    }

    /// End a physical connection and discard the last request/reply cache.
    pub fn disconnect(&mut self) {
        self.end(SessionState::Closed);
    }

    /// Observe lifecycle without exposing request or cached response bytes.
    #[must_use]
    pub const fn state(&self) -> SessionState {
        self.state
    }

    /// Actual HELLO-negotiated bounds, available only while this application session is active.
    /// Event adapters must use these bounds rather than the original compiled maxima.
    #[must_use]
    pub fn active_limits(&self) -> Option<ProtocolLimits> {
        (self.state == SessionState::Active).then_some(self.limits)
    }

    /// Dispatch anonymously, returning complete response-body length.
    /// # Errors
    /// Returns local lifecycle, malformed-frame, replay-conflict or output-capacity
    /// errors without a wire response. Valid request failures return `Ok(4)` with
    /// an exact common status/reason; HELLO does not confer access.
    pub fn dispatch(&mut self, frame: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        self.dispatch_with_access(frame, &AnonymousAccess, out)
    }

    /// Dispatch through a trusted resource-scoped handler without bypassing ID/cache rules.
    /// Core status remains anonymous; handler authorization is independent.
    /// # Errors
    /// Same lifecycle/structure/capacity errors as dispatch; revoked scope closes the session.
    pub fn dispatch_with_handler(
        &mut self,
        frame: Frame<'_>,
        handler: &mut dyn ServiceHandler,
        out: &mut [u8],
    ) -> Result<usize, SessionError> {
        self.dispatch_with_typed_handler(frame, handler, out)
    }

    /// Dispatch without erasing a concrete handler's type; wire and cache rules are identical.
    /// Existing trait-object callers may continue using `dispatch_with_handler`.
    /// # Errors
    /// Same lifecycle, capacity and authorization failures as `dispatch_with_handler`.
    pub fn dispatch_with_typed_handler<H: ServiceHandler + ?Sized>(
        &mut self,
        frame: Frame<'_>,
        handler: &mut H,
        out: &mut [u8],
    ) -> Result<usize, SessionError> {
        let snapshot = handler
            .informational_access()
            .unwrap_or_else(|| AnonymousAccess.access_snapshot());
        self.dispatch_inner(frame, &SnapshotAccess(snapshot), Some(handler), out)
    }

    /// Dispatch using integration-supplied access state, never request claims.
    ///
    /// Even `Authorized` status is insufficient for GRANT/DATASET: until actual
    /// resource providers and scoped role checks are integrated, both return
    /// `ACCESS_DENIED / FF03`. No fake stream acceptance or dataset ID is returned.
    /// # Errors
    /// Same local failures as [`Self::dispatch`]. Insufficient caller capacity
    /// does not consume the ID, activate HELLO, overwrite the cache or write bytes.
    pub fn dispatch_with_access(
        &mut self,
        frame: Frame<'_>,
        access: &dyn TrustedAccessProvider,
        out: &mut [u8],
    ) -> Result<usize, SessionError> {
        self.dispatch_inner::<dyn ServiceHandler>(frame, access, None, out)
    }

    fn dispatch_inner<H: ServiceHandler + ?Sized>(
        &mut self,
        frame: Frame<'_>,
        access: &dyn TrustedAccessProvider,
        handler: Option<&mut H>,
        out: &mut [u8],
    ) -> Result<usize, SessionError> {
        self.check_open()?;
        if handler.as_ref().is_some_and(|h| h.must_end_session()) {
            self.end(SessionState::Closed);
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        if frame.header.kind != Kind::Request
            || frame.header.message_id == 0
            || frame.header.offset != 0
            || usize::from(frame.header.total_length) != frame.payload.len()
            || frame.payload.len() > N
        {
            self.end(SessionState::Faulted);
            return Err(SessionError::InvalidFrame);
        }
        let cached = if let Some(pending) = self.pending_header {
            if frame.header != pending
                || frame.payload != &self.cached_request[..usize::from(pending.total_length)]
            {
                self.end(SessionState::Faulted);
                return Err(SessionError::RequestConflict);
            }
            false
        } else {
            self.is_replay(frame)?
        };
        if is_routed_request(frame) {
            if let Some(handler) = handler {
                if cached && self.cached_failure() {
                    return self.copy_cached(out);
                }
                return self.dispatch_business(frame, handler, out, cached);
            }
            // A no-handler entry point must never release a business success
            // previously cached through a resource-authorized entry point.
            if cached {
                if out.len() < STATUS_LENGTH {
                    return Err(SessionError::Capacity);
                }
                self.encode_reply(
                    Reply::Status(Status::AccessDenied, Reason::RoleRequired),
                    &mut out[..STATUS_LENGTH],
                )?;
                return Ok(STATUS_LENGTH);
            }
        }
        if cached {
            return self.copy_cached(out);
        }
        let reply = self.prepare(frame, access);
        let length = reply.length();
        if out.len() < length {
            return Err(SessionError::Capacity);
        }
        self.encode_reply(reply, &mut out[..length])?;
        if let Reply::Hello(hello) = reply {
            self.limits.control_max = hello.control_max;
            self.limits.data_max = hello.data_max;
            self.state = SessionState::Active;
        }
        self.remember(frame, &out[..length]);
        Ok(length)
    }

    fn remember(&mut self, frame: Frame<'_>, response: &[u8]) {
        self.cached_request[..frame.payload.len()].copy_from_slice(frame.payload);
        self.cached_response[..response.len()].copy_from_slice(response);
        self.response_length = response.len();
        self.last_header = Some(frame.header);
        self.pending_header = None;
    }

    fn copy_cached(&self, out: &mut [u8]) -> Result<usize, SessionError> {
        if out.len() < self.response_length {
            return Err(SessionError::Capacity);
        }
        out[..self.response_length].copy_from_slice(&self.cached_response[..self.response_length]);
        Ok(self.response_length)
    }

    fn dispatch_business<H: ServiceHandler + ?Sized>(
        &mut self,
        frame: Frame<'_>,
        handler: &mut H,
        out: &mut [u8],
        replay: bool,
    ) -> Result<usize, SessionError> {
        let entry = self
            .config
            .modules
            .iter()
            .find(|entry| entry.module == frame.header.module)
            .copied();
        let mut rejection = None;
        if self.state != SessionState::Active
            || (frame.header.module == 0 && frame.header.opcode == 7 && self.limits.data_max == 0)
        {
            rejection = Some((Status::InvalidState, Reason::None));
        } else if let Some(entry) = entry {
            if entry.availability != 1 {
                rejection = Some((
                    if entry.availability == 2 {
                        Status::HardwareFailure
                    } else {
                        Status::InvalidState
                    },
                    Reason::None,
                ));
            } else if !handler.authorized(frame) {
                if replay || self.pending_header.is_some() || handler.must_end_session() {
                    self.end(SessionState::Closed);
                    out.fill(0);
                    return Err(SessionError::AuthorizationRevoked);
                }
                rejection = Some((Status::AccessDenied, Reason::RoleRequired));
            } else if handler
                .required_features(&entry, frame.header.opcode)
                .is_none_or(|required| entry.features & required != required)
            {
                rejection = Some((Status::UnsupportedOpcode, Reason::None));
            } else if frame.payload.len()
                > usize::from(entry.input_max.min(self.limits.control_max))
            {
                rejection = Some((Status::InvalidArgument, Reason::None));
            }
        } else {
            rejection = Some((Status::UnsupportedModule, Reason::None));
        }
        if let Some((status, reason)) = rejection {
            if out.len() < STATUS_LENGTH {
                return Err(SessionError::Capacity);
            }
            self.encode_reply(Reply::Status(status, reason), &mut out[..STATUS_LENGTH])?;
            if !replay {
                self.remember(frame, &out[..STATUS_LENGTH]);
            }
            return Ok(STATUS_LENGTH);
        }
        if replay {
            return self.copy_handler_replay(frame, handler, out);
        }
        // Entry and handler were actually verified above. No configured output
        // maximum may be bypassed by returning a short apparent success.
        let Some(entry) = entry else {
            return Err(SessionError::InvalidHandlerResponse);
        };
        let limit = usize::from(entry.output_max.min(self.limits.control_max));
        if limit < STATUS_LENGTH {
            if out.len() < STATUS_LENGTH {
                return Err(SessionError::Capacity);
            }
            self.encode_reply(
                Reply::error(Status::LimitExceeded),
                &mut out[..STATUS_LENGTH],
            )?;
            self.remember(frame, &out[..STATUS_LENGTH]);
            return Ok(STATUS_LENGTH);
        }
        if out.len() < limit {
            return Err(SessionError::Capacity);
        }
        self.check_handler_readiness(frame, handler, out)?;
        let mut pending = [0xa5; N];
        let length = match handler.dispatch_ready(frame, &mut pending[..limit]) {
            Ok(length) => length,
            Err(SessionError::Capacity) => return Err(SessionError::Capacity),
            Err(SessionError::AuthorizationRevoked) => {
                self.end(SessionState::Closed);
                out.fill(0);
                return Err(SessionError::AuthorizationRevoked);
            }
            Err(error) => {
                self.end(SessionState::Faulted);
                return Err(error);
            }
        };
        if !handler.authorized(frame) || handler.must_end_session() {
            self.end(SessionState::Closed);
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        if !valid_service_response(&pending, length, limit) {
            self.end(SessionState::Faulted);
            return Err(SessionError::InvalidHandlerResponse);
        }
        out[..length].copy_from_slice(&pending[..length]);
        self.remember(frame, &out[..length]);
        Ok(length)
    }

    // Exact failure replays contain no protected data and confer no authority.
    fn cached_failure(&self) -> bool {
        self.response_length == STATUS_LENGTH && self.cached_response[..2] != [0, 0]
    }

    fn copy_handler_replay<H: ServiceHandler + ?Sized>(
        &mut self,
        frame: Frame<'_>,
        handler: &H,
        out: &mut [u8],
    ) -> Result<usize, SessionError> {
        if !handler.replay_valid(frame, &self.cached_response[..self.response_length]) {
            self.end(SessionState::Closed);
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        self.copy_cached(out)
    }
    #[allow(
        clippy::inline_always,
        reason = "Exact ARM envelope shares serial readiness and response scratch"
    )]
    #[inline(always)]
    fn check_handler_readiness<H: ServiceHandler + ?Sized>(
        &mut self,
        frame: Frame<'_>,
        handler: &mut H,
        out: &mut [u8],
    ) -> Result<(), SessionError> {
        let ready = match handler.response_ready(frame) {
            Ok(ready) => ready,
            Err(SessionError::AuthorizationRevoked) => {
                self.end(SessionState::Closed);
                out.fill(0);
                return Err(SessionError::AuthorizationRevoked);
            }
            Err(error) => {
                self.end(SessionState::Faulted);
                out.fill(0);
                return Err(error);
            }
        };
        if !handler.authorized(frame) || handler.must_end_session() {
            self.end(SessionState::Closed);
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        if !ready {
            self.pending_header = Some(frame.header);
            self.cached_request.fill(0);
            self.cached_request[..frame.payload.len()].copy_from_slice(frame.payload);
            self.cached_response.fill(0);
            self.response_length = 0;
            out.fill(0);
            return Err(SessionError::Deferred);
        }
        Ok(())
    }

    fn check_open(&self) -> Result<(), SessionError> {
        match self.state {
            SessionState::AwaitingNotifications => Err(SessionError::NotificationsRequired),
            SessionState::Closed => Err(SessionError::Closed),
            SessionState::Faulted => Err(SessionError::Faulted),
            SessionState::AwaitingHello | SessionState::Active => Ok(()),
        }
    }

    fn end(&mut self, state: SessionState) {
        self.state = state;
        self.last_header = None;
        self.pending_header = None;
        self.response_length = 0;
        self.cached_request.fill(0);
        self.cached_response.fill(0);
    }

    fn is_replay(&mut self, frame: Frame<'_>) -> Result<bool, SessionError> {
        let Some(last) = self.last_header else {
            if frame.header.message_id != 1 {
                self.end(SessionState::Faulted);
                return Err(SessionError::RequestOrder);
            }
            return Ok(false);
        };
        if frame.header.message_id == last.message_id {
            if frame.header == last
                && frame.payload == &self.cached_request[..usize::from(last.total_length)]
            {
                return Ok(true);
            }
            self.end(SessionState::Faulted);
            return Err(SessionError::RequestConflict);
        }
        if frame.header.message_id < last.message_id {
            self.end(SessionState::Faulted);
            return Err(SessionError::RequestOrder);
        }
        Ok(false)
    }

    fn prepare(&self, frame: Frame<'_>, access: &dyn TrustedAccessProvider) -> Reply {
        if self.state != SessionState::Active
            && (frame.header.module != 0 || frame.header.opcode != 1)
        {
            return Reply::error(Status::InvalidState);
        }
        if frame.header.module != 0 {
            return if self
                .config
                .modules
                .iter()
                .any(|entry| entry.module == frame.header.module)
            {
                Reply::Status(Status::AccessDenied, Reason::RoleRequired)
            } else {
                Reply::error(Status::UnsupportedModule)
            };
        }
        // Permission checks precede protected-command parsing and metadata lookup.
        if matches!(frame.header.opcode, 7 | 8) {
            return Reply::Status(Status::AccessDenied, Reason::RoleRequired);
        }
        if frame.payload.len() > usize::from(self.limits.control_max) {
            return Reply::error(Status::InvalidArgument);
        }
        match frame.header.opcode {
            1 => self.prepare_hello(frame.payload, access),
            2 if frame.payload.is_empty() => Reply::Info(self.config.info),
            3 => self.prepare_modules(frame.payload),
            4 if frame.payload.is_empty() => Reply::Limits(self.limits),
            5 if frame.payload.len() == 4 => Reply::Ping([
                frame.payload[0],
                frame.payload[1],
                frame.payload[2],
                frame.payload[3],
            ]),
            6 if frame.payload.is_empty() => Reply::Access(access.access_snapshot()),
            2 | 4 | 5 | 6 => Reply::error(Status::InvalidArgument),
            _ => Reply::error(Status::UnsupportedOpcode),
        }
    }

    fn prepare_hello(&self, payload: &[u8], access: &dyn TrustedAccessProvider) -> Reply {
        if self.state == SessionState::Active {
            return Reply::error(Status::InvalidState);
        }
        let request = match HelloRequest::decode(payload) {
            Ok(request) => request,
            Err(ProtocolError::InvalidVersion) => return Reply::error(Status::VersionMismatch),
            Err(_) => return Reply::error(Status::InvalidArgument),
        };
        Reply::Hello(HelloResponse {
            major: 1,
            minor: 2,
            session: self.config.session,
            control_max: self.config.limits.control_max.min(request.rx_control_max),
            data_max: self.config.limits.data_max.min(request.rx_data_max),
            inflight: 1,
            profile: self.config.profile as u8,
            access: access.access_snapshot().state as u8,
            reserved: 0,
            capability_rev: self.config.capability_rev,
        })
    }

    fn prepare_modules(&self, payload: &[u8]) -> Reply {
        if payload.len() != 3 {
            return Reply::error(Status::InvalidArgument);
        }
        let start = u16::from_le_bytes([payload[0], payload[1]]);
        let max = payload[2];
        if start == u16::MAX
            || !(1..=8).contains(&max)
            || usize::from(start) > self.config.modules.len()
        {
            return Reply::error(Status::InvalidArgument);
        }
        let remaining = self.config.modules.len() - usize::from(start);
        let capacity =
            (usize::from(self.limits.control_max) - MODULE_PAGE_HEADER) / ModuleEntry::LENGTH;
        let count = remaining.min(usize::from(max)).min(capacity);
        if remaining != 0 && count == 0 {
            return Reply::error(Status::LimitExceeded);
        }
        Reply::Modules {
            start: usize::from(start),
            count,
        }
    }

    fn encode_reply(&self, reply: Reply, out: &mut [u8]) -> Result<(), SessionError> {
        let (status, reason) = if let Reply::Status(status, reason) = reply {
            (status, reason)
        } else {
            (Status::Ok, Reason::None)
        };
        out[..2].copy_from_slice(&(status as u16).to_le_bytes());
        out[2..4].copy_from_slice(&(reason as u16).to_le_bytes());
        match reply {
            Reply::Status(_, _) => {}
            Reply::Hello(value) => {
                value.encode(&mut out[4..]).map_err(SessionError::Codec)?;
            }
            Reply::Info(value) => {
                value.encode(&mut out[4..]).map_err(SessionError::Codec)?;
            }
            Reply::Limits(value) => {
                value.encode(&mut out[4..]).map_err(SessionError::Codec)?;
            }
            Reply::Ping(value) => out[4..8].copy_from_slice(&value),
            Reply::Access(value) => {
                out[4] = value.state as u8;
                out[5..7].copy_from_slice(&value.reason.to_le_bytes());
            }
            Reply::Modules { start, count } => self.encode_modules(start, count, out)?,
        }
        Ok(())
    }

    fn encode_modules(
        &self,
        start: usize,
        count: usize,
        out: &mut [u8],
    ) -> Result<(), SessionError> {
        let end = start + count;
        let next = if end == self.config.modules.len() {
            u16::MAX
        } else {
            u16::try_from(end).map_err(|_| SessionError::Codec(ProtocolError::InvalidLength))?
        };
        out[4..8].copy_from_slice(&self.config.capability_rev.to_le_bytes());
        out[8..10].copy_from_slice(&next.to_le_bytes());
        out[10] =
            u8::try_from(count).map_err(|_| SessionError::Codec(ProtocolError::InvalidLength))?;
        for (entry, bytes) in self.config.modules[start..end]
            .iter()
            .zip(out[MODULE_PAGE_HEADER..].chunks_exact_mut(ModuleEntry::LENGTH))
        {
            // A single catalogue maximum covers response/event output. Applying
            // the smaller nonzero negotiated plane is conservative for both;
            // integrations must obey these returned per-connection caps.
            let output_cap = if self.limits.data_max == 0 {
                self.limits.control_max
            } else {
                self.limits.control_max.min(self.limits.data_max)
            };
            ModuleEntry {
                input_max: entry.input_max.min(self.limits.control_max),
                output_max: entry.output_max.min(output_cap),
                ..*entry
            }
            .encode(bytes)
            .map_err(SessionError::Codec)?;
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Reply {
    Status(Status, Reason),
    Hello(HelloResponse),
    Info(DeviceInfo),
    Modules { start: usize, count: usize },
    Limits(ProtocolLimits),
    Ping([u8; 4]),
    Access(AccessSnapshot),
}
impl Reply {
    fn error(status: Status) -> Self {
        Self::Status(status, Reason::None)
    }
    fn length(self) -> usize {
        match self {
            Self::Status(_, _) => STATUS_LENGTH,
            Self::Hello(_) => STATUS_LENGTH + HelloResponse::LENGTH,
            Self::Info(_) => STATUS_LENGTH + DeviceInfo::LENGTH,
            Self::Modules { count, .. } => MODULE_PAGE_HEADER + count * ModuleEntry::LENGTH,
            Self::Limits(_) => STATUS_LENGTH + ProtocolLimits::LENGTH,
            Self::Ping(_) => STATUS_LENGTH + 4,
            Self::Access(_) => STATUS_LENGTH + 3,
        }
    }
}

fn valid_service_response(bytes: &[u8], length: usize, limit: usize) -> bool {
    let status = u16::from_le_bytes([bytes[0], bytes[1]]);
    let reason = u16::from_le_bytes([bytes[2], bytes[3]]);
    (STATUS_LENGTH..=limit).contains(&length)
        && status <= 0x12
        && status != Status::Pending as u16
        && (status == 0 || length == STATUS_LENGTH)
        && (status != 0 || reason == 0)
}

fn validate_config(config: &Config<'_>) -> Result<(), ConfigError> {
    if config.session == 0 {
        return Err(ConfigError::InvalidSession);
    }
    if config.capability_rev == 0 {
        return Err(ConfigError::InvalidCapabilityRevision);
    }
    config
        .limits
        .encode(&mut [0; ProtocolLimits::LENGTH])
        .map_err(|_| ConfigError::InvalidLimits)?;
    let Some(core) = config.modules.first() else {
        return Err(ConfigError::InvalidModules);
    };
    if core.module != 0
        || core.major != 1
        || core.minor != 2
        || core.availability != 1
        || core.features != 0
    {
        return Err(ConfigError::InvalidModules);
    }
    let maximum_page = MODULE_PAGE_HEADER + config.modules.len().min(8) * ModuleEntry::LENGTH;
    if core.input_max < 8 || usize::from(core.output_max) < maximum_page {
        return Err(ConfigError::InvalidModules);
    }
    let mut previous = None;
    for entry in config.modules {
        if previous.is_some_and(|module| entry.module <= module)
            || entry.input_max > config.limits.control_max
            || entry.output_max > config.limits.control_max.max(config.limits.data_max)
            || entry.encode(&mut [0; ModuleEntry::LENGTH]).is_err()
        {
            return Err(ConfigError::InvalidModules);
        }
        previous = Some(entry.module);
    }
    Ok(())
}

fn is_routed_request(frame: Frame<'_>) -> bool {
    frame.header.module != 0 || matches!(frame.header.opcode, 7 | 8)
}
#[derive(Debug)]
struct SnapshotAccess(AccessSnapshot);
impl TrustedAccessProvider for SnapshotAccess {
    fn access_snapshot(&self) -> AccessSnapshot {
        self.0
    }
}
