//! Validate external `CONNECT_IND` fields before any connection register write.
#![allow(
    clippy::unwrap_used,
    reason = "Unexpected fixture failure aborts the test"
)]
use std::collections::VecDeque;
use syd881x_radio::{
    Bank, ByteRegisters, ConnectionRequest, ConnectionWindows, Radio, RadioError, ReceivedPacket,
    Register, StaticRandomAddress,
};

#[derive(Debug, Default)]
struct Io {
    bytes: VecDeque<u8>,
    writes: Vec<(u8, u8)>,
}
impl ByteRegisters for Io {
    type Error = ();
    fn read8(&mut self, r: Register) -> Result<u8, ()> {
        assert_eq!(r.bank(), Bank::Ll);
        match r.offset() {
            3 => Ok(0),
            0xc4 => self.bytes.pop_front().ok_or(()),
            _ => Ok(0xa0),
        }
    }
    fn write8(&mut self, r: Register, v: u8) -> Result<(), ()> {
        assert_eq!(r.bank(), Bank::Ll);
        self.writes.push((r.offset(), v));
        Ok(())
    }
}

#[test]
fn test_should_reproduce_pinned_board_window_math_without_floating_point() {
    let request = ConnectionRequest::parse(&packet(0x85, &payload()), identity()).unwrap();
    let windows = ConnectionWindows::for_board_clock(&request, 8000).unwrap();
    assert_eq!((windows.widening_ticks(), windows.setup_ticks()), (9, 2));
    let mut radio = Radio::new(Io::default(), 2).unwrap();
    radio.configure_connection_windows(windows).unwrap();
    assert_eq!(
        radio.into_inner().writes,
        vec![
            (0x79, 0xa0),
            (0x78, 0xa2),
            (0x3a, 9),
            (0x3b, 0),
            (0x36, 2),
            (0x37, 0)
        ]
    );
}

#[test]
fn test_should_reject_window_counter_and_setup_values_that_rom_would_truncate() {
    let mut p = payload();
    p[22] = 25;
    let request = ConnectionRequest::parse(&packet(0x85, &p), identity()).unwrap();
    assert!(ConnectionWindows::for_board_clock(&request, 8000).is_err());
    p[22..24].copy_from_slice(&3200_u16.to_le_bytes());
    p[20..22].copy_from_slice(&3200_u16.to_le_bytes());
    p[26..28].copy_from_slice(&3200_u16.to_le_bytes());
    let request = ConnectionRequest::parse(&packet(0x85, &p), identity()).unwrap();
    assert!(ConnectionWindows::for_board_clock(&request, 8000).is_err());
}
fn identity() -> StaticRandomAddress {
    StaticRandomAddress::new([0x43, 0x52, 0x53, 0x54, 0, 0xc0]).unwrap()
}
fn payload() -> [u8; 34] {
    [
        1, 2, 3, 4, 5, 6, 0x43, 0x52, 0x53, 0x54, 0, 0xc0, 0x91, 0x5d, 0x65, 0x50, 0x4a, 0x53,
        0x5a, 3, 1, 0, 24, 0, 0, 0, 72, 0, 0xff, 0xff, 0xff, 0xff, 0x1f, 0x2c,
    ]
}
fn packet(header: u8, payload: &[u8]) -> ReceivedPacket {
    let mut io = Io::default();
    io.bytes
        .extend([header, u8::try_from(payload.len()).unwrap()]);
    io.bytes.extend(payload);
    Radio::new(io, 2).unwrap().receive().unwrap().unwrap()
}

#[test]
fn test_should_decode_complete_legacy_request_and_apply_confirmed_rom_register_order() {
    let request = ConnectionRequest::parse(&packet(0x85, &payload()), identity()).unwrap();
    assert_eq!(request.interval_units(), 24);
    assert_eq!(request.timeout_units(), 72);
    assert_eq!(request.central_clock_accuracy_ppm(), 250);
    let mut radio = Radio::new(Io::default(), 2).unwrap();
    radio.configure_connection_parameters(&request).unwrap();
    assert_eq!(
        radio.into_inner().writes,
        vec![
            (0x25, 12),
            (0x98, 255),
            (0x99, 255),
            (0x9a, 255),
            (0x9b, 255),
            (0x9c, 31),
            (0x94, 0x4a),
            (0x95, 0x53),
            (0x96, 0x5a),
            (0x88, 0x91),
            (0x89, 0x5d),
            (0x8a, 0x65),
            (0x8b, 0x50),
            (0x26, 2),
            (0x27, 0),
            (0x2a, 24),
            (0x2b, 0),
            (0x2c, 0),
            (0x2d, 0),
            (0x2e, 74),
            (0x2f, 0),
            (0x28, 3),
        ]
    );
}

#[test]
fn test_should_reject_invalid_shape_address_channel_map_hop_access_address_and_timing() {
    let mut variants = Vec::new();
    let original = payload();
    for (offset, value) in [
        (6, 0),
        (19, 0),
        (19, 9),
        (20, 25),
        (22, 5),
        (23, 0xff),
        (26, 9),
        (26, 0),
        (27, 0),
        (32, 0xff),
        (33, 4),
        (33, 17),
    ] {
        let mut p = original;
        p[offset] = value;
        // Values already equal in the fixture cannot create invalid input.
        if p != original {
            variants.push(p);
        }
    }
    // All-zero / advertising / single-bit-distance / equal-byte AA are invalid.
    for aa in [0_u32, 0x8e89_bed6, 0x8e89_bed7, 0x5555_5555, 0xffff_ffff] {
        let mut p = original;
        p[12..16].copy_from_slice(&aa.to_le_bytes());
        variants.push(p);
    }
    // One used channel and insufficient supervision coverage.
    let mut p = original;
    p[28..33].copy_from_slice(&[1, 0, 0, 0, 0]);
    variants.push(p);
    let mut p = original;
    p[26..28].copy_from_slice(&10_u16.to_le_bytes());
    p[24] = 4;
    variants.push(p);
    for p in variants {
        assert!(
            ConnectionRequest::parse(&packet(0x85, &p), identity()).is_err(),
            "{p:?}"
        );
    }
    assert!(ConnectionRequest::parse(&packet(0x85, &original[..33]), identity()).is_err());
    assert!(ConnectionRequest::parse(&packet(0x05, &original), identity()).is_err());
    assert!(ConnectionRequest::parse(&packet(0xa5, &original), identity()).is_err());
}

#[test]
fn test_should_reject_nonzero_latency_before_mmio_in_polling_experiment() {
    let mut p = payload();
    p[24] = 1;
    let request = ConnectionRequest::parse(&packet(0x85, &p), identity()).unwrap();
    let mut radio = Radio::new(Io::default(), 2).unwrap();
    assert_eq!(
        radio.configure_connection_parameters(&request),
        Err(RadioError::InvalidParameters)
    );
    assert!(radio.into_inner().writes.is_empty());
}

#[test]
fn test_should_schedule_channel_map_update_at_its_future_instant_without_rewriting_live_map() {
    let mut radio = Radio::new(Io::default(), 2).unwrap();
    radio
        .apply_channel_map_indication(&[1, 255, 255, 0, 0, 0, 0xac, 0xa0])
        .unwrap();
    assert_eq!(
        radio.into_inner().writes,
        vec![
            (0xca, 255),
            (0xcb, 255),
            (0xcc, 0),
            (0xcd, 0),
            (0xce, 0),
            (0x38, 0xac),
            (0x39, 0xa0),
            (0x22, 0xa2)
        ]
    );
    for payload in [
        &[1, 0, 0, 0, 0, 0, 0xac, 0xa0][..],
        &[1, 255, 255, 0, 0, 0, 0x9f, 0xa0][..],
        &[1, 255, 255, 0, 0, 0, 0xa0, 0xa0][..],
        &[1, 255][..],
    ] {
        let mut radio = Radio::new(Io::default(), 2).unwrap();
        assert!(radio.apply_channel_map_indication(payload).is_err());
        assert!(radio.into_inner().writes.is_empty());
    }
}

#[test]
fn test_should_schedule_an_instant_across_event_counter_wraparound() {
    #[derive(Debug, Default)]
    struct Counter(Io);
    impl ByteRegisters for Counter {
        type Error = ();
        fn read8(&mut self, r: Register) -> Result<u8, ()> {
            match r.offset() {
                0x20 => Ok(0xfc),
                0x21 => Ok(0xff),
                _ => self.0.read8(r),
            }
        }
        fn write8(&mut self, r: Register, v: u8) -> Result<(), ()> {
            self.0.write8(r, v)
        }
    }
    let mut radio = Radio::new(Counter::default(), 2).unwrap();
    radio
        .apply_channel_map_indication(&[1, 255, 255, 0, 0, 0, 4, 0])
        .unwrap();
    assert!(radio.into_inner().0.writes.contains(&(0x38, 4)));
}
