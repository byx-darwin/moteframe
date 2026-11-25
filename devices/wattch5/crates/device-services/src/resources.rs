//! Scoped core GRANT/DATASET handlers. Access-state reporting is never authorization.
use crate::{AccessSnapshot, Reason, ServiceHandler, SessionError, Status, TrustedAccessProvider};
use device_protocol::{DataKind, Frame, Kind, ModuleEntry};
/// A complete valid credit request, independent of chip and GATT handles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrantRequest {
    /// History or explicitly compiled development raw stream.
    pub kind: DataKind,
    /// Nonzero connection-owned stream identity.
    pub stream_id: u16,
    /// Cumulative whole-message credit, not an increment.
    pub cumulative: u32,
}
/// Protected core resource operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoreRequest {
    /// Current history/raw stream credit.
    Grant(GrantRequest),
    /// Actual persistent dataset identity.
    Dataset,
}
impl CoreRequest {
    /// Decode exact module00 opcode07/08 business parameters.
    /// # Errors
    /// Invalid fields, unsupported kind or incorrect exact body length.
    pub fn decode(opcode: u8, body: &[u8]) -> Result<Self, ResourceFailure> {
        match opcode {
            7 => {
                if body.len() != 7 {
                    return Err(ResourceFailure::InvalidArgument);
                }
                let kind = match body[0] {
                    1 => DataKind::History,
                    #[cfg(feature = "raw-data")]
                    2 => DataKind::Raw,
                    _ => return Err(ResourceFailure::EnumUnsupported),
                };
                let stream_id = u16::from_le_bytes([body[1], body[2]]);
                if stream_id == 0 {
                    return Err(ResourceFailure::InvalidArgument);
                }
                Ok(Self::Grant(GrantRequest {
                    kind,
                    stream_id,
                    cumulative: u32::from_le_bytes([body[3], body[4], body[5], body[6]]),
                }))
            }
            8 if body.is_empty() => Ok(Self::Dataset),
            8 => Err(ResourceFailure::InvalidArgument),
            _ => Err(ResourceFailure::Unsupported),
        }
    }
}
/// Persistent dataset identity obtained from a verified backend. Zero is never a valid placeholder.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DatasetIdentity([u8; 16]);
impl core::fmt::Debug for DatasetIdentity {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DatasetIdentity")
            .field("length", &16)
            .finish_non_exhaustive()
    }
}
impl DatasetIdentity {
    /// Validate backend identity. This does not create or persist a dataset.
    /// # Errors
    /// A zero identity is a backend contract error, not successful anonymous data.
    pub fn new(bytes: [u8; 16]) -> Result<Self, ResourceFailure> {
        if bytes == [0; 16] {
            Err(ResourceFailure::InvalidIdentity)
        } else {
            Ok(Self(bytes))
        }
    }
    /// Borrow the verified encoded identity.
    #[must_use]
    pub const fn bytes(&self) -> &[u8; 16] {
        &self.0
    }
}
/// Failures mapped to the existing core status/reason dictionary only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceFailure {
    /// Invalid body or numeric range.
    InvalidArgument,
    /// Unsupported kind enumeration.
    EnumUnsupported,
    /// No implementation for this resource.
    Unsupported,
    /// Current stream/state is unavailable.
    InvalidState,
    /// Required trusted role/scope is absent.
    AccessDenied,
    /// Resource is already in use.
    Busy,
    /// Identifier/credit capacity exhausted.
    LimitExceeded,
    /// Actual stream/identity is not found.
    NotFound,
    /// Actual storage operation failed.
    StorageFailure,
    /// Stored identity metadata integrity failed.
    StoredIntegrity,
    /// Physical resource failed.
    HardwareFailure,
    /// Decreasing credit or different resource binding.
    Conflict,
    /// Backend supplied a zero or otherwise impossible identity/result.
    InvalidIdentity,
}
impl ResourceFailure {
    fn wire(self) -> (Status, Reason) {
        match self {
            Self::EnumUnsupported => (Status::InvalidArgument, Reason::EnumUnsupported),
            Self::AccessDenied => (Status::AccessDenied, Reason::RoleRequired),
            Self::StoredIntegrity => (Status::IntegrityFailure, Reason::StoredCrcMismatch),
            other => (
                match other {
                    Self::InvalidArgument => Status::InvalidArgument,
                    Self::Unsupported => Status::UnsupportedOpcode,
                    Self::InvalidState => Status::InvalidState,
                    Self::Busy => Status::Busy,
                    Self::LimitExceeded => Status::LimitExceeded,
                    Self::NotFound => Status::NotFound,
                    Self::StorageFailure => Status::StorageFailure,
                    Self::HardwareFailure => Status::HardwareFailure,
                    Self::Conflict => Status::Conflict,
                    Self::InvalidIdentity => Status::InternalFailure,
                    Self::EnumUnsupported | Self::AccessDenied | Self::StoredIntegrity => {
                        Status::InternalFailure
                    }
                },
                Reason::None,
            ),
        }
    }
}
/// Trusted resource owner/identity bridge, separate from informational access state.
/// Every provider must recheck actual scope and binding inside its serialized grant/read boundary.
/// There is no default successful implementation or generated persistent identity.
pub trait CoreResources: TrustedAccessProvider {
    /// Actual per-resource authorization; never infer this from reported Authorized/profile alone.
    fn authorized(&self, request: CoreRequest) -> bool;
    /// Verify the current stream kind/connection binding or real dataset readiness without mutation.
    /// # Errors
    /// Return actual missing/expired/busy/failed context instead of pretending the resource exists.
    fn validate(&self, request: CoreRequest) -> Result<(), ResourceFailure>;
    /// Atomically accept monotonic cumulative credit in the current real stream context.
    /// # Errors
    /// Scope/context/credit failure leaves old credit intact. Recheck inside the same actor boundary.
    fn grant(&mut self, request: GrantRequest) -> Result<u32, ResourceFailure>;
    /// Read an actual verified persistent identity; this method must never allocate an ephemeral one.
    /// # Errors
    /// Permission/storage/integrity/unavailable identity failure, never a zero placeholder.
    fn dataset(&mut self) -> Result<DatasetIdentity, ResourceFailure>;
    /// Side-effect-free check before releasing an accepted cached or queued response.
    /// Verify current stream generation/binding, or that dataset bytes still name this dataset/owner.
    /// `accepted_body` excludes the four-byte public status prefix. False ends the session.
    fn replay_valid(&self, request: CoreRequest, accepted_body: &[u8]) -> bool;
}
/// Invalid core resource handler registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourceConfigError {
    /// Core00/v1.2 metadata or capacities do not match implemented commands.
    InvalidMetadata,
}
/// Module00 resource extension; ordinary HELLO/INFO/MODULES/LIMITS/PING/ACCESS stay in Session.
pub struct CoreResourceHandler<'a, P: CoreResources> {
    metadata: ModuleEntry,
    provider: &'a mut P,
    closed: bool,
}
impl<P: CoreResources> core::fmt::Debug for CoreResourceHandler<'_, P> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CoreResourceHandler")
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}
impl<'a, P: CoreResources> CoreResourceHandler<'a, P> {
    /// Bind a real provider and immutable core capability snapshot.
    /// # Errors
    /// Rejects unsupported metadata, unknown feature bits or insufficient capacities.
    pub fn new(metadata: ModuleEntry, provider: &'a mut P) -> Result<Self, ResourceConfigError> {
        if metadata.module != 0
            || metadata.major != 1
            || metadata.minor != 2
            || metadata.availability != 1
            || metadata.features != 0
            || metadata.input_max < 7
            || metadata.input_max > 512
            || metadata.output_max < 20
            || metadata.output_max > 512
        {
            return Err(ResourceConfigError::InvalidMetadata);
        }
        Ok(Self {
            metadata,
            provider,
            closed: false,
        })
    }
    fn status(out: &mut [u8], status: Status, reason: Reason) -> Result<usize, SessionError> {
        if out.len() < 4 {
            return Err(SessionError::Capacity);
        }
        out[..2].copy_from_slice(&(status as u16).to_le_bytes());
        out[2..4].copy_from_slice(&(reason as u16).to_le_bytes());
        Ok(4)
    }
    fn fail(&mut self, out: &mut [u8], error: ResourceFailure) -> Result<usize, SessionError> {
        if error == ResourceFailure::AccessDenied {
            self.closed = true;
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        let (status, reason) = error.wire();
        Self::status(out, status, reason)
    }
}
impl<P: CoreResources> ServiceHandler for CoreResourceHandler<'_, P> {
    fn informational_access(&self) -> Option<AccessSnapshot> {
        Some(self.provider.access_snapshot())
    }
    fn must_end_session(&self) -> bool {
        self.closed
    }
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        self.closed = true;
        Ok(())
    }
    fn required_features(&self, module: &ModuleEntry, opcode: u8) -> Option<u32> {
        (*module == self.metadata && matches!(opcode, 7 | 8)).then_some(0)
    }
    fn authorized(&self, request: Frame<'_>) -> bool {
        !self.closed
            && request.header.module == 0
            && CoreRequest::decode(request.header.opcode, request.payload)
                .map_or(true, |request| self.provider.authorized(request))
    }
    fn replay_valid(&self, request: Frame<'_>, response: &[u8]) -> bool {
        if self.closed || response.len() < 4 {
            return false;
        }
        if response[..2] != [0, 0] {
            return true;
        }
        let Ok(operation) = CoreRequest::decode(request.header.opcode, request.payload) else {
            return false;
        };
        self.provider.authorized(operation)
            && self.provider.validate(operation).is_ok()
            && self.provider.replay_valid(operation, &response[4..])
    }
    fn dispatch(&mut self, request: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        if self.closed {
            return Err(SessionError::AuthorizationRevoked);
        }
        if request.header.kind != Kind::Request
            || request.header.module != 0
            || request.header.message_id == 0
            || request.header.offset != 0
            || usize::from(request.header.total_length) != request.payload.len()
        {
            return Err(SessionError::InvalidHandlerResponse);
        }
        let operation = match CoreRequest::decode(request.header.opcode, request.payload) {
            Ok(value) => value,
            Err(error) => {
                let (status, reason) = error.wire();
                return Self::status(out, status, reason);
            }
        };
        if !self.provider.authorized(operation) {
            return Self::status(out, Status::AccessDenied, Reason::RoleRequired);
        }
        let length = match operation {
            CoreRequest::Grant(_) => 11,
            CoreRequest::Dataset => 20,
        };
        if out.len() < length {
            return Err(SessionError::Capacity);
        }
        if let Err(error) = self.provider.validate(operation) {
            return self.fail(out, error);
        }
        if !self.provider.authorized(operation) {
            self.closed = true;
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        match operation {
            CoreRequest::Grant(grant) => {
                let accepted = match self.provider.grant(grant) {
                    Ok(value) => value,
                    Err(error) => return self.fail(out, error),
                };
                if accepted > grant.cumulative {
                    self.closed = true;
                    out.fill(0);
                    return Err(SessionError::InvalidHandlerResponse);
                }
                out[4] = match grant.kind {
                    DataKind::History => 1,
                    DataKind::Raw => 2,
                };
                out[5..7].copy_from_slice(&grant.stream_id.to_le_bytes());
                out[7..11].copy_from_slice(&accepted.to_le_bytes());
            }
            CoreRequest::Dataset => {
                let identity = match self.provider.dataset() {
                    Ok(value) => value,
                    Err(error) => return self.fail(out, error),
                };
                out[4..20].copy_from_slice(identity.bytes());
            }
        }
        if !self.provider.authorized(operation) {
            self.closed = true;
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        Self::status(out, Status::Ok, Reason::None)?;
        Ok(length)
    }
}
