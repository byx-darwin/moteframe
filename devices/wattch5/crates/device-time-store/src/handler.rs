//! Additive module01 adapter preserving the existing wire contract.
use crate::{Error, TimeStore, TrustedClock};
use core::fmt;
use device_protocol::{Frame, Kind, ModuleEntry};
use device_services::{Reason, ServiceHandler, SessionError, Status, TimeAccess, TimeConfigError};
use device_storage::persistence::{Error as StorageError, SnapshotStore};
/// Durable module01 handler sharing one trusted time owner.
pub struct DurableTimeHandler<'a, S: SnapshotStore, C: TrustedClock> {
    metadata: ModuleEntry,
    owner: &'a TimeStore<S, C>,
    access: &'a dyn TimeAccess,
}
impl<S: SnapshotStore, C: TrustedClock> fmt::Debug for DurableTimeHandler<'_, S, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DurableTimeHandler")
            .field("module", &self.metadata.module)
            .finish_non_exhaustive()
    }
}
impl<'a, S: SnapshotStore, C: TrustedClock> DurableTimeHandler<'a, S, C> {
    /// Bind actual metadata and live read/set scopes to the durable owner.
    /// # Errors
    /// Reject unsupported metadata or insufficient actual opcode capacities.
    pub fn new(
        metadata: ModuleEntry,
        owner: &'a TimeStore<S, C>,
        access: &'a dyn TimeAccess,
    ) -> Result<Self, TimeConfigError> {
        if metadata.module != 1
            || metadata.major != 1
            || metadata.minor != 0
            || metadata.availability != 1
            || metadata.features != 0
            || !(12..=512).contains(&metadata.input_max)
            || !(25..=512).contains(&metadata.output_max)
        {
            return Err(TimeConfigError::InvalidMetadata);
        }
        Ok(Self {
            metadata,
            owner,
            access,
        })
    }
}
fn status(out: &mut [u8], status: Status, reason: Reason) -> Result<usize, SessionError> {
    if out.len() < 4 {
        return Err(SessionError::Capacity);
    }
    out[..2].copy_from_slice(&(status as u16).to_le_bytes());
    out[2..4].copy_from_slice(&(reason as u16).to_le_bytes());
    Ok(4)
}
fn failure(error: Error, out: &mut [u8]) -> Result<usize, SessionError> {
    let code = match error {
        Error::AuthorizationRevoked => {
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        Error::AccessDenied => return status(out, Status::AccessDenied, Reason::RoleRequired),
        Error::Busy => Status::Busy,
        Error::Integrity => Status::IntegrityFailure,
        Error::Storage(StorageError::Conflict) => Status::Conflict,
        Error::Storage(_) => Status::StorageFailure,
        Error::Clock(_) | Error::InvalidReading | Error::NonMonotonic | Error::Overflow => {
            Status::HardwareFailure
        }
    };
    status(out, code, Reason::None)
}
impl<S: SnapshotStore, C: TrustedClock> ServiceHandler for DurableTimeHandler<'_, S, C> {
    fn required_features(&self, module: &ModuleEntry, opcode: u8) -> Option<u32> {
        (*module == self.metadata && matches!(opcode, 1 | 2)).then_some(0)
    }
    fn authorized(&self, request: Frame<'_>) -> bool {
        request.header.module == 1
            && self
                .owner
                .check_access(self.access, request.header.opcode == 2)
                .is_ok()
    }
    fn dispatch(&mut self, request: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        if request.header.kind != Kind::Request
            || request.header.module != 1
            || request.header.message_id == 0
            || request.header.offset != 0
            || usize::from(request.header.total_length) != request.payload.len()
        {
            return Err(SessionError::InvalidHandlerResponse);
        }
        if out.len() < 4 {
            return Err(SessionError::Capacity);
        }
        let set = match request.header.opcode {
            1 => false,
            2 => true,
            _ => return status(out, Status::UnsupportedOpcode, Reason::None),
        };
        if let Err(error) = self.owner.check_access(self.access, set) {
            return failure(error, out);
        }
        if request.payload.len() != if set { 12 } else { 0 } {
            return status(out, Status::InvalidArgument, Reason::None);
        }
        if out.len() < 25 {
            return Err(SessionError::Capacity);
        }
        let result = if set {
            let bytes = request.payload;
            let utc = i64::from_le_bytes([
                bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
            ]);
            let uncertainty = u32::from_le_bytes([bytes[8], bytes[9], bytes[10], bytes[11]]);
            self.owner.synchronize(utc, uncertainty, self.access)
        } else {
            self.owner.read(self.access)
        };
        let mapping = match result {
            Ok(mapping) => mapping,
            // Dispatch entry already observed permission. Any later denial is
            // revocation, including the owner's initial live-scope recheck.
            Err(Error::AccessDenied) => return failure(Error::AuthorizationRevoked, out),
            Err(error) => return failure(error, out),
        };
        status(out, Status::Ok, Reason::None)?;
        mapping
            .encode(&mut out[4..25])
            .map_err(SessionError::Codec)?;
        Ok(25)
    }
}
