//! Generic property adapter; no product fields, persistence or physical backend.
use crate::{Reason, ServiceHandler, SessionError, Status};
use device_protocol::{Frame, Kind, LOGICAL_MAX, ModuleEntry, PropertyRequest};

/// Resource operation, independent of build profile or request claims.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyOperation {
    /// Read a complete property snapshot.
    Get,
    /// Atomically validate and update a property.
    Set,
    /// Read a static module directory.
    Describe,
}
/// Integration-owned live authorization; no default permission is supplied.
pub trait PropertyAccess {
    /// `None` checks module permission before parsing; `Some` checks concrete
    /// resource scope. Revocation must remain visible through commit and send.
    fn allowed(&self, module: u8, operation: PropertyOperation, key: Option<u16>) -> bool;
}
/// Connection-static module-local DESCRIBE entry. No value is contained.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PropertyDescriptor {
    /// Sorted unique key, never FFFF.
    pub key: u16,
    /// bit0 read, bit1 write, bit2 object, bit3 persistent; other bits zero.
    pub access: u8,
    /// Actual backend-compiled schema identifier.
    pub schema: u16,
    /// Maximum complete encoded value length.
    pub value_max: u16,
}
/// Consistent read snapshot; bytes are copied separately into caller storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PropertyValue {
    /// Nonzero current property revision.
    pub revision: u32,
    /// Actual bytes copied, never padded output capacity.
    pub length: usize,
}
/// Successfully committed transition; neither revision may be zero or wrap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RevisionChange {
    /// Revision observed at the atomic commit boundary.
    pub previous: u32,
    /// Exactly previous+1, after successful commit.
    pub current: u32,
}
/// Backend failure translated to exact status without private error details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyFailure {
    /// Schema or value validation failed.
    InvalidValue,
    /// Property is absent.
    NotFound,
    /// Required live resource permission is absent.
    AccessDenied,
    /// Expected revision differs at commit.
    Conflict,
    /// Capacity is insufficient or revision cannot advance without wrapping.
    LimitExceeded,
    /// Revision cannot advance without wrapping.
    RevisionExhausted,
    /// Backend is busy.
    Busy,
    /// Persistent storage failed or durability is uncertain. Recover the backend
    /// and query the property revision before retrying an uncertain write.
    StorageFailure,
    /// Physical hardware failed.
    HardwareFailure,
    /// Stored data failed its schema/integrity check.
    IntegrityFailure,
    /// Backend violated its declared response contract.
    InternalFailure,
}
impl PropertyFailure {
    /// Existing wire status and reason for this failure, without private details.
    #[must_use]
    pub const fn status(self) -> (Status, Reason) {
        let status = match self {
            Self::InvalidValue => Status::InvalidArgument,
            Self::NotFound => Status::NotFound,
            Self::AccessDenied => return (Status::AccessDenied, Reason::RoleRequired),
            Self::Conflict => Status::Conflict,
            Self::LimitExceeded | Self::RevisionExhausted => Status::LimitExceeded,
            Self::Busy => Status::Busy,
            Self::StorageFailure => Status::StorageFailure,
            Self::HardwareFailure => Status::HardwareFailure,
            Self::IntegrityFailure => Status::IntegrityFailure,
            Self::InternalFailure => Status::InternalFailure,
        };
        (status, Reason::None)
    }
}
/// Backend contract; no fake data or successful defaults are supplied.
pub trait PropertyBackend {
    /// Sorted unique immutable directory for the connection. Schema/permission
    /// changes require ending the session and constructing a fresh adapter.
    fn catalogue(&self) -> &[PropertyDescriptor];
    /// Copy one consistent revision/value snapshot.
    /// # Errors
    /// Return actual storage/busy/hardware/lookup failure.
    fn read(&self, key: u16, out: &mut [u8]) -> Result<PropertyValue, PropertyFailure>;
    /// Validate the complete schema, including semantic ranges, without mutation.
    /// # Errors
    /// Reject invalid values before any side effect.
    fn validate_value(&self, key: u16, value: &[u8]) -> Result<(), PropertyFailure>;
    /// Single serialized commit boundary: recheck live permission, writable flag,
    /// schema, nonzero/nonwrapping revision and CAS, then commit. Expected zero
    /// permits a backend-approved blind property write. Validation/CAS failures
    /// preserve old state. An uncertain medium failure must block subsequent
    /// successful reads/writes until recovery; recovery may expose the complete
    /// old or new version, never a partial value or an invented successful write.
    /// Persistent entries succeed only after durable commit; the adapter cannot
    /// manufacture that guarantee. No queued or partial write may return success.
    /// # Errors
    /// Return the actual rejection or failure before committing.
    fn commit_atomic(
        &mut self,
        module: u8,
        key: u16,
        expected: u32,
        value: &[u8],
        access: &dyn PropertyAccess,
    ) -> Result<RevisionChange, PropertyFailure>;
}
/// Invalid adapter constructor input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyConfigError {
    /// Logical control maximum is outside 128..512.
    InvalidLimit,
    /// Unsorted/duplicate/sentinel key, reserved flag or oversized property.
    InvalidCatalogue,
}
/// Local transport/lifecycle rejection; no business response is fabricated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropertyDispatchError {
    /// Even the four-byte failure prefix cannot fit.
    Capacity,
    /// Caller supplied an incomplete or nonrequest logical frame.
    InvalidFrame,
    /// Permission changed during processing. Pending output is zeroed; caller
    /// must discard it and end the session/connection.
    AccessRevoked,
}
/// Borrowed adapter; transport and the Session business router remain separate.
pub struct PropertyService<'a, B: PropertyBackend> {
    module: u8,
    backend: &'a mut B,
    logical_max: usize,
}
impl<B: PropertyBackend> core::fmt::Debug for PropertyService<'_, B> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PropertyService")
            .field("module", &self.module)
            .field("logical_max", &self.logical_max)
            .finish_non_exhaustive()
    }
}
impl<'a, B: PropertyBackend> PropertyService<'a, B> {
    /// Validate the static directory and borrow the actual backend.
    /// # Errors
    /// Reject invalid limits, key ordering/FFFF, flags or property maximum.
    pub fn new(
        module: u8,
        backend: &'a mut B,
        logical_max: usize,
    ) -> Result<Self, PropertyConfigError> {
        if !(128..=LOGICAL_MAX).contains(&logical_max) {
            return Err(PropertyConfigError::InvalidLimit);
        }
        let mut previous = None;
        for entry in backend.catalogue() {
            if entry.key == u16::MAX
                || previous.is_some_and(|old| old >= entry.key)
                || entry.access & !15 != 0
                || (entry.access & 4 == 0 && usize::from(entry.value_max) > LOGICAL_MAX)
            {
                return Err(PropertyConfigError::InvalidCatalogue);
            }
            previous = Some(entry.key);
        }
        Ok(Self {
            module,
            backend,
            logical_max,
        })
    }
    /// Dispatch one complete request, returning exact status+success-body length.
    /// Capacity is checked before mutation. Caller serializes request IDs and must
    /// recheck live permission when actually enqueuing a delayed response.
    /// # Errors
    /// Reject malformed frames, insufficient prefix space or live revocation.
    pub fn dispatch(
        &mut self,
        frame: Frame<'_>,
        access: &dyn PropertyAccess,
        out: &mut [u8],
    ) -> Result<usize, PropertyDispatchError> {
        self.dispatch_bounded::<LOGICAL_MAX>(frame, access, out)
    }
    /// Use a compile-time staging bound without changing the configured logical limit.
    /// The legacy entry continues to support the full protocol maximum.
    /// # Errors
    /// A bound below the configured limit fails before backend access; other failures
    /// follow `dispatch`, including clearing tentative bytes on live revocation.
    pub fn dispatch_bounded<const N: usize>(
        &mut self,
        frame: Frame<'_>,
        access: &dyn PropertyAccess,
        out: &mut [u8],
    ) -> Result<usize, PropertyDispatchError> {
        if N < self.logical_max
            || self
                .backend
                .catalogue()
                .iter()
                .any(|entry| entry.access & 4 == 0 && usize::from(entry.value_max) > N)
        {
            return Err(PropertyDispatchError::Capacity);
        }
        if frame.header.kind != Kind::Request
            || frame.header.message_id == 0
            || frame.header.offset != 0
            || usize::from(frame.header.total_length) != frame.payload.len()
            || frame.payload.len() > self.logical_max
        {
            return Err(PropertyDispatchError::InvalidFrame);
        }
        if out.len() < 4 {
            return Err(PropertyDispatchError::Capacity);
        }
        if frame.header.module != self.module {
            return Ok(status(out, Status::UnsupportedModule, Reason::None));
        }
        let operation = match frame.header.opcode {
            0x10 => PropertyOperation::Get,
            0x11 => PropertyOperation::Set,
            0x12 => PropertyOperation::Describe,
            _ => return Ok(status(out, Status::UnsupportedOpcode, Reason::None)),
        };
        if !access.allowed(self.module, operation, None) {
            return Ok(status(out, Status::AccessDenied, Reason::RoleRequired));
        }
        let Ok(request) = PropertyRequest::decode(frame.header.opcode, frame.payload) else {
            return Ok(status(out, Status::InvalidArgument, Reason::None));
        };
        let key = match request {
            PropertyRequest::Get { key } | PropertyRequest::Set { key, .. } => Some(key),
            PropertyRequest::Describe { .. } => None,
        };
        if !access.allowed(self.module, operation, key) {
            return Ok(status(out, Status::AccessDenied, Reason::RoleRequired));
        }
        let limit = out.len().min(self.logical_max);
        let mut pending = [0; N];
        let result = match request {
            PropertyRequest::Get { key } => self.get(key, limit, &mut pending),
            PropertyRequest::Set {
                key,
                expected_revision,
                value,
            } => self.set(key, expected_revision, value, access, limit, &mut pending),
            PropertyRequest::Describe { start, max } => {
                self.describe(start, max, access, limit, &mut pending)
            }
        };
        if !access.allowed(self.module, operation, None)
            || !access.allowed(self.module, operation, key)
        {
            out.fill(0);
            return Err(PropertyDispatchError::AccessRevoked);
        }
        match result {
            Ok(length) => {
                if operation == PropertyOperation::Describe {
                    let next = u16::from_le_bytes([pending[4], pending[5]]);
                    if next != u16::MAX && !access.allowed(self.module, operation, Some(next)) {
                        out.fill(0);
                        return Err(PropertyDispatchError::AccessRevoked);
                    }
                    for entry in pending[7..length].chunks_exact(7) {
                        let key = u16::from_le_bytes([entry[0], entry[1]]);
                        if !access.allowed(self.module, operation, Some(key)) {
                            out.fill(0);
                            return Err(PropertyDispatchError::AccessRevoked);
                        }
                    }
                }
                out[..length].copy_from_slice(&pending[..length]);
                Ok(length)
            }
            Err(error) => {
                let (code, reason) = error.status();
                Ok(status(out, code, reason))
            }
        }
    }
    fn descriptor(&self, key: u16) -> Result<PropertyDescriptor, PropertyFailure> {
        self.backend
            .catalogue()
            .iter()
            .find(|entry| entry.key == key)
            .copied()
            .ok_or(PropertyFailure::NotFound)
    }
    fn get(&self, key: u16, limit: usize, out: &mut [u8]) -> Result<usize, PropertyFailure> {
        let entry = self.descriptor(key)?;
        if entry.access & 1 == 0 {
            return Err(PropertyFailure::AccessDenied);
        }
        if entry.access & 4 != 0 {
            return Err(PropertyFailure::InvalidValue);
        }
        // `out` is dispatch's private staging buffer. Preserve the backend's
        // full read capacity and publish nothing until its authority recheck.
        let snapshot = self.backend.read(key, out)?;
        if snapshot.revision == 0
            || snapshot.length > usize::from(entry.value_max)
            || snapshot.length > out.len()
        {
            return Err(PropertyFailure::InternalFailure);
        }
        self.backend
            .validate_value(key, &out[..snapshot.length])
            .map_err(|_| PropertyFailure::IntegrityFailure)?;
        let length = 12 + snapshot.length;
        if length > limit {
            return Err(PropertyFailure::LimitExceeded);
        }
        out.copy_within(0..snapshot.length, 12);
        out[..4].fill(0);
        out[4..8].copy_from_slice(&snapshot.revision.to_le_bytes());
        out[8..10].copy_from_slice(&key.to_le_bytes());
        out[10..12].copy_from_slice(
            &u16::try_from(snapshot.length)
                .map_err(|_| PropertyFailure::InternalFailure)?
                .to_le_bytes(),
        );
        Ok(length)
    }
    fn set(
        &mut self,
        key: u16,
        expected: u32,
        value: &[u8],
        access: &dyn PropertyAccess,
        limit: usize,
        out: &mut [u8],
    ) -> Result<usize, PropertyFailure> {
        if limit < 8 {
            return Err(PropertyFailure::LimitExceeded);
        }
        let entry = self.descriptor(key)?;
        if entry.access & 2 == 0 {
            return Err(PropertyFailure::AccessDenied);
        }
        if entry.access & 4 != 0 {
            return Err(PropertyFailure::InvalidValue);
        }
        if value.len() > usize::from(entry.value_max) {
            return Err(PropertyFailure::InvalidValue);
        }
        self.backend.validate_value(key, value)?;
        if !access.allowed(self.module, PropertyOperation::Set, None)
            || !access.allowed(self.module, PropertyOperation::Set, Some(key))
        {
            return Err(PropertyFailure::AccessDenied);
        }
        let change = self
            .backend
            .commit_atomic(self.module, key, expected, value, access)?;
        if change.previous == 0
            || change.previous.checked_add(1) != Some(change.current)
            || (expected != 0 && change.previous != expected)
        {
            return Err(PropertyFailure::InternalFailure);
        }
        out[4..8].copy_from_slice(&change.current.to_le_bytes());
        Ok(8)
    }
    fn describe(
        &self,
        start: u16,
        max: u8,
        access: &dyn PropertyAccess,
        limit: usize,
        out: &mut [u8],
    ) -> Result<usize, PropertyFailure> {
        if start == u16::MAX || !(1..=16).contains(&max) {
            return Err(PropertyFailure::InvalidValue);
        }
        if limit < 7 {
            return Err(PropertyFailure::LimitExceeded);
        }
        let capacity = usize::from(max).min((limit - 7) / 7);
        let mut count = 0;
        let mut next = u16::MAX;
        for entry in self
            .backend
            .catalogue()
            .iter()
            .filter(|entry| entry.key >= start)
        {
            // The first omitted key is observable too; authorize it before
            // returning it as the cursor rather than leaking its existence.
            if !access.allowed(self.module, PropertyOperation::Describe, Some(entry.key)) {
                return Err(PropertyFailure::AccessDenied);
            }
            if count == capacity {
                next = entry.key;
                break;
            }
            let position = 7 + count * 7;
            out[position..position + 2].copy_from_slice(&entry.key.to_le_bytes());
            out[position + 2] = entry.access;
            out[position + 3..position + 5].copy_from_slice(&entry.schema.to_le_bytes());
            out[position + 5..position + 7].copy_from_slice(&entry.value_max.to_le_bytes());
            count += 1;
        }
        if count == 0 && next != u16::MAX {
            return Err(PropertyFailure::LimitExceeded);
        }
        out[4..6].copy_from_slice(&next.to_le_bytes());
        out[6] = u8::try_from(count).map_err(|_| PropertyFailure::InternalFailure)?;
        Ok(7 + count * 7)
    }
}
fn status(out: &mut [u8], code: Status, reason: Reason) -> usize {
    out[..2].copy_from_slice(&(code as u16).to_le_bytes());
    out[2..4].copy_from_slice(&(reason as u16).to_le_bytes());
    4
}

/// Resource-authorized bridge into Session's ID/replay-protected business router.
pub struct PropertyHandler<'a, 'auth, B: PropertyBackend> {
    service: PropertyService<'a, B>,
    access: &'auth dyn PropertyAccess,
    version: (u8, u8),
}
impl<B: PropertyBackend> core::fmt::Debug for PropertyHandler<'_, '_, B> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PropertyHandler")
            .field("service", &self.service)
            .finish_non_exhaustive()
    }
}
impl<'a, 'auth, B: PropertyBackend> PropertyHandler<'a, 'auth, B> {
    /// Bind a real property backend, trusted scope provider and its compiled
    /// module version. This does not register or advertise any module.
    pub const fn new(
        service: PropertyService<'a, B>,
        access: &'auth dyn PropertyAccess,
        version: (u8, u8),
    ) -> Self {
        Self {
            service,
            access,
            version,
        }
    }
}
impl<B: PropertyBackend> ServiceHandler for PropertyHandler<'_, '_, B> {
    fn required_features(&self, module: &ModuleEntry, opcode: u8) -> Option<u32> {
        (module.module == self.service.module
            && (module.major, module.minor) == self.version
            && matches!(opcode, 0x10..=0x12))
        .then_some(0)
    }
    fn authorized(&self, request: Frame<'_>) -> bool {
        if request.header.module != self.service.module {
            return false;
        }
        let operation = match request.header.opcode {
            0x10 => PropertyOperation::Get,
            0x11 => PropertyOperation::Set,
            _ => PropertyOperation::Describe,
        };
        if !self.access.allowed(self.service.module, operation, None) {
            return false;
        }
        let Ok(body) = PropertyRequest::decode(request.header.opcode, request.payload) else {
            // An authorized malformed request is rejected by the real decoder;
            // it never grants permission to a resource or mutates a backend.
            return true;
        };
        match body {
            PropertyRequest::Get { key } | PropertyRequest::Set { key, .. } => {
                self.access
                    .allowed(self.service.module, operation, Some(key))
            }
            PropertyRequest::Describe { start, max } => self
                .service
                .backend
                .catalogue()
                .iter()
                .filter(|entry| entry.key >= start)
                .take(usize::from(max) + 1)
                .all(|entry| {
                    self.access
                        .allowed(self.service.module, operation, Some(entry.key))
                }),
        }
    }
    fn dispatch(&mut self, request: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        self.service
            .dispatch(request, self.access, out)
            .map_err(|error| match error {
                PropertyDispatchError::Capacity => SessionError::Capacity,
                PropertyDispatchError::InvalidFrame => SessionError::InvalidFrame,
                PropertyDispatchError::AccessRevoked => SessionError::AuthorizationRevoked,
            })
    }
}
