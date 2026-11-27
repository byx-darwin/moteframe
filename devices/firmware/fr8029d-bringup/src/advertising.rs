//! Exchange-memory records for the board-specific 1M advertising experiment.

/// Fixed static-random lab address in Bluetooth byte order.
pub const ADV_ADDRESS: [u8; 6] = [0x9d, 0x80, 0x29, 0x01, 0x25, 0xc3];
const ADV_DATA_LEN: u16 = 15;
/// Flags and complete local name in the advertising packet.
pub const ADV_DATA: &[u8; ADV_DATA_LEN as usize] = b"\x02\x01\x06\x0b\x09FR8029D-1M";
/// Legacy `ADV_NONCONN_IND` header with `TxAdd=1` and six address bytes.
pub const ADV_HEADER: u16 = ((6 + ADV_DATA_LEN) << 8) | 0x42;
/// Separate lab address to avoid scanner caches from earlier probes.
pub const CONNECTABLE_ADDRESS: [u8; 6] = [0x9e, 0x80, 0x29, 0x01, 0x25, 0xc3];
/// Advertising flags and name for the connectable receive probe.
pub const CONNECTABLE_DATA: &[u8; 14] = b"\x02\x01\x06\x0a\x09FR29-CONN";
pub use fr8029d_chip::radio::{CLOCK_MASK, SLOT_STRIDE};

/// Builds the control record for the one legacy advertising event.
#[must_use]
pub const fn advertising_control() -> [u16; 74] {
    records::advertising_control(ADV_ADDRESS, 0xf102)
}

/// Builds the legacy connectable advertising control record.
#[must_use]
pub const fn connectable_control(address: [u8; 6]) -> [u16; 74] {
    records::advertising_control(address, 0xf100)
}

use fr8029d_chip::radio::records;
pub use fr8029d_chip::radio::records::{
    CONTROL_OFFSET, RX_COUNT, RX_OFFSET, RX_WORDS, SCAN_TX_OFFSET, TX_OFFSET, TX_PAYLOAD_OFFSET,
    advertising_slot, connectable_descriptors, receive_descriptor, receive_ring, slot_complete,
    status_slot,
};

/// Builds the non-connectable advertising probe descriptor.
#[must_use]
pub const fn tx_descriptor() -> [u16; 8] {
    records::tx_descriptor(ADV_HEADER, TX_PAYLOAD_OFFSET)
}

/// Advances the half-slot clock, including wraparound.
#[must_use]
pub const fn advance_clock(clock: u32, ticks: u32) -> u32 {
    clock.wrapping_add(ticks) & CLOCK_MASK
}
