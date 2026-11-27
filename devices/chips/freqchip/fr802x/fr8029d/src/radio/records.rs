//! FR8029D packet RAM descriptor and scheduler record encodings.

use super::CLOCK_MASK;

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
// Byte offsets in Packet RAM. Both TX descriptors use the same payload only
// while a single controller token owns it. RX buffers remain disjoint at 251.
const _: () = {
    assert!(RX_OFFSET as usize + RX_COUNT * RX_WORDS * 2 <= TX_OFFSET as usize);
    assert!(SCAN_TX_OFFSET + 16 <= TX_PAYLOAD_OFFSET);
    assert!(TX_PAYLOAD_OFFSET + 251 <= SCAN_PAYLOAD_OFFSET);
    assert!(SCAN_PAYLOAD_OFFSET < RX_PAYLOAD_BASE);
    assert!(RX_PAYLOAD_STRIDE >= 251 + 4);
    assert!(RX_PAYLOAD_BASE as usize + (RX_COUNT - 1) * RX_PAYLOAD_STRIDE as usize + 255 <= 0x4000);
};
/// Byte offset of the installed peripheral link control record.
pub const CONNECTION_CONTROL: u16 = 0x01e4;
/// Descriptor returned to the CPU by the transmitter.
pub const TX_DONE: u16 = 1 << 15;
/// Word-offset bits in the first TX descriptor halfword.
pub const TX_LINK_MASK: u16 = 0x3fff;
/// Descriptor returned to the CPU by the receiver.
pub const RX_READY: u16 = 1 << 15;
/// Receive hardware status halfword offset.
pub const RX_STATUS: usize = 2;
/// Receive PDU header halfword offset.
pub const RX_HEADER: usize = 4;
/// Receive coarse timestamp word offset.
pub const RX_COARSE: usize = 8;
/// Receive fine timestamp and link ID halfword offset.
pub const RX_FINE_AND_LINK: usize = 0x0c;
/// Receive payload byte offset field within a descriptor.
pub const RX_PAYLOAD: usize = 0x14;
/// Transmit PDU header halfword offset.
pub const TX_HEADER: usize = 2;
/// Transmit payload byte offset field within a descriptor.
pub const TX_PAYLOAD: usize = 4;
/// Hop state halfword offset within a connection control record.
pub const LINK_HOP: usize = 0x18;
/// Receive window halfword offset within a connection control record.
pub const LINK_RX_WINDOW: usize = 0x1e;
/// Transmit queue head halfword offset within a connection control record.
pub const LINK_TX_HEAD: usize = 0x24;
/// Receive event limit halfword offset within a connection control record.
pub const LINK_RX_LIMIT: usize = 0x2e;
/// Transmit event limit halfword offset within a connection control record.
pub const LINK_TX_LIMIT: usize = 0x30;
/// First halfword offset of the connection channel map.
pub const LINK_CHANNEL_MAP: usize = 0x32;
/// Event counter halfword offset within a connection control record.
pub const LINK_EVENT_COUNTER: usize = 0x60;

/// The coarse-countdown companion field is ten bits wide.
#[must_use]
pub const fn rx_fine_countdown(fine_and_link: u16) -> u16 {
    fine_and_link & 0x03ff
}

/// Extract the hardware link ID from a received timestamp.
#[must_use]
pub const fn rx_link_id(fine_and_link: u16) -> u16 {
    fine_and_link >> 11
}

/// Encode an empty, returned TX descriptor and its next word pointer.
#[must_use]
pub const fn returned_tx_descriptor(next_offset: u16) -> [u16; 8] {
    [TX_DONE | (next_offset / 4), 0, 0, 0, 0, 0, 0, 0]
}

/// Byte address of a descriptor in the installed RX ring.
#[must_use]
pub const fn rx_descriptor_address(index: usize) -> usize {
    super::PACKET_RAM + RX_OFFSET as usize + index * RX_WORDS * size_of::<u16>()
}

/// Initial FR8029D control fields for a peripheral connection on LE 1M.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinkControl {
    /// Over-the-air access address.
    pub access_address: u32,
    /// Initial CRC, with the low 24 bits used.
    pub crc_init: u32,
    /// Data channel map.
    pub channel_map: [u8; 5],
    /// CSA1 hop increment.
    pub hop: u8,
    /// Event duration limit in 625 us slots.
    pub event_limit: u16,
    /// Encoded receive window.
    pub receive_window: u16,
    /// Hardware link ID.
    pub link_id: u8,
    /// Byte offset of the first aligned TX descriptor.
    pub tx_offset: u16,
}

impl LinkControl {
    /// Encode the installed packet RAM control record.
    #[must_use]
    pub fn encode(self) -> [u16; 74] {
        let [aa0, aa1, aa2, aa3] = self.access_address.to_le_bytes();
        let [crc0, crc1, crc2, _] = self.crc_init.to_le_bytes();
        let [ch0, ch1, ch2, ch3, ch4] = self.channel_map;
        let mut words = [0; 74];
        words[0] = 0x0003;
        words[1] = u16::from_le_bytes([0x20, self.link_id]);
        // TX/RX PHY both use code 0 for 1M. The peripheral role is in word 0.
        words[0x06 / 2] = 0x1100;
        words[0x0e / 2] = u16::from_le_bytes([aa0, aa1]);
        words[0x10 / 2] = u16::from_le_bytes([aa2, aa3]);
        words[0x12 / 2] = u16::from_le_bytes([crc0, crc1]);
        words[0x14 / 2] = u16::from(crc2);
        words[LINK_HOP / 2] = 0x8000 | (u16::from(self.hop) << 8);
        words[0x1a / 2] = 7;
        words[LINK_RX_WINDOW / 2] = self.receive_window;
        words[LINK_TX_HEAD / 2] = self.tx_offset / 4;
        words[LINK_RX_LIMIT / 2] = self.event_limit;
        words[LINK_TX_LIMIT / 2] = self.event_limit;
        words[LINK_CHANNEL_MAP / 2] = u16::from_le_bytes([ch0, ch1]);
        words[(LINK_CHANNEL_MAP + 2) / 2] = u16::from_le_bytes([ch2, ch3]);
        words[(LINK_CHANNEL_MAP + 4) / 2] = u16::from(ch4);
        // The ROM peripheral initializer at 0x2a2bc sets the RAM buffer
        // capacity to 251 and leaves the following halfword at zero. The
        // length-update routine at 0x28140 updates software limits and the
        // event duration separately.
        words[0x38 / 2] = 0xfb;
        words
    }
}

/// Encode a connection scheduler slot using a precomputed duration.
#[must_use]
pub const fn connection_slot(
    coarse: u32,
    fine: u16,
    control_offset: u16,
    duration: u16,
) -> [u16; 8] {
    let [a, b, c, d] = (coarse & CLOCK_MASK).to_le_bytes();
    [
        0x2802,
        u16::from_le_bytes([a, b]),
        u16::from_le_bytes([c, d]),
        fine,
        control_offset / 4,
        duration,
        0,
        0x0f00,
    ]
}

/// Encode the legacy advertising control record for the address and filter bits.
#[must_use]
pub const fn advertising_control(address: [u8; 6], filter: u16) -> [u16; 74] {
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

/// Encode a terminal TX descriptor with a byte payload offset.
#[must_use]
pub const fn tx_descriptor(header: u16, payload_offset: u16) -> [u16; 8] {
    [0, header, payload_offset, 0, 0, 0, 0, 0]
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

/// Builds one descriptor in the fixed six-slot ring, with a private payload buffer.
#[must_use]
pub const fn receive_descriptor(index: u16) -> [u16; RX_WORDS] {
    debug_assert!(index < 6);
    let mut record = [0; RX_WORDS];
    record[0] = (RX_OFFSET + 36 * ((index + 1) % 6)) / 4;
    record[10] = RX_PAYLOAD_BASE + RX_PAYLOAD_STRIDE * index;
    record
}

/// Builds a ring of receive descriptors with six disjoint byte buffers.
#[must_use]
pub fn receive_ring() -> [[u16; RX_WORDS]; RX_COUNT] {
    let mut ring = [[0; RX_WORDS]; RX_COUNT];
    for (index, record) in (0_u16..).zip(&mut ring) {
        *record = receive_descriptor(index);
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
