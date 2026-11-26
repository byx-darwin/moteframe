//! Owned event serialization at the HCI boundary.
use crate::{ControllerEvent, FrameKind};
#[cfg(feature = "data-length")]
const HCI_BUFFER_SIZE: usize = 256;
#[cfg(not(feature = "data-length"))]
const HCI_BUFFER_SIZE: usize = 64;
/// Per-read owned storage; no packet borrows a Controller or its backend.
#[derive(Clone)]
pub struct HciBuffer {
    pub(crate) bytes: [u8; HCI_BUFFER_SIZE],
    pub(crate) len: usize,
}
impl core::fmt::Debug for HciBuffer {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HciBuffer")
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}
impl HciBuffer {
    pub(crate) const fn new() -> Self {
        Self {
            bytes: [0; HCI_BUFFER_SIZE],
            len: 0,
        }
    }
    fn packet(&mut self, code: u8, payload: &[u8]) {
        self.bytes[0] = 4;
        self.bytes[1] = code;
        self.bytes[2] = u8::try_from(payload.len()).unwrap_or(0);
        self.bytes[3..3 + payload.len()].copy_from_slice(payload);
        self.len = 3 + payload.len();
    }
    pub(crate) fn encode(
        &mut self,
        event: &ControllerEvent,
        event_mask: u64,
        le_mask: u64,
    ) -> bool {
        match *event {
            ControllerEvent::Connected {
                id,
                peer,
                parameters,
            } => {
                if event_mask & (1 << 61) == 0 || le_mask & 1 == 0 {
                    return false;
                }
                let h = id.handle().to_le_bytes();
                let mut p = [0; 19];
                p[0] = 1;
                p[2..4].copy_from_slice(&h);
                p[4] = 1;
                p[5] = u8::from(peer.is_random());
                p[6..12].copy_from_slice(&peer.address());
                p[12..14].copy_from_slice(&parameters.interval().to_le_bytes());
                p[14..16].copy_from_slice(&parameters.latency().to_le_bytes());
                p[16..18].copy_from_slice(&parameters.timeout().to_le_bytes());
                self.packet(0x3e, &p);
            }
            ControllerEvent::Acl { id, frame } => {
                let flags = if frame.kind() == FrameKind::Start {
                    0x2000
                } else {
                    0x1000
                };
                let handle = (id.handle() | flags).to_le_bytes();
                self.bytes[..3].copy_from_slice(&[2, handle[0], handle[1]]);
                self.bytes[3..5].copy_from_slice(&u16::from(frame.as_pdu()[1]).to_le_bytes());
                self.bytes[5..5 + frame.payload().len()].copy_from_slice(frame.payload());
                self.len = 5 + frame.payload().len();
            }
            ControllerEvent::Completed { id, packets } => {
                let h = id.handle().to_le_bytes();
                let n = packets.to_le_bytes();
                self.packet(0x13, &[1, h[0], h[1], n[0], n[1]]);
            }
            ControllerEvent::Disconnected { id, reason } => {
                if event_mask & (1 << 4) == 0 {
                    return false;
                }
                let h = id.handle().to_le_bytes();
                self.packet(5, &[0, h[0], h[1], reason]);
            }
            #[cfg(feature = "data-length")]
            ControllerEvent::DataLengthChanged { id, limits } => {
                if event_mask & (1 << 61) == 0 || le_mask & (1 << 6) == 0 {
                    return false;
                }
                let mut p = [0; 11];
                p[0] = 7;
                for (bytes, field) in p[1..].chunks_exact_mut(2).zip([
                    id.handle(),
                    limits.tx_octets,
                    limits.tx_time,
                    limits.rx_octets,
                    limits.rx_time,
                ]) {
                    bytes.copy_from_slice(&field.to_le_bytes());
                }
                self.packet(0x3e, &p);
            }
            ControllerEvent::PhyUpdated { id, tx, rx } => {
                if event_mask & (1 << 61) == 0 || le_mask & (1 << 11) == 0 {
                    return false;
                }
                let [lo, hi] = id.handle().to_le_bytes();
                self.packet(0x3e, &[0x0c, 0, lo, hi, tx, rx]);
            }
            ControllerEvent::ParametersUpdated { id, parameters } => {
                if event_mask & (1 << 61) == 0 || le_mask & (1 << 2) == 0 {
                    return false;
                }
                let mut p = [0; 10];
                p[0] = 3;
                p[2..4].copy_from_slice(&id.handle().to_le_bytes());
                p[4..6].copy_from_slice(&parameters.interval().to_le_bytes());
                p[6..8].copy_from_slice(&parameters.latency().to_le_bytes());
                p[8..10].copy_from_slice(&parameters.timeout().to_le_bytes());
                self.packet(0x3e, &p);
            }
        }
        true
    }
}
