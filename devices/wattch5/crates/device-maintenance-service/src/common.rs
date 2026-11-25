#[cfg(any(
    feature = "raw-data",
    feature = "self-test",
    feature = "engineering",
    feature = "factory-config",
    feature = "device-jobs"
))]
use core::cell::Cell;
#[cfg(any(
    feature = "raw-data",
    feature = "self-test",
    feature = "engineering",
    feature = "factory-config",
    feature = "device-jobs"
))]
use device_protocol::{Frame, Kind, ModuleEntry};
#[cfg(any(
    feature = "raw-data",
    feature = "self-test",
    feature = "engineering",
    feature = "factory-config",
    feature = "device-jobs"
))]
use device_services::{Reason, SessionError, Status};
/// Explicit wire integration failures; no implicit successful fallback exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Capacity,
    Denied,
    DangerousDenied,
    Closed,
    Busy,
    Clock,
    Timeout,
    Unsupported,
    Unavailable,
    NotFound,
    Conflict,
    Storage,
    UnknownOutcome,
    Backend,
    #[cfg(any(
        feature = "raw-data",
        feature = "self-test",
        feature = "engineering",
        feature = "factory-config"
    ))]
    Engine(device_maintenance::Error),
}
#[cfg(any(
    feature = "raw-data",
    feature = "self-test",
    feature = "engineering",
    feature = "factory-config"
))]
impl From<device_maintenance::Error> for Error {
    fn from(e: device_maintenance::Error) -> Self {
        Self::Engine(e)
    }
}
/// Actual trusted monotonic milliseconds. Missing clock cannot be replaced with peer time.
pub trait Clock {
    /// # Errors
    /// Reports real clock unavailability/failure.
    fn now_ms(&self) -> Result<u64, Error>;
}
/// Exact C40 immutable operation identity and complete versioned parameter bytes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct NamedRequest<'a> {
    pub operation_id: [u8; 16],
    pub operation: u16,
    pub params: &'a [u8],
}
impl core::fmt::Debug for NamedRequest<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NamedRequest")
            .field("params_length", &self.params.len())
            .finish_non_exhaustive()
    }
}
impl<'a> NamedRequest<'a> {
    /// # Errors
    /// Rejects zero identity/operation, incorrect complete length and bodies over512B.
    pub fn decode(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() < 20 || bytes.len() > 512 {
            return Err(Error::Invalid);
        }
        let mut id = [0; 16];
        id.copy_from_slice(&bytes[..16]);
        let operation = u16_at(bytes, 16);
        let n = usize::from(u16_at(bytes, 18));
        if id == [0; 16] || operation == 0 || n != bytes.len() - 20 {
            return Err(Error::Invalid);
        }
        Ok(Self {
            operation_id: id,
            operation,
            params: &bytes[20..],
        })
    }
}
#[cfg(any(
    feature = "raw-data",
    feature = "self-test",
    feature = "engineering",
    feature = "factory-config",
    feature = "device-jobs"
))]
pub(crate) struct Guard<'a> {
    clock: &'a dyn Clock,
    last: Cell<Option<u64>>,
    closed: Cell<bool>,
}
#[cfg(any(
    feature = "raw-data",
    feature = "self-test",
    feature = "engineering",
    feature = "factory-config",
    feature = "device-jobs"
))]
impl<'a> Guard<'a> {
    pub const fn new(clock: &'a dyn Clock) -> Self {
        Self {
            clock,
            last: Cell::new(None),
            closed: Cell::new(false),
        }
    }
    pub fn now(&self) -> Result<u64, Error> {
        if self.closed.get() {
            return Err(Error::Closed);
        }
        let now = self.clock.now_ms().inspect_err(|_| self.closed.set(true))?;
        if self.last.get().is_some_and(|last| now < last) {
            self.closed.set(true);
            return Err(Error::Clock);
        }
        self.last.set(Some(now));
        Ok(now)
    }
    pub fn close(&self) {
        self.closed.set(true);
    }
    pub fn closed(&self) -> bool {
        self.closed.get()
    }
}
#[cfg(any(
    feature = "raw-data",
    feature = "self-test",
    feature = "engineering",
    feature = "factory-config",
    feature = "device-jobs"
))]
pub(crate) fn entry(e: ModuleEntry, module: u8, major: u8, minor: u8) -> Result<(), Error> {
    if e.module != module
        || e.major != major
        || e.minor != minor
        || e.availability != 1
        || e.features != 0
        || !(128..=512).contains(&e.input_max)
        || !(128..=512).contains(&e.output_max)
    {
        return Err(Error::Invalid);
    }
    Ok(())
}
#[cfg(any(
    feature = "raw-data",
    feature = "self-test",
    feature = "engineering",
    feature = "factory-config",
    feature = "device-jobs"
))]
pub(crate) fn frame(f: Frame<'_>, entry: ModuleEntry) -> Result<(), SessionError> {
    if f.header.kind != Kind::Request
        || f.header.module != entry.module
        || f.header.message_id == 0
        || f.header.offset != 0
        || usize::from(f.header.total_length) != f.payload.len()
        || f.payload.len() > usize::from(entry.input_max)
    {
        return Err(SessionError::InvalidFrame);
    }
    Ok(())
}
#[cfg(any(
    feature = "device-jobs",
    feature = "engineering",
    feature = "factory-config"
))]
pub(crate) fn id16(bytes: &[u8]) -> Result<[u8; 16], Error> {
    if bytes.len() != 16 {
        return Err(Error::Invalid);
    }
    let mut id = [0; 16];
    id.copy_from_slice(bytes);
    if id == [0; 16] {
        return Err(Error::Invalid);
    }
    Ok(id)
}
#[cfg(feature = "self-test")]
pub(crate) fn id32(bytes: &[u8]) -> Result<u32, Error> {
    if bytes.len() != 4 {
        return Err(Error::Invalid);
    }
    let id = u32_at(bytes, 0);
    if id == 0 {
        return Err(Error::Invalid);
    }
    Ok(id)
}
pub(crate) fn u16_at(b: &[u8], p: usize) -> u16 {
    u16::from_le_bytes([b[p], b[p + 1]])
}
#[cfg(any(
    feature = "raw-data",
    feature = "self-test",
    feature = "engineering",
    feature = "factory-config"
))]
pub(crate) fn u32_at(b: &[u8], p: usize) -> u32 {
    u32::from_le_bytes([b[p], b[p + 1], b[p + 2], b[p + 3]])
}
#[cfg(any(
    feature = "raw-data",
    feature = "self-test",
    feature = "engineering",
    feature = "factory-config",
    feature = "device-jobs"
))]
pub(crate) fn put16(b: &mut [u8], p: usize, v: u16) {
    b[p..p + 2].copy_from_slice(&v.to_le_bytes());
}
#[cfg(any(
    feature = "raw-data",
    feature = "self-test",
    feature = "engineering",
    feature = "factory-config",
    feature = "device-jobs"
))]
pub(crate) fn put32(b: &mut [u8], p: usize, v: u32) {
    b[p..p + 4].copy_from_slice(&v.to_le_bytes());
}
#[cfg(feature = "raw-data")]
pub(crate) fn put64(b: &mut [u8], p: usize, v: u64) {
    b[p..p + 8].copy_from_slice(&v.to_le_bytes());
}
#[cfg(any(
    feature = "raw-data",
    feature = "self-test",
    feature = "engineering",
    feature = "factory-config",
    feature = "device-jobs"
))]
pub(crate) fn status(b: &mut [u8], s: Status, r: Reason) -> usize {
    put16(b, 0, s as u16);
    put16(b, 2, r as u16);
    4
}
#[cfg(any(
    feature = "raw-data",
    feature = "self-test",
    feature = "engineering",
    feature = "factory-config",
    feature = "device-jobs"
))]
pub(crate) fn failure(b: &mut [u8], e: Error) -> usize {
    let (s, r) = match e {
        Error::DangerousDenied => (Status::AccessDenied, Reason::DangerousScopeRequired),
        Error::Denied => (Status::AccessDenied, Reason::RoleRequired),
        Error::Capacity => (Status::LimitExceeded, Reason::None),
        Error::Busy => (Status::Busy, Reason::None),
        Error::Unsupported => (Status::UnsupportedOpcode, Reason::None),
        Error::Timeout => (Status::Timeout, Reason::None),
        Error::Unavailable | Error::Closed => (Status::InvalidState, Reason::None),
        Error::NotFound => (Status::NotFound, Reason::None),
        Error::Conflict => (Status::Conflict, Reason::None),
        Error::Storage | Error::UnknownOutcome => (Status::StorageFailure, Reason::None),
        Error::Backend => (Status::HardwareFailure, Reason::None),
        #[cfg(any(
            feature = "raw-data",
            feature = "self-test",
            feature = "engineering",
            feature = "factory-config"
        ))]
        Error::Engine(e) => return failure(b, engine_error(e)),
        _ => (Status::InvalidArgument, Reason::None),
    };
    status(b, s, r)
}
#[cfg(any(
    feature = "raw-data",
    feature = "self-test",
    feature = "engineering",
    feature = "factory-config",
    feature = "device-jobs"
))]
pub(crate) fn local(e: Error) -> SessionError {
    match e {
        Error::Capacity => SessionError::Capacity,
        Error::Denied | Error::Closed | Error::DangerousDenied => {
            SessionError::AuthorizationRevoked
        }
        Error::Clock => SessionError::Codec(device_protocol::ProtocolError::InvalidState),
        Error::Timeout => SessionError::Codec(device_protocol::ProtocolError::Timeout),
        #[cfg(any(
            feature = "raw-data",
            feature = "self-test",
            feature = "engineering",
            feature = "factory-config"
        ))]
        Error::Engine(error) => local(engine_error(error)),
        _ => SessionError::InvalidHandlerResponse,
    }
}
#[cfg(any(
    feature = "raw-data",
    feature = "self-test",
    feature = "engineering",
    feature = "factory-config"
))]
pub(crate) fn engine_error(e: device_maintenance::Error) -> Error {
    use device_maintenance::Error as E;
    match e {
        E::Denied | E::Commercial => Error::Denied,
        E::Closed => Error::Closed,
        E::Busy => Error::Busy,
        E::NotFound => Error::NotFound,
        E::Conflict => Error::Conflict,
        E::Capacity => Error::Capacity,
        E::Unsupported => Error::Unsupported,
        E::Unavailable | E::CriterionUndefined => Error::Unavailable,
        E::Clock => Error::Clock,
        E::Timeout => Error::Timeout,
        E::Backend => Error::Backend,
        _ => Error::Invalid,
    }
}

impl Error {
    /// Translate a real stream bridge failure to the existing core resource dictionary.
    #[must_use]
    pub fn resource_failure(self) -> device_services::ResourceFailure {
        use device_services::ResourceFailure as R;
        match self {
            Self::Invalid => R::InvalidArgument,
            Self::Capacity => R::LimitExceeded,
            Self::Denied | Self::DangerousDenied => R::AccessDenied,
            Self::Closed | Self::Clock | Self::Timeout | Self::Unavailable => R::InvalidState,
            Self::Busy => R::Busy,
            Self::Unsupported => R::Unsupported,
            Self::NotFound => R::NotFound,
            Self::Conflict => R::Conflict,
            Self::Storage | Self::UnknownOutcome => R::StorageFailure,
            Self::Backend => R::HardwareFailure,
            #[cfg(any(
                feature = "raw-data",
                feature = "self-test",
                feature = "engineering",
                feature = "factory-config"
            ))]
            Self::Engine(e) => engine_error(e).resource_failure(),
        }
    }
}
