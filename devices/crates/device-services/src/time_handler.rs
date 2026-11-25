//! Module01 command adapter. The clock and its calibration policy belong to product integration.
use crate::{Reason, ServiceHandler, SessionError, SoftwareClock, Status};
use core::cell::RefCell;
use device_protocol::{Frame, Kind, ModuleEntry};
/// Actual monotonic-source failure; never replace it with an invented zero clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockSourceError {
    /// The selected timing source is not currently available.
    Unavailable,
    /// The real timing source failed or lost its calibration/timebase.
    Fault,
}
/// Trusted monotonic source; microseconds and configured drift bound must describe the same source.
pub trait ClockSource {
    /// Read actual monotonic microseconds without changing the UTC origin.
    /// # Errors
    /// Unavailable or failed timing source; no successful placeholder timestamp.
    fn monotonic_us(&self) -> Result<u64, ClockSourceError>;
}
/// Independent live read/set scopes, not a phone-supplied flag or `AccessState`.
pub trait TimeAccess {
    /// Whether the principal may read the time or set it (`set=true`).
    fn allowed(&self, set: bool) -> bool;
}
/// Invalid clock-module capability metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimeConfigError {
    /// Must be module01/v1.0 with actual opcode capacities and no unknown feature bits.
    InvalidMetadata,
}
/// Synchronous clock handler; no RTC register, hardware frequency or ownership identity is fabricated.
pub struct TimeHandler<'a> {
    metadata: ModuleEntry,
    clock: &'a RefCell<SoftwareClock>,
    source: &'a dyn ClockSource,
    access: &'a dyn TimeAccess,
}
impl core::fmt::Debug for TimeHandler<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("TimeHandler")
            .field("module", &self.metadata.module)
            .finish_non_exhaustive()
    }
}
impl<'a> TimeHandler<'a> {
    /// Bind a real source and trusted scopes to the device's shared software mapping.
    /// # Errors
    /// Rejects unsupported metadata instead of registering a guessed module version.
    pub fn new(
        metadata: ModuleEntry,
        clock: &'a RefCell<SoftwareClock>,
        source: &'a dyn ClockSource,
        access: &'a dyn TimeAccess,
    ) -> Result<Self, TimeConfigError> {
        if metadata.module != 1
            || metadata.major != 1
            || metadata.minor != 0
            || metadata.availability != 1
            || metadata.features != 0
            || metadata.input_max < 12
            || metadata.input_max > 512
            || metadata.output_max < 25
            || metadata.output_max > 512
        {
            return Err(TimeConfigError::InvalidMetadata);
        }
        Ok(Self {
            metadata,
            clock,
            source,
            access,
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
}
impl ServiceHandler for TimeHandler<'_> {
    fn required_features(&self, module: &ModuleEntry, opcode: u8) -> Option<u32> {
        (*module == self.metadata && matches!(opcode, 1 | 2)).then_some(0)
    }
    fn authorized(&self, request: Frame<'_>) -> bool {
        request.header.module == 1 && self.access.allowed(request.header.opcode == 2)
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
            _ => return Self::status(out, Status::UnsupportedOpcode, Reason::None),
        };
        if !self.access.allowed(set) {
            return Self::status(out, Status::AccessDenied, Reason::RoleRequired);
        }
        if request.payload.len() != if set { 12 } else { 0 } {
            return Self::status(out, Status::InvalidArgument, Reason::None);
        }
        if out.len() < 25 {
            return Err(SessionError::Capacity);
        }
        let Ok(now) = self.source.monotonic_us() else {
            return Self::status(out, Status::HardwareFailure, Reason::None);
        };
        if !self.access.allowed(set) {
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        let Ok(mut clock) = self.clock.try_borrow_mut() else {
            return Self::status(out, Status::Busy, Reason::None);
        };
        if set {
            let b = request.payload;
            let utc = i64::from_le_bytes([b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]]);
            let uncertainty = u32::from_le_bytes([b[8], b[9], b[10], b[11]]);
            if clock.synchronize(utc, uncertainty, now).is_err() {
                return Self::status(out, Status::HardwareFailure, Reason::None);
            }
        }
        let Ok(mapping) = clock.read(now) else {
            return Self::status(out, Status::HardwareFailure, Reason::None);
        };
        if !self.access.allowed(set) {
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        Self::status(out, Status::Ok, Reason::None)?;
        mapping
            .encode(&mut out[4..25])
            .map_err(SessionError::Codec)?;
        Ok(25)
    }
}
