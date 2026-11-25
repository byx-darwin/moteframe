//! Fixed SDK event-prefix regression checks.
use ble_syd881x::{DecodeError, decode_event};
use device_ble::{Channel, Event, Packet};

#[test]
fn test_should_copy_unaligned_write_event_without_borrowing_vendor_memory() {
    let mut bytes = [0, 0, 2, 0, 0, 1, 0, 2, 0, 0x34, 0x12, 3, 0, 9, 8, 7];
    let event = decode_event(&bytes);
    bytes.fill(0);
    assert_eq!(
        event,
        Ok(Some(Event::Received {
            channel: Channel(0x1234),
            packet: Packet::try_from_slice(&[9, 8, 7]).ok().unwrap_or_default(),
        }))
    );
}
#[test]
fn test_should_reject_truncated_or_header_only_oversize_events() {
    for bytes in [
        &[][..],
        &[0, 0, 2, 0][..],
        &[0, 0, 2, 0, 0][..],
        &[0, 0, 2, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0][..],
    ] {
        assert_eq!(decode_event(bytes), Err(DecodeError::Truncated));
    }
    let mut bytes = [0; 201];
    bytes[2] = 2;
    bytes[11] = 186;
    assert_eq!(decode_event(&bytes), Err(DecodeError::PayloadTooLarge));
    bytes[11] = 185;
    assert!(matches!(
        decode_event(&bytes),
        Ok(Some(Event::Received { .. }))
    ));
}
#[test]
fn test_should_decode_connection_and_ignore_unknown_events() {
    assert_eq!(decode_event(&[0, 2, 0, 0, 0]), Ok(Some(Event::Connected)));
    assert_eq!(
        decode_event(&[0, 4, 0, 0, 0, 19]),
        Ok(Some(Event::Disconnected { reason: 19 }))
    );
    assert_eq!(decode_event(&[0, 4, 0, 0, 0]), Err(DecodeError::Truncated));
    assert_eq!(decode_event(&[0, 0, 0, 0, 0]), Ok(None));
}

#[test]
fn test_should_ignore_other_event_domains_even_with_matching_gap_code() {
    assert_eq!(decode_event(&[1, 2, 0, 0, 0]), Ok(None));
}
