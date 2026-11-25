//! Frozen ordinary control hints. Actual mutations and per-resource queue authorization are external.
use device_protocol::{Frame, Header, Kind, ProtocolError};
/// Real committed object transition, distinct from collection revision or transaction state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Change {
    /// Actual create/update committed.
    Updated = 1,
    /// Actual tombstone committed.
    Deleted = 2,
}
/// Common control hints only; constructing one never commits data or proves a hardware observation.
/// The owning runtime must bind the actual dataset/module scope and recheck it at queue admission.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ControlEvent {
    /// Actual power observation with per-field validity; missing battery data stays unavailable.
    #[cfg(feature = "device")]
    Battery {
        /// Actual observation, validated against the same battery value contract.
        state: crate::BatteryState,
    },
    /// Immutable capability snapshot changed; the client must reread MODULES without mixing revisions.
    CapabilityChanged {
        /// Actual nonzero capability revision.
        revision: u32,
    },
    /// A property really changed; the client rereads actual state.
    PropertyChanged {
        /// Owning registered business module.
        module: u8,
        /// Module-local key, never FFFF.
        key: u16,
        /// Actual nonzero property revision.
        revision: u32,
    },
    /// A committed stable object changed; this is not a durable phone-save acknowledgement.
    ObjectChanged {
        /// Owning registered business module.
        module: u8,
        /// Stable ID1..FFFFFFFE within the current dataset and module.
        object_id: u32,
        /// Actual object revision, distinct from collection/birth revisions.
        revision: u32,
        /// Real committed transition.
        change: Change,
    },
}
impl core::fmt::Debug for ControlEvent {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ControlEvent").finish_non_exhaustive()
    }
}
impl ControlEvent {
    /// Encode one complete baseline-size control event, without a response prefix or application CRC.
    /// Retries must reuse the same event ID and exact bytes until actual FIFO admission. Control event
    /// IDs use the connection's ordinary-event allocator, not its nonreusing DATA message sequence.
    /// # Errors
    /// Invalid identity/revision/key or insufficient caller space; no partial output is published.
    pub fn encode(self, message_id: u16, out: &mut [u8]) -> Result<usize, ProtocolError> {
        let mut body = [0; 9];
        let (module, opcode, length) = self.encode_body(&mut body)?;
        let frame = Frame {
            header: Header {
                kind: Kind::Event,
                module,
                opcode,
                message_id,
                offset: 0,
                total_length: length,
            },
            payload: &body[..usize::from(length)],
        };
        frame.encode(out)
    }
    /// Encode only the typed ordinary-event body and return its exact route and length.
    /// This does not allocate an event ID or claim that a queue accepted a fragment.
    /// # Errors
    /// Rejects invalid fields and insufficient caller storage before writing output.
    pub fn encode_body(self, out: &mut [u8]) -> Result<(u8, u8, u16), ProtocolError> {
        let mut body = [0; 9];
        let (module, opcode, length) = match self {
            #[cfg(feature = "device")]
            Self::Battery { state } => {
                state.encode(&mut body)?;
                (2, 0x80, 8)
            }
            Self::CapabilityChanged { revision } => {
                nonzero(revision)?;
                body[..4].copy_from_slice(&revision.to_le_bytes());
                (0, 0x80, 4u16)
            }
            Self::PropertyChanged {
                module,
                key,
                revision,
            } => {
                nonzero(revision)?;
                if key == u16::MAX {
                    return Err(ProtocolError::InvalidId);
                }
                body[..2].copy_from_slice(&key.to_le_bytes());
                body[2..6].copy_from_slice(&revision.to_le_bytes());
                (module, 0x90, 6)
            }
            Self::ObjectChanged {
                module,
                object_id,
                revision,
                change,
            } => {
                nonzero(revision)?;
                nonzero(object_id)?;
                if object_id == u32::MAX {
                    return Err(ProtocolError::InvalidId);
                }
                body[..4].copy_from_slice(&object_id.to_le_bytes());
                body[4..8].copy_from_slice(&revision.to_le_bytes());
                body[8] = change as u8;
                (module, 0x91, 9)
            }
        };
        let destination = out
            .get_mut(..usize::from(length))
            .ok_or(ProtocolError::Capacity)?;
        destination.copy_from_slice(&body[..usize::from(length)]);
        Ok((module, opcode, length))
    }
}
fn nonzero(value: u32) -> Result<(), ProtocolError> {
    if value == 0 {
        Err(ProtocolError::InvalidId)
    } else {
        Ok(())
    }
}
