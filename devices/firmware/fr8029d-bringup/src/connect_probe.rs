//! Capture of connection requests received by the legacy advertiser.

use super::{PACKET_RAM, diagnostic, increment, read8, read32, write_record, write32};
#[cfg(test)]
use super::{test_halfword as halfword, test_set_halfword as set_halfword};
#[cfg(not(test))]
use core::ptr::{read_volatile, write_volatile};
use core::sync::atomic::{Ordering, fence};
#[cfg(feature = "ble-gatt-dle")]
use fr8029d_bringup::advertising::receive_descriptor;
#[cfg(not(feature = "ble-gatt-dle"))]
use fr8029d_bringup::advertising::receive_ring;
use fr8029d_bringup::advertising::{
    RX_COUNT, RX_OFFSET, RX_WORDS, SCAN_TX_OFFSET, TX_OFFSET, connectable_descriptors,
};
use fr8029d_bringup::connection::{ConnectionRequest, PacketTime};

const RX_QUEUE: usize = 0x4000_0028;
const RX_READY: u16 = 1 << 15;
const INVALID_ADV_STATUS: u16 = 0x003d;
const CONNECT_REQUEST_BYTES: usize = 34;
const SCAN_REQUESTS: usize = 16;
const CONNECT_ACCEPTED: usize = 17;
const CONNECT_STATUS_REJECTS: usize = 36;
const CONNECT_DECODE_REJECTS: usize = 37;
const LAST_CONNECT_REJECTION: usize = 38;
const DECODER_REJECTION: u32 = 1 << 31;

#[derive(Debug)]
pub(super) struct Receiver {
    #[cfg(not(feature = "ble-gatt-dle"))]
    buffers: [u16; RX_COUNT],
    #[cfg(feature = "ble-gatt-dle")]
    held: u8,
    next: usize,
}

pub(super) struct Packet {
    pub header: u16,
    pub status: u16,
    pub time: PacketTime,
    pub link_id: u16,
    pub payload: [u8; CONNECT_REQUEST_BYTES],
}

// A completed descriptor is still CPU-owned while RX_READY is set. The
// held bitmap prevents taking that descriptor again on a later radio event.
// Reset discards the queue and rebuilds the entire ring.
#[cfg(feature = "ble-gatt-dle")]
#[derive(Debug)]
pub(super) struct HeldRx {
    pub header: u16,
    pub status: u16,
    pub time: PacketTime,
    pub link_id: u16,
    pub payload: RxBuffer,
}

#[cfg(feature = "ble-gatt-dle")]
#[derive(Debug)]
pub(super) struct RxBuffer {
    index: u8,
    offset: u16,
}

#[cfg(feature = "ble-gatt-dle")]
const _: () = assert!(RX_COUNT <= u8::BITS as usize);

#[cfg(feature = "ble-gatt-dle")]
impl RxBuffer {
    pub(super) fn byte(&self, index: usize) -> u8 {
        read8(PACKET_RAM + usize::from(self.offset) + index)
    }

    pub(super) fn release(self, receiver: &mut Receiver) {
        receiver.held &= !(1 << self.index);
        let address = descriptor(usize::from(self.index));
        fence(Ordering::Release);
        set_halfword(address, halfword(address) & !RX_READY);
    }
}

impl Receiver {
    #[cfg(feature = "ble-gatt-dle")]
    #[inline(never)]
    fn ready_link(&self, index: usize) -> Result<Option<HeldRx>, u32> {
        let address = descriptor(index);
        if self.held & (1 << index) != 0 || halfword(address) & RX_READY == 0 {
            return Ok(None);
        }
        fence(Ordering::Acquire);
        let offset = halfword(address + 0x14);
        if offset != receive_descriptor(u16::from(index.to_le_bytes()[0]))[10] {
            return Err(17);
        }
        let fine = halfword(address + 0x0c);
        Ok(Some(HeldRx {
            header: halfword(address + 4),
            status: halfword(address + 2),
            time: PacketTime {
                coarse: read32(address + 8) & super::CLOCK_MASK,
                fine_countdown: fine & 0x03ff,
            },
            link_id: fine >> 11,
            payload: RxBuffer {
                index: index.to_le_bytes()[0],
                offset,
            },
        }))
    }

    /// Observe CPU-owned descriptors before `EVENT_END` without consuming them.
    #[cfg(feature = "ble-gatt-dle")]
    pub(super) fn ready_time(
        &self,
        matches: impl Fn(&HeldRx) -> bool,
    ) -> Result<Option<PacketTime>, u32> {
        let mut index = self.next;
        for _ in 0..RX_COUNT {
            let Some(packet) = self.ready_link(index)? else {
                break;
            };
            if matches(&packet) {
                return Ok(Some(packet.time));
            }
            index = (index + 1) % RX_COUNT;
        }
        Ok(None)
    }

    /// Takes ownership of a data buffer without copying it into the event queue.
    #[cfg(feature = "ble-gatt-dle")]
    pub(super) fn take_link(&mut self) -> Result<Option<HeldRx>, u32> {
        let Some(packet) = self.ready_link(self.next)? else {
            return Ok(None);
        };
        self.held |= 1 << self.next;
        self.next = (self.next + 1) % RX_COUNT;
        Ok(Some(packet))
    }

    pub(super) fn start() -> Self {
        let [advertising, scan_response] = connectable_descriptors();
        write_record(PACKET_RAM + usize::from(TX_OFFSET), &advertising);
        write_record(PACKET_RAM + usize::from(SCAN_TX_OFFSET), &scan_response);
        #[cfg(feature = "ble-gatt-dle")]
        for (index, slot) in (0_u16..).zip(0..RX_COUNT) {
            write_record(descriptor(slot), &receive_descriptor(index));
        }
        #[cfg(not(feature = "ble-gatt-dle"))]
        let ring = receive_ring();
        #[cfg(not(feature = "ble-gatt-dle"))]
        for (index, record) in ring.iter().enumerate() {
            write_record(descriptor(index), record);
        }
        fence(Ordering::Release);
        write32(RX_QUEUE + 4, 0);
        write32(RX_QUEUE, u32::from(RX_OFFSET / 4));
        Self {
            #[cfg(not(feature = "ble-gatt-dle"))]
            buffers: ring.map(|record| record[10]),
            #[cfg(feature = "ble-gatt-dle")]
            held: 0,
            next: 0,
        }
    }

    #[cfg(feature = "ble-gatt-dle")]
    pub(super) fn pop(&mut self) -> Result<Option<Packet>, u32> {
        let Some(packet) = self.take_link()? else {
            return Ok(None);
        };
        let mut payload = [0; CONNECT_REQUEST_BYTES];
        let length = usize::from(packet.header >> 8);
        if let Some(bytes) = payload.get_mut(..length) {
            for (index, byte) in bytes.iter_mut().enumerate() {
                *byte = packet.payload.byte(index);
            }
        }
        packet.payload.release(self);
        Ok(Some(Packet {
            header: packet.header,
            status: packet.status,
            time: packet.time,
            link_id: packet.link_id,
            payload,
        }))
    }

    /// Copies the completed descriptor before returning its buffer to the baseband.
    #[cfg(not(feature = "ble-gatt-dle"))]
    pub(super) fn pop(&mut self) -> Result<Option<Packet>, u32> {
        let address = descriptor(self.next);
        let link = halfword(address);
        if link & RX_READY == 0 {
            return Ok(None);
        }
        fence(Ordering::Acquire);
        let status = halfword(address + 2);
        let header = halfword(address + 4);
        let buffer = halfword(address + 0x14);
        if buffer != self.buffers[self.next] {
            return Err(17);
        }
        let fine = halfword(address + 0x0c);
        let mut packet = Packet {
            header,
            status,
            time: PacketTime {
                coarse: read32(address + 8) & super::CLOCK_MASK,
                fine_countdown: fine & 0x03ff,
            },
            link_id: fine >> 11,
            payload: [0; CONNECT_REQUEST_BYTES],
        };
        let length = usize::from(header >> 8);
        if length <= packet.payload.len() {
            for (index, byte) in packet.payload[..length].iter_mut().enumerate() {
                *byte = read8(PACKET_RAM + usize::from(buffer) + index);
            }
        }
        fence(Ordering::Release);
        set_halfword(address, link & !RX_READY);
        self.next = (self.next + 1) % RX_COUNT;
        Ok(Some(packet))
    }

    /// Inspects the ring between completed advertising events.
    pub(super) fn drain(
        &mut self,
        advertiser: [u8; 6],
    ) -> Result<Option<(ConnectionRequest, PacketTime)>, u32> {
        for _ in 0..RX_COUNT {
            let Some(packet) = self.pop()? else {
                break;
            };
            if let Some(request) = Self::inspect_advertisement(&packet, advertiser) {
                return Ok(Some((request, packet.time)));
            }
        }
        Ok(None)
    }

    pub(super) fn inspect_advertisement(
        packet: &Packet,
        advertiser: [u8; 6],
    ) -> Option<ConnectionRequest> {
        #[cfg(feature = "ble-gatt-dle")]
        if packet.link_id != 0 {
            return None;
        }
        let kind = packet.header & 0xf;
        if packet.status & INVALID_ADV_STATUS != 0 {
            if kind == 5 {
                increment(CONNECT_STATUS_REJECTS);
                diagnostic(LAST_CONNECT_REJECTION, u32::from(packet.status));
            }
            return None;
        }
        if kind == 3 {
            increment(SCAN_REQUESTS);
        }
        if kind != 5 {
            return None;
        }
        let request = match ConnectionRequest::decode(packet.header, &packet.payload, advertiser) {
            Ok(request) => request,
            Err(error) => {
                increment(CONNECT_DECODE_REJECTS);
                diagnostic(
                    LAST_CONNECT_REJECTION,
                    DECODER_REJECTION | (error as u32 + 1),
                );
                return None;
            }
        };
        increment(CONNECT_ACCEPTED);
        diagnostic(18, u32::from(packet.header));
        diagnostic(19, packet.time.coarse);
        diagnostic(20, u32::from(packet.time.fine_countdown));
        for (index, bytes) in packet.payload.chunks(4).enumerate() {
            let mut word = [0; 4];
            word[..bytes.len()].copy_from_slice(bytes);
            diagnostic(21 + index, u32::from_le_bytes(word));
        }
        Some(request)
    }
}

fn descriptor(index: usize) -> usize {
    PACKET_RAM + usize::from(RX_OFFSET) + index * RX_WORDS * size_of::<u16>()
}

#[cfg(not(test))]
fn set_halfword(address: usize, value: u16) {
    // SAFETY: The event returned this aligned RX descriptor to the CPU.
    unsafe { write_volatile(address as *mut u16, value) };
}

#[cfg(not(test))]
fn halfword(address: usize) -> u16 {
    // SAFETY: All call sites read aligned halfwords in the installed ring.
    unsafe { read_volatile(address as *const u16) }
}
