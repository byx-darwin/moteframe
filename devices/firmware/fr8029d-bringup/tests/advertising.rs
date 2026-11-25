//! Tests for the FR8029D exchange-memory advertising packet and event layout.
use fr8029d_bringup::advertising::{
    ADV_DATA, ADV_HEADER, CONNECTABLE_ADDRESS, CONNECTABLE_DATA, CONTROL_OFFSET, RX_COUNT,
    RX_OFFSET, SCAN_TX_OFFSET, TX_OFFSET, TX_PAYLOAD_OFFSET, advance_clock, advertising_control,
    advertising_slot, connectable_control, connectable_descriptors, receive_ring, slot_complete,
    status_slot, tx_descriptor,
};

#[test]
fn test_should_encode_legacy_nonconnectable_payload_for_the_baseband() {
    assert_eq!(ADV_DATA, b"\x02\x01\x06\x0b\x09FR8029D-1M");
    assert_eq!(ADV_HEADER, 0x1542);
    let control = advertising_control();
    assert_eq!(control[0], 0x0404);
    assert_eq!(control[0x30 / 2], 0x0008);
    assert_eq!(
        tx_descriptor(),
        [0, ADV_HEADER, TX_PAYLOAD_OFFSET, 0, 0, 0, 0, 0]
    );
}

#[test]
fn test_should_encode_the_split_28_bit_event_timestamp() {
    assert_eq!(advance_clock(0x0fff_fffe, 4), 2);
    assert_eq!(
        advertising_slot(0x0123_abcd),
        [
            0x2802,
            0xabcd,
            0x0123,
            0x0270,
            CONTROL_OFFSET / 4,
            0x0acf,
            0x0c00,
            0x0f00
        ]
    );
    assert_eq!(status_slot(0x0900_8002), 9);
    assert_eq!(status_slot(0x5f00_8002), 15);
}

#[test]
fn test_should_only_accept_a_completed_slot_with_known_status() {
    for incomplete in [0, 0x2802, 0x2832] {
        assert!(!slot_complete(incomplete));
    }
    assert!(slot_complete(0x281a));
}

#[test]
fn connectable_advertising_encodes_raw_pdu_types_and_the_linked_scan_response() {
    assert_eq!(CONNECTABLE_DATA, b"\x02\x01\x06\x0a\x09FR29-CONN");
    let control = connectable_control(CONNECTABLE_ADDRESS);
    assert_eq!(control[4..7], [0x809e, 0x0129, 0xc325]);
    assert_eq!(CONNECTABLE_ADDRESS, [0x9e, 0x80, 0x29, 0x01, 0x25, 0xc3]);
    assert_eq!(control[12], 0xf100);
    let [advertising, scan] = connectable_descriptors();
    assert_eq!(
        advertising,
        [SCAN_TX_OFFSET / 4, 0x1440, TX_PAYLOAD_OFFSET, 0, 0, 0, 0, 0]
    );
    assert_eq!(scan, [TX_OFFSET / 4, 0x0644, 0x1a82, 0, 0, 0, 0, 0]);
}

#[test]
fn cold_rx_ring_links_aligned_descriptors_and_independent_byte_buffers() {
    assert_eq!(RX_OFFSET / 4, 0x310);
    let ring = receive_ring();
    assert_eq!(RX_COUNT, ring.len());
    let links = [0x319, 0x322, 0x32b, 0x334, 0x33d, 0x310];
    let payloads = [0x1de9, 0x1eed, 0x1ff1, 0x20f5, 0x21f9, 0x22fd];
    for ((record, link), payload) in ring.into_iter().zip(links).zip(payloads) {
        let mut expected = [0; 18];
        expected[0] = link;
        expected[10] = payload;
        assert_eq!(record, expected);
    }
}
