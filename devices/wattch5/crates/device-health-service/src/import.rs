//! Optional App import command; source is assigned by the trusted endpoint, never the peer.
use crate::{Error, HealthAccess, Operation};
use core::cell::Cell;
use device_health::{ResultOrigin, SourcedResult, Timestamp};
use device_protocol::{Frame, Kind, ModuleEntry};
use device_services::{Reason, ServiceHandler, SessionError, Status};

/// Connection-bound request identity, distinct from persistent record IDs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImportKey {
    /// Actual fresh connection identity supplied by product integration.
    pub connection: u64,
    /// Health module owning this request.
    pub module: u8,
    /// Actual nonzero owning protocol request ID.
    pub message: u16,
}
/// Borrowed App import: schema1 value bytes and explicit UTC, without a caller-supplied source.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HealthImport<'a> {
    /// Selected module25,26or27.
    pub module: u8,
    /// Supplied UTC with explicit validity, never a synchronization operation.
    pub timestamp: Timestamp,
    /// Validated unchanged schema1 value body.
    pub value: &'a [u8],
}
impl core::fmt::Debug for HealthImport<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HealthImport")
            .field("value_length", &self.value.len())
            .finish()
    }
}
impl<'a> HealthImport<'a> {
    /// Decode C33: `value_schema:u16=1,utc_ms:i64,time_valid:u8,value_body`.
    /// # Errors
    /// Unknown schemas, noncanonical time, malformed/missing result values fail.
    pub fn decode(module: u8, bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() < 11 || bytes[..2] != [1, 0] || bytes[10] > 1 {
            return Err(Error::Invalid);
        }
        let utc = i64::from_le_bytes(bytes[2..10].try_into().map_err(|_| Error::Invalid)?);
        let value = &bytes[11..];
        let request = Self {
            module,
            timestamp: Timestamp::new(utc, bytes[10] == 1)?,
            value,
        };
        request.validate()?;
        Ok(request)
    }
    /// Check typed callers using the same complete-value policy as wire decoding.
    /// # Errors
    /// Unsupported modules, malformed values and empty imports are invalid.
    pub fn validate(self) -> Result<(), Error> {
        SourcedResult::new(self.module, self.value, ResultOrigin::AppImport)?;
        let present = match self.module {
            0x25 => self.value[6] & 3 != 0,
            0x26 => self.value[4] & 1 != 0,
            0x27 => self.value[2] == 1,
            _ => false,
        };
        if !present {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
/// One boot-owned producer/store. Methods admit or inspect work but perform no medium I/O.
/// The separately scheduled worker retains accepted commits across request/connection teardown.
pub trait HealthImportPort {
    /// Actual ready import storage/source; no default readiness is inferred.
    fn available(&self, module: u8) -> bool;
    /// Inspect/admit the exact immutable request; true means its receipt is durable and verified.
    /// # Errors
    /// Busy, invalid identity, denial and actual storage failure remain explicit.
    fn prepare(&self, key: ImportKey, request: HealthImport<'_>) -> Result<bool, Error>;
    /// Return the verified nonzero history ID for that exact request; never allocate here.
    /// # Errors
    /// No matching durable result or a changed request fails.
    fn receipt(&self, key: ImportKey, request: HealthImport<'_>) -> Result<u64, Error>;
    /// Check that a cached success still refers to the same committed import.
    fn current(&self, key: ImportKey, record: u64) -> bool;
    /// Detach only delivery; accepted durable work must continue.
    fn detach(&self, key: ImportKey);
}
/// Adds25/26/27.C33 to one existing health handler without a second replay cache.
/// The owning Session still controls framing, replay and final queue admission.
pub struct ImportHealthHandler<'a, H> {
    inner: H,
    connection: u64,
    port: &'a dyn HealthImportPort,
    access: &'a dyn HealthAccess,
    pending: Cell<Option<ImportKey>>,
    failure: Cell<Option<(ImportKey, Error)>>,
}
impl<H> core::fmt::Debug for ImportHealthHandler<'_, H> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ImportHealthHandler")
            .finish_non_exhaustive()
    }
}
impl<'a, H: ServiceHandler> ImportHealthHandler<'a, H> {
    /// Bind the same actual connection/access provider as the enclosed health actor.
    /// # Errors
    /// Zero connection identity is invalid.
    pub fn new(
        inner: H,
        connection: u64,
        port: &'a dyn HealthImportPort,
        access: &'a dyn HealthAccess,
    ) -> Result<Self, Error> {
        if connection == 0 {
            return Err(Error::Invalid);
        }
        Ok(Self {
            inner,
            connection,
            port,
            access,
            pending: Cell::new(None),
            failure: Cell::new(None),
        })
    }
    /// Borrow the original health actor for its existing result/event pump.
    pub fn inner_mut(&mut self) -> &mut H {
        &mut self.inner
    }
    fn key(&self, f: Frame<'_>) -> ImportKey {
        ImportKey {
            connection: self.connection,
            module: f.header.module,
            message: f.header.message_id,
        }
    }
    fn is_import(f: Frame<'_>) -> bool {
        f.header.opcode == 0x33 && matches!(f.header.module, 0x25..=0x27)
    }
    fn permitted(&self, module: u8) -> bool {
        !self.inner.must_end_session() && self.access.allowed(module, Operation::Import, None)
    }
    fn cancel(&self) {
        if let Some(key) = self.pending.take() {
            self.port.detach(key);
        }
    }
}
impl<H: ServiceHandler> ServiceHandler for ImportHealthHandler<'_, H> {
    fn advance(&mut self, now: u64) -> Result<(), SessionError> {
        self.inner.advance(now)
    }
    fn informational_access(&self) -> Option<device_services::AccessSnapshot> {
        self.inner.informational_access()
    }
    fn required_features(&self, e: &ModuleEntry, op: u8) -> Option<u32> {
        if op == 0x33 && matches!(e.module, 0x25..=0x27) {
            // Require an actual registered health route as well as a real import port.
            self.inner.required_features(e, 2)
        } else {
            self.inner.required_features(e, op)
        }
    }
    fn authorized(&self, f: Frame<'_>) -> bool {
        if Self::is_import(f) {
            self.permitted(f.header.module)
        } else {
            self.inner.authorized(f)
        }
    }
    fn must_end_session(&self) -> bool {
        self.inner.must_end_session()
    }
    fn response_ready(&mut self, f: Frame<'_>) -> Result<bool, SessionError> {
        if !Self::is_import(f) {
            return self.inner.response_ready(f);
        }
        if !self.permitted(f.header.module) {
            self.cancel();
            return Err(SessionError::AuthorizationRevoked);
        }
        let Ok(request) = HealthImport::decode(f.header.module, f.payload) else {
            return Ok(true);
        };
        let key = self.key(f);
        if key.message == 0 {
            return Ok(true);
        }
        self.pending.set(Some(key));
        self.failure.set(None);
        match self.port.prepare(key, request) {
            Ok(ready) => Ok(ready),
            Err(Error::Busy) => Ok(false),
            Err(error) => {
                self.failure.set(Some((key, error)));
                Ok(true)
            }
        }
    }
    fn dispatch(&mut self, f: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        if !Self::is_import(f) {
            return self.inner.dispatch(f, out);
        }
        if out.len() < 12 {
            return Err(SessionError::Capacity);
        }
        if f.header.kind != Kind::Request || !self.permitted(f.header.module) {
            return Err(SessionError::AuthorizationRevoked);
        }
        let result = HealthImport::decode(f.header.module, f.payload).and_then(|request| {
            let key = self.key(f);
            if self.pending.get() != Some(key) {
                return Err(Error::Busy);
            }
            if let Some((failed, error)) = self.failure.get()
                && failed == key
            {
                return Err(error);
            }
            self.port.receipt(key, request)
        });
        match result {
            Ok(id) if id != 0 => {
                out[..4].fill(0);
                out[4..12].copy_from_slice(&id.to_le_bytes());
                Ok(12)
            }
            result => {
                let status = match result {
                    Err(Error::Busy) => Status::Busy,
                    Err(Error::Denied | Error::Closed) => Status::AccessDenied,
                    Err(Error::Capacity) => Status::LimitExceeded,
                    Err(Error::Backend) | Ok(_) => Status::HardwareFailure,
                    _ => Status::InvalidArgument,
                };
                out[..2].copy_from_slice(&(status as u16).to_le_bytes());
                out[2..4].copy_from_slice(&(Reason::None as u16).to_le_bytes());
                Ok(4)
            }
        }
    }
    fn replay_valid(&self, f: Frame<'_>, b: &[u8]) -> bool {
        if !Self::is_import(f) {
            return self.inner.replay_valid(f, b);
        }
        if b.len() < 4 || !self.permitted(f.header.module) {
            return false;
        }
        if b[..2] != [0, 0] {
            return true;
        }
        b.len() == 12
            && self.port.current(
                self.key(f),
                u64::from_le_bytes(b[4..12].try_into().unwrap_or([0; 8])),
            )
    }
    fn response_admitted(&mut self, f: Frame<'_>, response: &[u8]) -> Result<(), SessionError> {
        if Self::is_import(f) {
            if !self.permitted(f.header.module) {
                self.cancel();
                return Err(SessionError::AuthorizationRevoked);
            }
            Ok(())
        } else {
            self.inner.response_admitted(f, response)
        }
    }
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        self.cancel();
        self.inner.connection_closed()
    }
}
