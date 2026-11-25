//! Shared property and object command bodies. Decoding does not authorize storage access.
use crate::{
    ProtocolError,
    bytes::{Reader, Writer},
};
/// Property command body; payload Debug is redacted.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum PropertyRequest<'a> {
    /// GET value and revision.
    Get {
        /// Module-local key.
        key: u16,
    },
    /// SET an atomic revision-checked value.
    Set {
        /// Module-local key.
        key: u16,
        /// Expected current revision.
        expected_revision: u32,
        /// Complete schema-encoded value.
        value: &'a [u8],
    },
    /// DESCRIBE an inclusive page.
    Describe {
        /// Inclusive starting key.
        start: u16,
        /// Maximum returned entries, nonzero.
        max: u8,
    },
}
impl core::fmt::Debug for PropertyRequest<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PropertyRequest")
            .field("opcode", &self.opcode())
            .finish_non_exhaustive()
    }
}
impl<'a> PropertyRequest<'a> {
    /// Wire opcode for this command.
    #[must_use]
    pub const fn opcode(&self) -> u8 {
        match self {
            Self::Get { .. } => 0x10,
            Self::Set { .. } => 0x11,
            Self::Describe { .. } => 0x12,
        }
    }
    /// Decode an exact command body. Module schemas and permissions are checked separately.
    /// # Errors
    /// Rejects unknown opcodes, lengths or zero page sizes.
    pub fn decode(opcode: u8, bytes: &'a [u8]) -> Result<Self, ProtocolError> {
        if bytes.len() > crate::LOGICAL_MAX {
            return Err(ProtocolError::InvalidLength);
        }
        let mut r = Reader::new(bytes);
        let value = match opcode {
            0x10 => Self::Get { key: r.u16()? },
            0x11 => {
                let key = r.u16()?;
                let expected_revision = r.u32()?;
                let n = usize::from(r.u16()?);
                Self::Set {
                    key,
                    expected_revision,
                    value: r.bytes(n)?,
                }
            }
            0x12 => {
                let start = r.u16()?;
                let max = r.u8()?;
                if max == 0 {
                    return Err(ProtocolError::InvalidLength);
                }
                Self::Describe { start, max }
            }
            _ => return Err(ProtocolError::InvalidKind),
        };
        r.finish()?;
        Ok(value)
    }
    /// Encode a property request into caller storage.
    /// # Errors
    /// Returns invalid field/length or insufficient capacity.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, ProtocolError> {
        let mut w = Writer::new(out);
        match *self {
            Self::Get { key } => w.u16(key)?,
            Self::Set {
                key,
                expected_revision,
                value,
            } => {
                w.u16(key)?;
                w.u32(expected_revision)?;
                w.u16(u16::try_from(value.len()).map_err(|_| ProtocolError::InvalidLength)?)?;
                w.bytes(value)?;
            }
            Self::Describe { start, max } => {
                w.u16(start)?;
                w.u8(max)?;
            }
        }
        let len = w.len();
        PropertyRequest::decode(self.opcode(), &out[..len])?;
        Ok(len)
    }
}
/// Exact BEGIN payload.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ObjectBegin {
    /// Module-local nonzero type.
    pub kind: u16,
    /// Stable ID in 1..FFFFFFFE.
    pub object_id: u32,
    /// Zero for creation, current nonzero revision for update.
    pub expected_revision: u32,
    /// Complete encoded length, up to 4096.
    pub total: u32,
    /// New identity versus existing-object edit.
    pub create: bool,
    /// Nonzero creation token; all zero on edit.
    pub creation_token: [u8; 16],
}
impl core::fmt::Debug for ObjectBegin {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ObjectBegin")
            .field("kind", &self.kind)
            .field("object_id", &self.object_id)
            .field("total", &self.total)
            .finish_non_exhaustive()
    }
}
/// Shared object command body, borrowing chunks without allocation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ObjectRequest<'a> {
    /// Start creation/edit.
    Begin(ObjectBegin),
    /// Contiguous object chunk.
    Write {
        /// Transaction identity.
        transaction: u32,
        /// Byte offset.
        offset: u32,
        /// Chunk bytes.
        bytes: &'a [u8],
    },
    /// Integrity-protected atomic commit.
    Commit {
        /// Transaction identity.
        transaction: u32,
        /// WOBJ CRC32C.
        crc: u32,
    },
    /// Revision-bound object read.
    Read {
        /// Stable identity.
        object_id: u32,
        /// Required object revision.
        expected_revision: u32,
        /// First byte.
        offset: u32,
        /// Maximum chunk size.
        max: u16,
    },
    /// Delete one ID or FFFFFFFF for an atomic collection clear.
    Delete {
        /// Stable ID or collection sentinel.
        object_id: u32,
        /// Object or collection revision.
        expected_revision: u32,
    },
    /// Inclusive ID enumeration.
    Enumerate {
        /// First stable ID.
        start: u32,
        /// Maximum entries.
        max: u8,
    },
    /// Discard staging without releasing reserved identities.
    Abort {
        /// Transaction identity.
        transaction: u32,
    },
    /// Query current immutable and revision metadata.
    Info {
        /// Stable identity.
        object_id: u32,
    },
    /// Resolve uncertain creation/commit by durable identity.
    IdState {
        /// Stable identity.
        object_id: u32,
    },
}
impl core::fmt::Debug for ObjectRequest<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ObjectRequest")
            .field("opcode", &self.opcode())
            .finish_non_exhaustive()
    }
}
fn nonzero(value: u32) -> Result<u32, ProtocolError> {
    if value == 0 {
        Err(ProtocolError::InvalidId)
    } else {
        Ok(value)
    }
}
fn object_id(value: u32) -> Result<u32, ProtocolError> {
    nonzero(value)?;
    if value == u32::MAX {
        Err(ProtocolError::InvalidId)
    } else {
        Ok(value)
    }
}
impl<'a> ObjectRequest<'a> {
    /// Module-independent object opcode.
    #[must_use]
    pub const fn opcode(&self) -> u8 {
        match self {
            Self::Begin(_) => 0x20,
            Self::Write { .. } => 0x21,
            Self::Commit { .. } => 0x22,
            Self::Read { .. } => 0x23,
            Self::Delete { .. } => 0x24,
            Self::Enumerate { .. } => 0x25,
            Self::Abort { .. } => 0x26,
            Self::Info { .. } => 0x27,
            Self::IdState { .. } => 0x28,
        }
    }
    /// Decode an exact object command. Physical backend and schema availability are separate.
    /// # Errors
    /// Rejects lengths, zero/reserved identities, illegal creation fields and unknown opcodes.
    pub fn decode(opcode: u8, bytes: &'a [u8]) -> Result<Self, ProtocolError> {
        if bytes.len() > crate::LOGICAL_MAX {
            return Err(ProtocolError::InvalidLength);
        }
        let mut r = Reader::new(bytes);
        let value = match opcode {
            0x20 => {
                let kind = r.u16()?;
                let id = object_id(r.u32()?)?;
                let revision = r.u32()?;
                let total = r.u32()?;
                let create = r.u8()?;
                let mut token = [0; 16];
                token.copy_from_slice(r.bytes(16)?);
                if kind == 0
                    || total > 4096
                    || create > 1
                    || (create == 1 && (revision != 0 || token == [0; 16]))
                    || (create == 0 && (revision == 0 || token != [0; 16]))
                {
                    return Err(ProtocolError::InvalidState);
                }
                Self::Begin(ObjectBegin {
                    kind,
                    object_id: id,
                    expected_revision: revision,
                    total,
                    create: create == 1,
                    creation_token: token,
                })
            }
            0x21 => {
                let transaction = nonzero(r.u32()?)?;
                let offset = r.u32()?;
                let n = usize::from(r.u16()?);
                if n == 0 {
                    return Err(ProtocolError::InvalidLength);
                }
                Self::Write {
                    transaction,
                    offset,
                    bytes: r.bytes(n)?,
                }
            }
            0x22 => Self::Commit {
                transaction: nonzero(r.u32()?)?,
                crc: r.u32()?,
            },
            0x23 => {
                let id = object_id(r.u32()?)?;
                let revision = nonzero(r.u32()?)?;
                let offset = r.u32()?;
                let max = r.u16()?;
                if max == 0 {
                    return Err(ProtocolError::InvalidLength);
                }
                Self::Read {
                    object_id: id,
                    expected_revision: revision,
                    offset,
                    max,
                }
            }
            0x24 => Self::Delete {
                object_id: nonzero(r.u32()?)?,
                expected_revision: nonzero(r.u32()?)?,
            },
            0x25 => {
                let start = r.u32()?;
                if start == u32::MAX {
                    return Err(ProtocolError::InvalidId);
                }
                let max = r.u8()?;
                if max == 0 {
                    return Err(ProtocolError::InvalidLength);
                }
                Self::Enumerate { start, max }
            }
            0x26 => Self::Abort {
                transaction: nonzero(r.u32()?)?,
            },
            0x27 => Self::Info {
                object_id: object_id(r.u32()?)?,
            },
            0x28 => Self::IdState {
                object_id: object_id(r.u32()?)?,
            },
            _ => return Err(ProtocolError::InvalidKind),
        };
        r.finish()?;
        Ok(value)
    }
    /// Encode a validated command into caller storage.
    /// # Errors
    /// Rejects invalid fields/lengths or insufficient output capacity.
    pub fn encode(&self, out: &mut [u8]) -> Result<usize, ProtocolError> {
        let mut w = Writer::new(out);
        match *self {
            Self::Begin(b) => {
                w.u16(b.kind)?;
                w.u32(b.object_id)?;
                w.u32(b.expected_revision)?;
                w.u32(b.total)?;
                w.u8(u8::from(b.create))?;
                w.bytes(&b.creation_token)?;
            }
            Self::Write {
                transaction,
                offset,
                bytes,
            } => {
                w.u32(transaction)?;
                w.u32(offset)?;
                w.u16(u16::try_from(bytes.len()).map_err(|_| ProtocolError::InvalidLength)?)?;
                w.bytes(bytes)?;
            }
            Self::Commit { transaction, crc } => {
                w.u32(transaction)?;
                w.u32(crc)?;
            }
            Self::Read {
                object_id,
                expected_revision,
                offset,
                max,
            } => {
                w.u32(object_id)?;
                w.u32(expected_revision)?;
                w.u32(offset)?;
                w.u16(max)?;
            }
            Self::Delete {
                object_id,
                expected_revision,
            } => {
                w.u32(object_id)?;
                w.u32(expected_revision)?;
            }
            Self::Enumerate { start, max } => {
                w.u32(start)?;
                w.u8(max)?;
            }
            Self::Abort { transaction } => w.u32(transaction)?,
            Self::Info { object_id } | Self::IdState { object_id } => w.u32(object_id)?,
        }
        let len = w.len();
        ObjectRequest::decode(self.opcode(), &out[..len])?;
        Ok(len)
    }
}
