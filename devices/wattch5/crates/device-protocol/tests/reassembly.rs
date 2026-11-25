//! Protocol boundary regressions.
use device_protocol::{Fragmenter, Frame, Header, Kind, Progress, ProtocolError, Reassembler};
fn header(n: u16) -> Header {
    Header {
        kind: Kind::Request,
        module: 4,
        opcode: 0x21,
        message_id: 3,
        offset: 0,
        total_length: n,
    }
}
fn fragment(offset: u16, total: u16, payload: &[u8]) -> Frame<'_> {
    let mut h = header(total);
    h.offset = offset;
    Frame { header: h, payload }
}
#[test]
fn test_should_reassemble_boundaries_and_require_reset() {
    for n in [0u16, 1, 10, 11, 128, 512] {
        let b = vec![0xa5; usize::from(n)];
        let mut tx = Fragmenter::new(header(n), &b, 20).unwrap_or_else(|e| panic!("{e:?}"));
        let mut rx = Reassembler::<512>::new(20, 512).unwrap_or_else(|e| panic!("{e:?}"));
        let mut out = [0; 20];
        let mut now = 0;
        while !tx.is_complete() {
            let size = tx.write_next(&mut out).unwrap_or_else(|e| panic!("{e:?}"));
            let r = rx
                .push(
                    Frame::decode(&out[..size], 20, 512).unwrap_or_else(|e| panic!("{e:?}")),
                    now,
                )
                .unwrap_or_else(|e| panic!("{e:?}"));
            tx.advance().unwrap_or_else(|e| panic!("{e:?}"));
            assert_eq!(
                r,
                if tx.is_complete() {
                    Progress::Complete
                } else {
                    Progress::Pending
                }
            );
            now += 100;
        }
        assert_eq!(rx.message().unwrap_or_else(|e| panic!("{e:?}")).payload, b);
        assert_eq!(
            rx.message()
                .unwrap_or_else(|e| panic!("{e:?}"))
                .header
                .offset,
            0
        );
        assert_eq!(
            rx.push(fragment(0, 1, &[1]), now),
            Err(ProtocolError::InvalidState)
        );
        rx.reset();
        assert_eq!(rx.push(fragment(0, 1, &[1]), now), Ok(Progress::Complete));
    }
}
#[test]
fn test_should_fault_on_identity_sequence_and_malformed_fragments() {
    for variant in 0..7 {
        let mut r = Reassembler::<32>::new(20, 32).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(r.push(fragment(0, 20, &[0; 10]), 0), Ok(Progress::Pending));
        let mut f = fragment(10, 20, &[1; 10]);
        match variant {
            0 => f.header.message_id = 4,
            1 => f.header.module = 3,
            2 => f.header.opcode = 2,
            3 => f.header.total_length = 21,
            4 => f.header.offset = 0,
            5 => f.header.kind = Kind::Event,
            _ => f.payload = &[0; 9],
        }
        assert!(r.push(f, 10).is_err());
        assert_eq!(
            r.push(fragment(10, 20, &[0; 10]), 20),
            Err(ProtocolError::InvalidState)
        );
        assert_eq!(r.message(), Err(ProtocolError::InvalidState));
    }
    let mut r = Reassembler::<32>::new(20, 32).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(r.push(fragment(0, 33, &[0; 10]), 0).is_err());
}
#[test]
fn test_should_enforce_idle_monotonic_and_absolute_budgets() {
    for (elapsed, expected) in [
        (2000, Ok(Progress::Complete)),
        (2001, Err(ProtocolError::Timeout)),
    ] {
        let mut r = Reassembler::<32>::new(20, 32).unwrap_or_else(|e| panic!("{e:?}"));
        r.push(fragment(0, 20, &[0; 10]), 10_000)
            .unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            r.push(fragment(10, 20, &[0; 10]), 10_000 + elapsed),
            expected
        );
    }
    let mut r = Reassembler::<32>::new(20, 32).unwrap_or_else(|e| panic!("{e:?}"));
    r.push(fragment(0, 20, &[0; 10]), 99)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        r.push(fragment(10, 20, &[0; 10]), 98),
        Err(ProtocolError::InvalidState)
    );
    for (last, expected) in [
        (7100, Ok(Progress::Complete)),
        (7101, Err(ProtocolError::Timeout)),
    ] {
        let mut r = Reassembler::<512>::new(20, 512).unwrap_or_else(|e| panic!("{e:?}"));
        for i in 0u16..51 {
            r.push(fragment(i * 10, 512, &[0; 10]), u64::from(i) * 140)
                .unwrap_or_else(|e| panic!("{e:?}"));
        }
        assert_eq!(r.push(fragment(510, 512, &[0; 2]), last), expected);
    }
}
#[test]
fn test_should_redact_buffer_from_debug_in_all_states() {
    let mut r = Reassembler::<32>::new(20, 32).unwrap_or_else(|e| panic!("{e:?}"));
    r.push(fragment(0, 14, b"PRIVATE123"), 0)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(!format!("{r:?}").contains("80, 82, 73"));
    r.push(fragment(10, 14, b"4567"), 1)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(!format!("{r:?}").contains("80, 82, 73"));
    assert!(r.push(fragment(0, 1, &[1]), 2).is_err());
    assert!(!format!("{r:?}").contains("80, 82, 73"));
}
#[test]
fn test_should_expire_silent_partial_message_without_waiting_for_another_fragment() {
    let mut r = Reassembler::<32>::new(20, 32).unwrap_or_else(|e| panic!("{e:?}"));
    r.push(fragment(0, 20, &[0; 10]), 500)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(r.advance(2500), Ok(()));
    assert_eq!(r.advance(2501), Err(ProtocolError::Timeout));
    assert_eq!(
        r.push(fragment(10, 20, &[0; 10]), 2501),
        Err(ProtocolError::InvalidState)
    );
}
