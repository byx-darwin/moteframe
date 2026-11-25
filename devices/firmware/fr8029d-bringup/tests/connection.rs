//! Host tests for a legacy LE 1M peripheral connection.
use fr8029d_bringup::connection::{ConnectError, ConnectionRequest, ConnectionUpdate, PacketTime};

const ADV_ADDRESS: [u8; 6] = [0x9e, 0x80, 0x29, 0x01, 0x25, 0xc3];
const REQUEST: [u8; 34] = [
    1, 2, 3, 4, 5, 0xc6, // InitA
    0x9e, 0x80, 0x29, 0x01, 0x25, 0xc3, // AdvA
    0x78, 0x56, 0x34, 0x12, // Access Address
    0xc0, 0xab, 0x12, // CRCInit
    2,    // WinSize
    3, 0, // WinOffset
    0x18, 0, // Interval
    2, 0, // Latency
    0xc8, 0, // Timeout
    0xfe, 0xff, 0xff, 0xff, 0x1f, // ChM
    0xe5, // SCA 7, hop 5
];

#[test]
fn test_should_decode_legacy_connection_little_endian() -> Result<(), ConnectError> {
    let request = ConnectionRequest::decode(0x22e5, &REQUEST, ADV_ADDRESS)?;
    assert_eq!(request.initiator, [1, 2, 3, 4, 5, 0xc6]);
    assert!(request.initiator_random);
    assert_eq!(request.access_address, 0x1234_5678);
    assert_eq!(request.crc_init, 0x12_ab_c0);
    assert_eq!(request.window_size, 2);
    assert_eq!(request.window_offset, 3);
    assert_eq!(request.interval, 24);
    assert_eq!(request.latency, 2);
    assert_eq!(request.supervision_timeout, 200);
    assert_eq!(request.channel_map, [0xfe, 0xff, 0xff, 0xff, 0x1f]);
    assert_eq!(request.hop, 5);
    assert_eq!(request.sca, 7);
    Ok(())
}

#[test]
fn test_should_reject_wrong_pdu_and_invalid_parameters() {
    for header in [0x22c3, 0x2185, 0x2205] {
        assert_eq!(
            ConnectionRequest::decode(header, &REQUEST, ADV_ADDRESS),
            Err(ConnectError::Header)
        );
    }
    let mut address = ADV_ADDRESS;
    address[0] ^= 1;
    assert_eq!(
        ConnectionRequest::decode(0x2285, &REQUEST, address),
        Err(ConnectError::Advertiser)
    );
    for (index, byte, error) in [
        (19, 0, ConnectError::Window),
        (19, 9, ConnectError::Window),
        (20, 25, ConnectError::Window),
        (22, 5, ConnectError::Interval),
        (25, 2, ConnectError::Supervision),
        (26, 18, ConnectError::Supervision),
        (32, 0x20, ConnectError::ChannelMap),
        (33, 0xe4, ConnectError::Hop),
    ] {
        let mut payload = REQUEST;
        payload[index] = byte;
        assert_eq!(
            ConnectionRequest::decode(0x2285, &payload, ADV_ADDRESS),
            Err(error)
        );
    }
    let mut payload = REQUEST;
    payload[28..33].copy_from_slice(&[0, 0, 0, 0, 0x10]);
    assert_eq!(
        ConnectionRequest::decode(0x2285, &payload, ADV_ADDRESS),
        Err(ConnectError::ChannelMap)
    );
}

#[test]
fn test_should_encode_first_receive_time_across_coarse_wrap() -> Result<(), ConnectError> {
    let request = ConnectionRequest::decode(0x2285, &REQUEST, ADV_ADDRESS)?;
    let time = PacketTime {
        coarse: 0x0fff_fffd,
        fine_countdown: 600,
    };
    let event = request.first_event(time);
    // The first possible anchor is 24 - 94 + 704 + 2500 * (1 + 3)
    // half-microseconds after the initial coarse boundary. Open 71
    // half-microseconds earlier (64 guard plus 7 clock widening).
    assert_eq!(event.start.coarse, 13);
    assert_eq!(event.start.fine_countdown, 61);
    assert_eq!(event.receive_window, 1293);
    assert_eq!(
        event.slot_words(0x01e4),
        [0x2802, 13, 0, 61, 0x79, 3537, 0, 0x0f00]
    );
    let control = request.control_words(1, 0x0d38, event);
    for (offset, word) in [
        (0x00, 3),
        (0x02, 0x0120),
        (0x06, 0x1100),
        (0x0e, 0x5678),
        (0x10, 0x1234),
        (0x12, 0xabc0),
        (0x14, 0x0012),
        (0x18, 0x8500),
        (0x1a, 7),
        (0x1e, 1293),
        (0x24, 0x034e),
        (0x2e, 45),
        (0x30, 45),
        (0x32, 0xfffe),
        (0x34, 0xffff),
        (0x36, 0x001f),
        (0x38, 0x00fb),
        (0x60, 0),
    ] {
        assert_eq!(control[offset / 2], word, "control at {offset:#04x}");
    }
    Ok(())
}

#[test]
fn test_should_derive_data_anchor_without_adding_pdu_airtime() -> Result<(), ConnectError> {
    let request = ConnectionRequest::decode(0x2285, &REQUEST, ADV_ADDRESS)?;
    let anchor = PacketTime {
        coarse: 0,
        fine_countdown: 604,
    }
    .data_anchor();
    assert_eq!(anchor.coarse, 0x0fff_ffff);
    assert_eq!(anchor.fine_countdown, 73);
    let next = request.event_after(anchor, 1);
    // A 30 ms interval is 96 coarse ticks. At 520 ppm the one-sided
    // allowance is 32 half-microseconds in addition to the 64-tick guard.
    assert_eq!(next.start.coarse, 95);
    assert_eq!(next.start.fine_countdown, 169);
    assert_eq!(next.receive_window, 55);
    Ok(())
}

#[test]
fn test_should_decode_and_schedule_the_captured_50_millisecond_interval() -> Result<(), ConnectError>
{
    // The air trace supplies LLData and RX time. Use a synthetic initiator address.
    let mut payload = REQUEST;
    payload[12..].copy_from_slice(&[
        0xca, 0xca, 0xc0, 0x22, 0xbd, 0x2d, 0x4e, 2, 0x1d, 0, 0x28, 0, 0, 0, 0xf4, 1, 0xff, 0xff,
        0xff, 0xff, 0x1f, 0x2d,
    ]);
    let request = ConnectionRequest::decode(0x22c5, &payload, ADV_ADDRESS)?;
    assert_eq!(request.access_address, 0x22c0_caca);
    assert_eq!(request.crc_init, 0x4e_2d_bd);
    assert_eq!(request.interval, 40);
    assert_eq!(request.window_offset, 29);
    assert_eq!(request.supervision_timeout, 500);
    assert_eq!(request.hop, 13);
    assert_eq!(request.sca, 1);
    let event = request.first_event(PacketTime {
        coarse: 0x0005_308b,
        fine_countdown: 0x1ee,
    });
    assert_eq!(event.receive_window, 1319);
    assert_eq!(
        event.slot_words(0x01e4),
        [0x2802, 0x3103, 5, 7, 0x79, 3589, 0, 0x0f00]
    );
    Ok(())
}

const UPDATE_IND: [u8; 12] = [0, 1, 4, 0, 6, 0, 0, 0, 0xf4, 1, 0x0b, 0];

#[test]
fn test_should_schedule_the_phone_update_from_the_old_anchor() -> Result<(), ConnectError> {
    let update = ConnectionUpdate::decode(&UPDATE_IND, 5)?;
    assert_eq!(update.instant, 11);
    assert_eq!(update.interval, 6);
    assert_eq!(update.latency, 0);
    assert_eq!(update.supervision_timeout, 500);

    let anchor = PacketTime {
        coarse: 0x0fff_fffd,
        fine_countdown: 600,
    };
    let event = update.event_at(anchor, 40, 6, 1);
    assert_eq!(event.receive_window, 894);
    assert_eq!(
        event.slot_words(0x01e4),
        [0x2802, 972, 0, 498, 0x79, 2739, 0, 0x0f00]
    );
    let next = event.one_interval_later(update.interval);
    assert_eq!(next.receive_window, 894);
    assert_eq!(
        next.slot_words(0x01e4),
        [0x2802, 996, 0, 498, 0x79, 2739, 0, 0x0f00]
    );
    Ok(())
}

#[test]
fn test_should_accept_future_instant_across_a_counter_wrap() -> Result<(), ConnectError> {
    let mut wrapped = UPDATE_IND;
    wrapped[10..].copy_from_slice(&2_u16.to_le_bytes());
    assert_eq!(ConnectionUpdate::decode(&wrapped, u16::MAX)?.instant, 2);
    assert_eq!(ConnectionUpdate::decode(&wrapped, 1)?.instant, 2);
    assert_eq!(ConnectionUpdate::decode(&wrapped, 0x8004)?.instant, 2);
    for counter in [2, 3, 0x8002, 0x8003] {
        assert_eq!(
            ConnectionUpdate::decode(&wrapped, counter),
            Err(ConnectError::Instant)
        );
    }
    Ok(())
}

#[test]
fn test_should_reject_malformed_connection_updates() {
    assert_eq!(
        ConnectionUpdate::decode(&UPDATE_IND[..11], 5),
        Err(ConnectError::Header)
    );
    for (index, value, error) in [
        (0, 1, ConnectError::Header),
        (1, 6, ConnectError::Window),
        (2, 7, ConnectError::Window),
        (4, 5, ConnectError::Interval),
        (7, 2, ConnectError::Supervision),
    ] {
        let mut update = UPDATE_IND;
        update[index] = value;
        assert_eq!(ConnectionUpdate::decode(&update, 5), Err(error));
    }
}
