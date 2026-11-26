//! Long L2CAP buffers shared by the Host. Each checked-out buffer has one owner.
use crate::host::{Packet, PacketPool};
use core::cell::Cell;
use embassy_sync::blocking_mutex::CriticalSectionMutex;
use static_cell::ConstStaticCell;

const MTU: usize = 251;
const BUFFERS: usize = 4;

struct Slot {
    initial: ConstStaticCell<[u8; MTU]>,
    returned: CriticalSectionMutex<Cell<Option<&'static mut [u8]>>>,
}

static SLOTS: [Slot; BUFFERS] = [const {
    Slot {
        initial: ConstStaticCell::new([0; MTU]),
        returned: CriticalSectionMutex::new(Cell::new(None)),
    }
}; BUFFERS];

/// Fixed-capacity Host pool for a 247-byte ATT MTU.
#[derive(Debug)]
pub struct HostPacketPool;

/// An exclusively owned buffer returned to its original slot on drop.
pub struct HostPacket {
    bytes: &'static mut [u8],
    slot: &'static Slot,
}

impl core::fmt::Debug for HostPacket {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("HostPacket").finish_non_exhaustive()
    }
}

impl PacketPool for HostPacketPool {
    type Packet = HostPacket;
    const MTU: usize = MTU;

    fn capacity() -> usize {
        BUFFERS
    }

    fn allocate() -> Option<HostPacket> {
        for slot in &SLOTS {
            if let Some(bytes) = slot.returned.lock(Cell::take) {
                bytes.fill(0);
                return Some(HostPacket { bytes, slot });
            }
            if let Some(bytes) = slot.initial.try_take() {
                return Some(HostPacket { bytes, slot });
            }
        }
        None
    }
}

impl Packet for HostPacket {}

impl AsRef<[u8]> for HostPacket {
    fn as_ref(&self) -> &[u8] {
        self.bytes
    }
}

impl AsMut<[u8]> for HostPacket {
    fn as_mut(&mut self) -> &mut [u8] {
        self.bytes
    }
}

impl Drop for HostPacket {
    fn drop(&mut self) {
        let bytes = core::mem::take(&mut self.bytes);
        self.slot.returned.lock(|slot| slot.set(Some(bytes)));
    }
}

#[cfg(test)]
mod tests {
    use super::{HostPacketPool, PacketPool};

    #[test]
    fn buffers_are_exclusive_and_reusable_after_release() {
        let mut packets: [Option<_>; 4] = core::array::from_fn(|_| HostPacketPool::allocate());
        assert!(packets.iter().all(Option::is_some));
        assert!(HostPacketPool::allocate().is_none());
        for packet in packets.iter_mut().flatten() {
            assert_eq!(packet.as_ref().len(), 251);
            packet.as_mut().fill(0x55);
        }
        packets[1] = None;
        assert!(HostPacketPool::allocate().is_some_and(|p| p.as_ref().iter().all(|b| *b == 0)));
        assert!(packets.iter().flatten().all(|p| p.as_ref()[250] == 0x55));
    }
}
