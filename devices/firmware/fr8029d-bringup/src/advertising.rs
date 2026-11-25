//! Exchange-memory records for the board-specific 1M advertising experiment.

/// Fixed static-random lab address in Bluetooth byte order.
pub const ADV_ADDRESS: [u8; 6] = [0x9d, 0x80, 0x29, 0x01, 0x25, 0xc3];
const ADV_DATA_LEN: u16 = 15;
/// Flags and complete local name in the advertising packet.
pub const ADV_DATA: &[u8; ADV_DATA_LEN as usize] = b"\x02\x01\x06\x0b\x09FR8029D-1M";
/// Legacy `ADV_NONCONN_IND` header with `TxAdd=1` and six address bytes.
pub const ADV_HEADER: u16 = ((6 + ADV_DATA_LEN) << 8) | 0x42;
/// Control record byte offset in Packet RAM.
pub const CONTROL_OFFSET: u16 = 0x0150;
/// Transmit descriptor byte offset in Packet RAM.
pub const TX_OFFSET: u16 = 0x0d18;
/// Advertising data byte offset in Packet RAM.
pub const TX_PAYLOAD_OFFSET: u16 = 0x1984;
/// Scan response descriptor byte offset in Packet RAM.
pub const SCAN_TX_OFFSET: u16 = 0x0d28;
const SCAN_PAYLOAD_OFFSET: u16 = 0x1a82;
/// Byte offset of the first receive descriptor.
pub const RX_OFFSET: u16 = 0x0c40;
/// Number of descriptors in the advertising receive ring.
pub const RX_COUNT: usize = 6;
/// Number of halfwords in each receive descriptor.
pub const RX_WORDS: usize = 18;
const RX_PAYLOAD_BASE: u16 = 0x1de9;
const RX_PAYLOAD_STRIDE: u16 = 260;
/// Separate lab address to avoid scanner caches from earlier probes.
pub const CONNECTABLE_ADDRESS: [u8; 6] = [0x9e, 0x80, 0x29, 0x01, 0x25, 0xc3];
/// Advertising flags and name for the connectable receive probe.
pub const CONNECTABLE_DATA: &[u8; 14] = b"\x02\x01\x06\x0a\x09FR29-CONN";
/// Scheduler entry stride in bytes.
pub const SLOT_STRIDE: usize = 16;
/// Maximum value of the scheduler's coarse clock.
pub const CLOCK_MASK: u32 = 0x0fff_ffff;

/// Builds the control record for the one legacy advertising event.
#[must_use]
pub const fn advertising_control() -> [u16; 74] {
    control_for(ADV_ADDRESS, 0xf102)
}

/// Builds the legacy connectable advertising control record.
#[must_use]
pub const fn connectable_control(address: [u8; 6]) -> [u16; 74] {
    control_for(address, 0xf100)
}

const fn control_for(address: [u8; 6], filter: u16) -> [u16; 74] {
    let mut control = [0; 74];
    // The baseband legacy-mode selector is independent of the PDU type.
    control[0] = 0x0404;
    control[1] = 0x0021;
    control[4] = u16::from_le_bytes([address[0], address[1]]);
    control[5] = u16::from_le_bytes([address[2], address[3]]);
    control[6] = u16::from_le_bytes([address[4], address[5]]);
    control[7] = 0xbed6;
    control[8] = 0x8e89;
    control[9] = 0x5555;
    control[10] = 0x0055;
    control[11] = 0x0011;
    control[12] = filter;
    control[13] = 0x0007;
    control[18] = TX_OFFSET / 4;
    control[24] = 0x0008;
    control[27] = 0x00e0;
    control
}

/// Builds a single-PDU descriptor with a null next pointer and a byte payload offset.
#[must_use]
pub const fn tx_descriptor() -> [u16; 8] {
    [0, ADV_HEADER, TX_PAYLOAD_OFFSET, 0, 0, 0, 0, 0]
}

/// Builds the linked `ADV_IND` and empty `SCAN_RSP` descriptors.
#[must_use]
pub const fn connectable_descriptors() -> [[u16; 8]; 2] {
    // Both PDUs carry the random AdvA from the control record. With ChSel
    // cleared the advertiser only offers Channel Selection Algorithm #1.
    [
        [SCAN_TX_OFFSET / 4, 0x1440, TX_PAYLOAD_OFFSET, 0, 0, 0, 0, 0],
        [TX_OFFSET / 4, 0x0644, SCAN_PAYLOAD_OFFSET, 0, 0, 0, 0, 0],
    ]
}

/// Builds a ring of receive descriptors with six disjoint byte buffers.
#[must_use]
pub fn receive_ring() -> [[u16; RX_WORDS]; RX_COUNT] {
    const STRIDE_BYTES: u16 = 36;
    let mut ring = [[0; RX_WORDS]; RX_COUNT];
    for (index, record) in (0_u16..).zip(&mut ring) {
        let next = (index + 1) % 6;
        record[0] = (RX_OFFSET + STRIDE_BYTES * next) / 4;
        record[10] = RX_PAYLOAD_BASE + RX_PAYLOAD_STRIDE * index;
    }
    ring
}

/// Encodes an advertising event with known fixed timing fields and a scheduled clock.
#[must_use]
pub const fn advertising_slot(clock: u32) -> [u16; 8] {
    let bytes = (clock & CLOCK_MASK).to_le_bytes();
    [
        0x2802,
        u16::from_le_bytes([bytes[0], bytes[1]]),
        u16::from_le_bytes([bytes[2], bytes[3]]),
        0x0270,
        CONTROL_OFFSET / 4,
        0x0acf,
        0x0c00,
        0x0f00,
    ]
}

/// Advances the half-slot clock, including wraparound.
#[must_use]
pub const fn advance_clock(clock: u32, ticks: u32) -> u32 {
    clock.wrapping_add(ticks) & CLOCK_MASK
}

/// Reads the last completed event index from scheduler status.
#[must_use]
pub const fn status_slot(status: u32) -> u32 {
    (status >> 24) & 0xf
}

/// Accepts the normal completion status seen in the stock ring.
#[must_use]
pub const fn slot_complete(flags: u16) -> bool {
    flags & 0x38 == 0x18
}
