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
    // half-microseconds after the initial coarse boundary.
    let (start, fine, window, duration) = if MAC_PROBE_TIMING {
        (10, 170, 2285, 5521)
    } else {
        (13, 61, 1293, 3537)
    };
    assert_eq!(event.start.coarse, u32::from(start));
    assert_eq!(event.start.fine_countdown, fine);
    assert_eq!(event.receive_window, window);
    assert_eq!(
        event.slot_words(0x01e4),
        [0x2802, start, 0, fine, 0x79, duration, 0, 0x0f00]
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
        (0x1e, window),
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
    // A 30 ms interval is 96 coarse ticks. At 520 ppm the clock widening
    // adds 32 half-microseconds to the fixed guard.
    let (start, fine, window) = if MAC_PROBE_TIMING {
        (92, 278, 1047)
    } else {
        (95, 169, 55)
    };
    assert_eq!(next.start.coarse, start);
    assert_eq!(next.start.fine_countdown, fine);
    assert_eq!(next.receive_window, window);
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
    let (start, fine, window, duration) = if MAC_PROBE_TIMING {
        (0x3100, 116, 2311, 5573)
    } else {
        (0x3103, 7, 1319, 3589)
    };
    assert_eq!(event.receive_window, window);
    assert_eq!(
        event.slot_words(0x01e4),
        [0x2802, start, 5, fine, 0x79, duration, 0, 0x0f00]
    );
    Ok(())
}

const UPDATE_IND: [u8; 12] = [0, 1, 4, 0, 6, 0, 0, 0, 0xf4, 1, 0x0b, 0];
const MAC_PROBE_TIMING: bool = cfg!(any(
    all(
        feature = "ble-gatt-mac-probe",
        not(feature = "ble-gatt-xip")
    ),
    all(
        feature = "mic1-acquisition",
        feature = "ble-gatt-freertos",
        feature = "ble-gatt-xip",
        not(feature = "ble-gatt-dle")
    )
));

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
    let (start, fine, window, duration) = if MAC_PROBE_TIMING {
        (969, 607, 1886, 4723)
    } else {
        (972, 498, 894, 2739)
    };
    assert_eq!(event.receive_window, window);
    assert_eq!(
        event.slot_words(0x01e4),
        [0x2802, start, 0, fine, 0x79, duration, 0, 0x0f00]
    );
    let next = event.one_interval_later(update.interval);
    assert_eq!(next.receive_window, window);
    assert_eq!(
        next.slot_words(0x01e4),
        [0x2802, start + 24, 0, fine, 0x79, duration, 0, 0x0f00]
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

#[test]
fn asymmetric_phy_update_uses_peripheral_directions_at_the_wrapping_instant() {
    use fr8029d_bringup::phy::LinkPhy;
    let mut phy = LinkPhy::default();
    assert!(phy.schedule(0xffff, 1, 1, 2));
    assert_eq!(phy.apply(0), None);
    assert_eq!(phy.apply(1), Some(1)); // peripheral TX=2M, RX=1M
    assert!(phy.schedule(2, 3, 2, 0));
    assert_eq!(phy.apply(3), Some(5));
    assert!(!phy.schedule(4, 3, 1, 1));
}

#[test]
fn isolated_losses_do_not_downgrade_and_sustained_losses_request_one_m() {
    use fr8029d_bringup::phy::LinkPhy;
    let mut phy = LinkPhy::default();
    phy.peer_features(true);
    assert!(phy.schedule(0, 6, 2, 2));
    assert_eq!(phy.apply(6), Some(5));
    for _ in 0..100 {
        phy.observe_event(true);
        phy.observe_event(false);
        assert_eq!(phy.take_request(), None);
    }
    for _ in 0..8 {
        phy.observe_event(true);
    }
    assert_eq!(phy.take_request(), Some(1));
    assert_eq!(phy.take_request(), None);
}

#[test]
fn one_m_peers_stay_at_one_m_and_no_change_holds_off_retries() {
    use fr8029d_bringup::phy::LinkPhy;
    let mut phy = LinkPhy::default();
    for _ in 0..200 {
        phy.observe_event(false);
    }
    assert_eq!(phy.take_request(), None);
    phy.peer_features(true);
    assert_eq!(phy.take_request(), Some(2));
    assert!(phy.schedule(10, 0, 0, 0));
    for _ in 0..127 {
        phy.observe_event(false);
    }
    assert_eq!(phy.take_request(), None);
    phy.observe_event(false);
    assert_eq!(phy.take_request(), Some(2));
}

#[test]
fn downgrade_and_recovery_wait_for_the_centrals_instant() {
    use fr8029d_bringup::phy::LinkPhy;
    let mut phy = LinkPhy::default();
    phy.peer_features(true);
    assert!(phy.schedule(1, 7, 2, 2));
    assert_eq!(phy.apply(7), Some(5));
    for _ in 0..8 {
        phy.observe_event(true);
    }
    assert_eq!(phy.take_request(), Some(1));
    assert!(phy.schedule(16, 22, 1, 1));
    assert!(phy.rx_two_m());
    assert_eq!(phy.apply(22), Some(0));
    assert!(!phy.rx_two_m());
    for _ in 0..127 {
        phy.observe_event(false);
    }
    assert_eq!(phy.take_request(), None);
    phy.observe_event(false);
    assert_eq!(phy.take_request(), Some(2));
    assert!(phy.schedule(151, 157, 2, 2));
    assert_eq!(phy.apply(157), Some(5));
}

#[test]
fn quiet_central_is_queried_once_and_unsupported_directions_are_preserved() {
    use fr8029d_bringup::phy::LinkPhy;
    let mut phy = LinkPhy::default();
    for _ in 0..16 {
        phy.observe_event(false);
    }
    assert!(phy.take_feature_query());
    assert!(!phy.take_feature_query());
    phy.peer_features(true);
    assert_eq!(phy.take_request(), Some(2));
    assert!(phy.schedule(18, 24, 4, 2));
    assert_eq!(phy.apply(24), Some(1));
    assert!(!phy.rx_two_m());
}

#[cfg(feature = "ble-gatt-dle")]
#[test]
fn extended_event_allots_airtime_for_both_directions_without_moving_the_anchor()
-> Result<(), ConnectError> {
    use fr8029d_bringup::connection::DataAirtime;
    let request = ConnectionRequest::decode(0x2285, &REQUEST, ADV_ADDRESS)?;
    let event = request.event_after(
        PacketTime {
            coarse: 5,
            fine_countdown: 40,
        },
        1,
    );
    let budget = |bytes, time, two_m| DataAirtime::new(bytes, time, two_m);
    let legacy = budget(27, 328, false);
    let full = budget(251, 2120, false);
    let fast = budget(251, 2120, true);
    let asymmetric = budget(251, 328, true);
    assert_eq!(
        (
            legacy.octets(),
            full.octets(),
            fast.octets(),
            asymmetric.octets()
        ),
        (27, 251, 251, 71)
    );
    let duration = |rx, tx| event.with_data_airtime(rx, tx).slot_words(0x1e4)[5];
    assert_eq!(duration(legacy, legacy), event.slot_words(0x1e4)[5]);
    assert_eq!(
        duration(full, full) - duration(legacy, legacy),
        2 * (2120 - 328)
    );
    assert_eq!(
        duration(full, fast) - duration(legacy, legacy),
        2120 + 1064 - 2 * 328
    );
    assert_eq!(event.with_data_airtime(full, fast).start, event.start);
    assert_eq!(
        event.with_data_airtime(full, fast).receive_window,
        event.receive_window
    );
    Ok(())
}
