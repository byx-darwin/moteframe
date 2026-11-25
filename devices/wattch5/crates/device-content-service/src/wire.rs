use crate::Error;
use device_services::{Reason, SessionError, Status};
/// Fully decoded command. Unknown opcodes never turn into successful empty responses.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Command {
    /// 06.C30.
    #[cfg(feature = "find")]
    Find(device_content::FindRequest),
    /// 06.C31.
    #[cfg(feature = "find")]
    FindStop(u32),
    /// 06.C32, frozen result dictionary.
    #[cfg(feature = "find")]
    FindResult {
        /// Find identity.
        id: u32,
        /// Frozen 1 found,2 timeout,3 cancelled.
        result: u8,
    },
    /// 07.C30 exact token echo.
    #[cfg(feature = "phone")]
    Heartbeat(u32),
    /// Candidate 07.C31.
    #[cfg(feature = "phone")]
    SosAck {
        /// Event identity.
        id: u32,
        /// Actual result 1..4.
        result: u8,
    },
    /// 08.C30.
    #[cfg(feature = "companion-radio")]
    Radio(bool),
    /// 30.C30.
    #[cfg(feature = "notifications")]
    NotificationAck {
        /// Notification identity.
        notification: u32,
        /// Call action1..3.
        action: u8,
        /// Actual result1..5.
        result: u8,
    },
    /// 33.C30.
    #[cfg(feature = "media")]
    CameraMode(bool),
    /// 33.C31.
    #[cfg(feature = "media")]
    CameraAck {
        /// Event identity.
        id: u32,
        /// Frozen result0..3.
        result: u8,
    },
    /// 33.C32.
    #[cfg(feature = "media")]
    Music(u8),
    /// 33.C33.
    #[cfg(feature = "media")]
    MusicAck {
        /// Event identity.
        id: u32,
        /// Frozen result0..3.
        result: u8,
    },
    /// Candidate schema2 33.C34.
    #[cfg(feature = "gimbal")]
    Gimbal(device_content::GimbalRequest),
}
impl core::fmt::Debug for Command {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Command")
            .field("length", &core::mem::size_of_val(self))
            .finish_non_exhaustive()
    }
}
impl Command {
    /// Decode exact command payloads and all enum/result constraints before state changes.
    /// # Errors
    /// Returns Unknown for unsupported/disabled routes, Invalid for malformed fields and `EnumUnsupported` for unknown enumeration values.
    pub fn decode(module: u8, opcode: u8, bytes: &[u8]) -> Result<Self, Error> {
        let value = match (module, opcode) {
            #[cfg(feature = "find")]
            (6, 0x30) => {
                if bytes.len() != 4 {
                    return Err(Error::Invalid);
                }
                if bytes[0] > 1 || !(1..=3).contains(&bytes[1]) {
                    return Err(Error::EnumUnsupported);
                }
                Self::Find(device_content::FindRequest::decode(bytes)?)
            }
            #[cfg(feature = "find")]
            (6, 0x31) => Self::FindStop(nonzero_id(bytes)?),
            #[cfg(feature = "find")]
            (6, 0x32) => {
                let (id, result) = id_result(bytes, 1, 3)?;
                Self::FindResult { id, result }
            }
            #[cfg(feature = "phone")]
            (7, 0x30) => Self::Heartbeat(u32::from_le_bytes(
                bytes.try_into().map_err(|_| Error::Invalid)?,
            )),
            #[cfg(feature = "phone")]
            (7, 0x31) => {
                let (id, result) = id_result(bytes, 1, 4)?;
                Self::SosAck { id, result }
            }
            #[cfg(feature = "companion-radio")]
            (8, 0x30) => Self::Radio(boolean(bytes)?),
            #[cfg(feature = "notifications")]
            (0x30, 0x30) => {
                if bytes.len() != 6 {
                    return Err(Error::Invalid);
                }
                let notification = nonzero_id(&bytes[..4])?;
                let action = bytes[4];
                let result = bytes[5];
                if !(1..=3).contains(&action) || !(1..=5).contains(&result) {
                    return Err(Error::EnumUnsupported);
                }
                Self::NotificationAck {
                    notification,
                    action,
                    result,
                }
            }
            #[cfg(feature = "media")]
            (0x33, 0x30) => Self::CameraMode(boolean(bytes)?),
            #[cfg(feature = "media")]
            (0x33, 0x31) => {
                let (id, result) = id_result(bytes, 0, 3)?;
                Self::CameraAck { id, result }
            }
            #[cfg(feature = "media")]
            (0x33, 0x32) => {
                let [action] = bytes else {
                    return Err(Error::Invalid);
                };
                if !(1..=5).contains(action) {
                    return Err(Error::EnumUnsupported);
                }
                Self::Music(*action)
            }
            #[cfg(feature = "media")]
            (0x33, 0x33) => {
                let (id, result) = id_result(bytes, 0, 3)?;
                Self::MusicAck { id, result }
            }
            #[cfg(feature = "gimbal")]
            (0x33, 0x34) => {
                if bytes.len() != 11 {
                    return Err(Error::Invalid);
                }
                if bytes[4] > 1 || bytes[5] > 1 || bytes[6] > 1 {
                    return Err(Error::EnumUnsupported);
                }
                Self::Gimbal(device_content::GimbalRequest::decode(bytes)?)
            }
            _ => return Err(device_content::Error::Unknown.into()),
        };
        Ok(value)
    }
    pub(crate) const fn operation(self) -> device_content::Operation {
        match self {
            #[cfg(feature = "find")]
            Self::Find(_) | Self::FindStop(_) => device_content::Operation::Find,
            #[cfg(feature = "find")]
            Self::FindResult { .. } => device_content::Operation::Settle,
            #[cfg(feature = "phone")]
            Self::Heartbeat(_) | Self::SosAck { .. } => device_content::Operation::Settle,
            #[cfg(feature = "companion-radio")]
            Self::Radio(_) => device_content::Operation::Submit,
            #[cfg(feature = "notifications")]
            Self::NotificationAck { .. } => device_content::Operation::Settle,
            #[cfg(feature = "media")]
            Self::CameraMode(_) | Self::Music(_) => device_content::Operation::Submit,
            #[cfg(feature = "media")]
            Self::CameraAck { .. } | Self::MusicAck { .. } => device_content::Operation::Settle,
            #[cfg(feature = "gimbal")]
            Self::Gimbal(_) => device_content::Operation::Submit,
        }
    }
    /// Optional resource identity for trusted authorization.
    #[must_use]
    pub const fn id(self) -> Option<u32> {
        match self {
            #[cfg(feature = "find")]
            Self::FindStop(id) | Self::FindResult { id, .. } => Some(id),
            #[cfg(feature = "phone")]
            Self::SosAck { id, .. } => Some(id),
            #[cfg(feature = "notifications")]
            Self::NotificationAck { notification, .. } => Some(notification),
            #[cfg(feature = "media")]
            Self::CameraAck { id, .. } | Self::MusicAck { id, .. } => Some(id),
            #[cfg(feature = "gimbal")]
            Self::Gimbal(value) => Some(value.action_id),
            #[cfg(feature = "phone")]
            Self::Heartbeat(_) => None,
            #[cfg(feature = "find")]
            Self::Find(_) => None,
            #[cfg(feature = "companion-radio")]
            Self::Radio(_) => None,
            #[cfg(feature = "media")]
            Self::CameraMode(_) | Self::Music(_) => None,
        }
    }
}
/// Opaque, connection/generation/FIFO-bound final admission receipt. Never decode from peer bytes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct EventToken {
    pub(crate) connection: u64,
    pub(crate) generation: u64,
    pub(crate) sequence: u64,
    pub(crate) module: u8,
    pub(crate) opcode: u8,
    pub(crate) id: u32,
    pub(crate) resource_generation: u64,
}
impl EventToken {
    /// Event module.
    #[must_use]
    pub const fn module(self) -> u8 {
        self.module
    }
    /// Event opcode.
    #[must_use]
    pub const fn opcode(self) -> u8 {
        self.opcode
    }
}
impl core::fmt::Debug for EventToken {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("EventToken")
            .field("length", &core::mem::size_of_val(self))
            .finish_non_exhaustive()
    }
}
pub(crate) fn status(out: &mut [u8], status: Status, reason: Reason) -> usize {
    out[..2].copy_from_slice(&(status as u16).to_le_bytes());
    out[2..4].copy_from_slice(&(reason as u16).to_le_bytes());
    4
}
pub(crate) fn wire_error(error: Error) -> (Status, Reason) {
    use device_content::Error as Content;
    match error {
        Error::Content(Content::Unknown | Content::Unavailable) => {
            (Status::UnsupportedOpcode, Reason::None)
        }
        Error::Content(Content::Invalid) | Error::Invalid => {
            (Status::InvalidArgument, Reason::None)
        }
        Error::Content(Content::Capacity) | Error::Capacity => {
            (Status::LimitExceeded, Reason::None)
        }
        Error::Content(Content::NotFound) => (Status::NotFound, Reason::None),
        Error::Content(Content::Conflict) => (Status::Conflict, Reason::None),
        Error::TerminalConflict => (Status::Conflict, Reason::TerminalConflict),
        Error::EnumUnsupported => (Status::InvalidArgument, Reason::EnumUnsupported),
        Error::Content(Content::Denied) | Error::Denied => {
            (Status::AccessDenied, Reason::RoleRequired)
        }
        Error::Content(Content::State) | Error::Closed => {
            (Status::InvalidState, Reason::JobBindingMismatch)
        }
        Error::Content(Content::Backend) | Error::Backend => {
            (Status::HardwareFailure, Reason::None)
        }
        Error::Busy => (Status::Busy, Reason::None),
        Error::Clock => (Status::InternalFailure, Reason::None),
    }
}
pub(crate) fn local_error(error: Error) -> SessionError {
    match error {
        Error::Capacity => SessionError::Capacity,
        Error::Denied | Error::Closed | Error::Clock => SessionError::AuthorizationRevoked,
        _ => SessionError::InvalidHandlerResponse,
    }
}

#[cfg(any(
    feature = "find",
    feature = "phone",
    feature = "media",
    feature = "notifications"
))]
fn nonzero_id(bytes: &[u8]) -> Result<u32, Error> {
    let id = u32::from_le_bytes(bytes.try_into().map_err(|_| Error::Invalid)?);
    if id == 0 {
        return Err(Error::Invalid);
    }
    Ok(id)
}
#[cfg(any(feature = "find", feature = "phone", feature = "media"))]
fn id_result(bytes: &[u8], min: u8, max: u8) -> Result<(u32, u8), Error> {
    if bytes.len() != 5 {
        return Err(Error::Invalid);
    }
    let id = nonzero_id(&bytes[..4])?;
    let result = bytes[4];
    if result < min || result > max {
        return Err(Error::EnumUnsupported);
    }
    Ok((id, result))
}
#[cfg(any(feature = "media", feature = "companion-radio"))]
fn boolean(bytes: &[u8]) -> Result<bool, Error> {
    match bytes {
        [0] => Ok(false),
        [1] => Ok(true),
        [_] => Err(Error::EnumUnsupported),
        _ => Err(Error::Invalid),
    }
}
