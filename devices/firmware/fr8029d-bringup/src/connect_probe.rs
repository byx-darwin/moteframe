//! Capture of connection requests received by the legacy advertiser.

use super::{PACKET_RAM, diagnostic, read8, read32, write_record, write32};
use core::{
    ptr::{read_volatile, write_volatile},
    sync::atomic::{Ordering, fence},
};
use fr8029d_bringup::advertising::{
    RX_COUNT, RX_OFFSET, RX_WORDS, SCAN_TX_OFFSET, TX_OFFSET, connectable_descriptors, receive_ring,
};
use fr8029d_bringup::connection::{ConnectionRequest, PacketTime};

const RX_QUEUE: usize = 0x4000_0028;
const RX_READY: u16 = 1 << 15;
const INVALID_ADV_STATUS: u16 = 0x003d;
const CONNECT_REQUEST_BYTES: usize = 34;

#[derive(Debug)]
pub(super) struct Receiver {
    scanned: u32,
    requests: u32,
    buffers: [u16; RX_COUNT],
    next: usize,
}

pub(super) struct Packet {
    pub header: u16,
    pub status: u16,
    pub time: PacketTime,
    pub link_id: u16,
    pub payload: [u8; CONNECT_REQUEST_BYTES],
}

impl Receiver {
    pub(super) fn start() -> Self {
        let [advertising, scan_response] = connectable_descriptors();
        write_record(PACKET_RAM + usize::from(TX_OFFSET), &advertising);
        write_record(PACKET_RAM + usize::from(SCAN_TX_OFFSET), &scan_response);
        let ring = receive_ring();
        for (index, record) in ring.iter().enumerate() {
            write_record(descriptor(index), record);
        }
        fence(Ordering::Release);
        write32(RX_QUEUE + 4, 0);
        write32(RX_QUEUE, u32::from(RX_OFFSET / 4));
        Self {
            scanned: 0,
            requests: 0,
            buffers: ring.map(|record| record[10]),
            next: 0,
        }
    }

    /// Copies the completed descriptor before returning its buffer to the baseband.
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
        // SAFETY: The event returned this descriptor. Preserve its next pointer.
        unsafe { write_volatile(address as *mut u16, link & !RX_READY) };
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
            if packet.status & INVALID_ADV_STATUS != 0 {
                continue;
            }
            if packet.header & 0xf == 3 {
                self.scanned += 1;
                diagnostic(16, self.scanned);
            }
            if let Ok(request) =
                ConnectionRequest::decode(packet.header, &packet.payload, advertiser)
            {
                self.requests += 1;
                diagnostic(17, self.requests);
                diagnostic(18, u32::from(packet.header));
                diagnostic(19, packet.time.coarse);
                diagnostic(20, u32::from(packet.time.fine_countdown));
                for (index, bytes) in packet.payload.chunks(4).enumerate() {
                    let mut word = [0; 4];
                    word[..bytes.len()].copy_from_slice(bytes);
                    diagnostic(21 + index, u32::from_le_bytes(word));
                }
                return Ok(Some((request, packet.time)));
            }
        }
        Ok(None)
    }
}

fn descriptor(index: usize) -> usize {
    PACKET_RAM + usize::from(RX_OFFSET) + index * RX_WORDS * size_of::<u16>()
}

fn halfword(address: usize) -> u16 {
    // SAFETY: All call sites read aligned halfwords in the installed ring.
    unsafe { read_volatile(address as *const u16) }
}
