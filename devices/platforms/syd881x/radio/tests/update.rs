//! Update staging is separate from its future-event setup and completion.
#![allow(
    clippy::unwrap_used,
    reason = "Unexpected fixture failure aborts the test"
)]
use syd881x_radio::{
    Bank, ByteRegisters, ConnectionUpdate, ConnectionWindows, Radio, RadioError, Register,
    UpdateProgress,
};
#[derive(Debug)]
struct Io {
    mem: [u8; 256],
    writes: Vec<(u8, u8)>,
}
impl Io {
    fn new(counter: u16) -> Self {
        let mut s = Self {
            mem: [0; 256],
            writes: vec![],
        };
        s.mem[0x20..0x22].copy_from_slice(&counter.to_le_bytes());
        s.mem[0x22] = 0xa2;
        s.mem[0x38] = counter.wrapping_add(12).to_le_bytes()[0];
        s.mem[0x39] = counter.wrapping_add(12).to_le_bytes()[1];
        s.mem[0x78] = 0xa0;
        s.mem[0x79] = 0xa0;
        s
    }
}
impl ByteRegisters for Io {
    type Error = ();
    fn read8(&mut self, r: Register) -> Result<u8, ()> {
        assert_eq!(r.bank(), Bank::Ll);
        Ok(self.mem[usize::from(r.offset())])
    }
    fn write8(&mut self, r: Register, v: u8) -> Result<(), ()> {
        self.mem[usize::from(r.offset())] = v;
        self.writes.push((r.offset(), v));
        Ok(())
    }
}
fn update(instant: u16) -> ConnectionUpdate {
    let [lo, hi] = instant.to_le_bytes();
    ConnectionUpdate::parse(&[0, 3, 16, 0, 24, 0, 0, 0, 72, 0, lo, hi]).unwrap()
}
#[test]
fn test_should_stage_update_and_apply_setup_at_previous_event_then_complete_on_clear() {
    let u = update(106);
    let windows = ConnectionWindows::for_update(u, 24, 250, 8000).unwrap();
    assert_eq!((windows.widening_ticks(), windows.setup_ticks()), (9, 14));
    let mut radio = Radio::new(Io::new(100), 1).unwrap();
    let mut pending = radio.schedule_connection_update(u, 24, 250, 8000).unwrap();
    let mut io = radio.into_inner();
    assert!(io.writes.contains(&(0x26, 15)));
    assert!(io.writes.contains(&(0x2a, 24)));
    assert!(io.writes.contains(&(0x2e, 74)));
    assert!(io.writes.contains(&(0x30, 106)));
    assert!(io.writes.contains(&(0x22, 0xa3)));
    assert!(!io.writes.iter().any(|(r, _)| *r == 0x36 || *r == 0x37));
    let mut radio = Radio::new(io, 1).unwrap();
    assert_eq!(
        radio
            .poll_connection_update(&mut pending, Some(100))
            .unwrap(),
        UpdateProgress::Pending
    );
    io = radio.into_inner();
    io.mem[0x20] = 105;
    let mut radio = Radio::new(io, 1).unwrap();
    assert_eq!(
        radio
            .poll_connection_update(&mut pending, Some(105))
            .unwrap(),
        UpdateProgress::SetupApplied
    );
    io = radio.into_inner();
    assert_eq!(&io.writes[io.writes.len() - 2..], &[(0x36, 14), (0x37, 0)]);
    io.mem[0x20] = 106;
    io.mem[0x22] &= !1;
    let mut radio = Radio::new(io, 1).unwrap();
    assert_eq!(
        radio.poll_connection_update(&mut pending, None).unwrap(),
        UpdateProgress::Complete
    );
    assert_eq!(pending.parameters(), [24, 0, 72]);
}
#[test]
fn test_should_validate_shapes_timing_pending_and_stale_instants_before_writes() {
    for p in [
        &[0][..],
        &[1, 3, 16, 0, 24, 0, 0, 0, 72, 0, 106, 0],
        &[0, 0, 16, 0, 24, 0, 0, 0, 72, 0, 106, 0],
        &[0, 3, 25, 0, 24, 0, 0, 0, 72, 0, 106, 0],
        &[0, 3, 16, 0, 24, 0, 1, 0, 72, 0, 106, 0],
    ] {
        assert!(ConnectionUpdate::parse(p).is_err());
    }
    for (counter, instant, busy) in [(100, 100, false), (100, 99, false), (100, 106, true)] {
        let u = update(instant);
        let mut io = Io::new(counter);
        if busy {
            io.mem[0x22] |= 1;
        }
        let mut r = Radio::new(io, 1).unwrap();
        assert!(r.schedule_connection_update(u, 24, 250, 8000).is_err());
        assert!(r.into_inner().writes.is_empty());
    }
}
#[test]
fn test_should_handle_zero_offset_changed_interval_wrap_and_reject_late_setup() {
    let u = ConnectionUpdate::parse(&[0, 2, 0, 0, 36, 0, 0, 0, 72, 0, 4, 0]).unwrap();
    let w = ConnectionWindows::for_update(u, 24, 250, 8000).unwrap();
    assert_eq!(w.setup_ticks(), 9);
    let mut r = Radio::new(Io::new(0xfffc), 1).unwrap();
    let mut p = r.schedule_connection_update(u, 24, 250, 8000).unwrap();
    let mut io = r.into_inner();
    assert!(io.writes.contains(&(0x26, 0)));
    assert!(io.writes.contains(&(0x2a, 36)));
    io.mem[0x20..0x22].copy_from_slice(&4_u16.to_le_bytes());
    let mut r = Radio::new(io, 1).unwrap();
    assert_eq!(
        r.poll_connection_update(&mut p, None),
        Err(RadioError::UpdateSetupTimeout {
            counter: 4,
            instant: 4
        })
    );
}

#[test]
fn test_should_not_apply_setup_from_an_end_token_for_a_different_event() {
    let u = update(106);
    let mut r = Radio::new(Io::new(100), 1).unwrap();
    let mut p = r.schedule_connection_update(u, 24, 250, 8000).unwrap();
    let mut io = r.into_inner();
    io.mem[0x20] = 105;
    let mut r = Radio::new(io, 1).unwrap();
    assert_eq!(
        r.poll_connection_update(&mut p, Some(104)).unwrap(),
        UpdateProgress::Pending
    );
    assert!(!r.into_inner().writes.iter().any(|(o, _)| *o == 0x36));
}

#[test]
fn test_should_refuse_stale_hardware_flag_and_late_commit_or_completion() {
    #[derive(Debug)]
    struct Moving {
        io: Io,
        clear_map: bool,
        advance: bool,
    }
    impl ByteRegisters for Moving {
        type Error = ();
        fn read8(&mut self, r: Register) -> Result<u8, ()> {
            self.io.read8(r)
        }
        fn write8(&mut self, r: Register, v: u8) -> Result<(), ()> {
            self.io.write8(r, v)?;
            if r.offset() == 0x28 {
                if self.clear_map {
                    self.io.mem[0x22] &= !2;
                }
                if self.advance {
                    self.io.mem[0x20] = 106;
                }
            }
            Ok(())
        }
    }
    let u = update(106);
    let mut r = Radio::new(
        Moving {
            io: Io::new(100),
            clear_map: true,
            advance: false,
        },
        1,
    )
    .unwrap();
    r.schedule_connection_update(u, 24, 250, 8000).unwrap();
    assert_eq!(r.into_inner().io.mem[0x22], 0xa1);
    let mut r = Radio::new(
        Moving {
            io: Io::new(100),
            clear_map: false,
            advance: true,
        },
        1,
    )
    .unwrap();
    assert!(r.schedule_connection_update(u, 24, 250, 8000).is_err());
    assert!(!r.into_inner().io.writes.iter().any(|(o, _)| *o == 0x22));
    let mut r = Radio::new(Io::new(100), 1).unwrap();
    let mut p = r.schedule_connection_update(u, 24, 250, 8000).unwrap();
    let mut io = r.into_inner();
    io.mem[0x20] = 105;
    let mut r = Radio::new(io, 1).unwrap();
    r.poll_connection_update(&mut p, Some(105)).unwrap();
    let mut io = r.into_inner();
    io.mem[0x20] = 206;
    io.mem[0x22] &= !1;
    let mut r = Radio::new(io, 1).unwrap();
    assert_eq!(
        r.poll_connection_update(&mut p, None),
        Err(RadioError::UpdateCompletionTimeout)
    );
}

#[test]
fn test_should_withhold_end_tokens_on_service_gaps_or_ack_boundary_crossing() {
    #[derive(Debug)]
    struct Advance(Io);
    impl ByteRegisters for Advance {
        type Error = ();
        fn read8(&mut self, r: Register) -> Result<u8, ()> {
            self.0.read8(r)
        }
        fn write8(&mut self, r: Register, v: u8) -> Result<(), ()> {
            self.0.write8(r, v)?;
            if r.offset() == 1 {
                self.0.mem[0x20] = 101;
            }
            Ok(())
        }
    }
    let mut io = Io::new(100);
    io.mem[1] = 32;
    let mut r = Radio::new(io, 1).unwrap();
    let e = r.take_connection_events(Some(99)).unwrap();
    assert_eq!((e.counter(), e.ended_counter()), (100, Some(100)));
    let mut io = Io::new(100);
    io.mem[1] = 32;
    let mut r = Radio::new(io, 1).unwrap();
    let e = r.take_connection_events(Some(98)).unwrap();
    assert_eq!((e.bits(), e.counter(), e.ended_counter()), (32, 100, None));
    assert_eq!(e.timing_ambiguity(), 1);
    let mut io = Io::new(100);
    io.mem[1] = 32;
    let mut r = Radio::new(Advance(io), 1).unwrap();
    let e = r.take_connection_events(Some(100)).unwrap();
    assert_eq!((e.bits(), e.counter(), e.ended_counter()), (32, 101, None));
    assert_eq!(e.timing_ambiguity(), 2);
    let mut io = r.into_inner().0;
    assert_eq!(io.writes, [(1, 32)]);
    io.mem[0x20] = 102;
    io.mem[1] = 32; // Fresh END after the ambiguous acknowledgement.
    let mut r = Radio::new(io, 1).unwrap();
    let e = r.take_connection_events(Some(101)).unwrap();
    assert_eq!((e.ended_counter(), e.timing_ambiguity()), (Some(102), 0));
}

#[test]
fn test_should_recheck_setup_after_mmio_and_expire_a_deferred_request() {
    #[derive(Debug)]
    struct Advance(Io);
    impl ByteRegisters for Advance {
        type Error = ();
        fn read8(&mut self, r: Register) -> Result<u8, ()> {
            self.0.read8(r)
        }
        fn write8(&mut self, r: Register, v: u8) -> Result<(), ()> {
            self.0.write8(r, v)?;
            if r.offset() == 0x36 {
                self.0.mem[0x20] = 106;
            }
            Ok(())
        }
    }
    let u = update(106);
    let mut r = Radio::new(Io::new(100), 1).unwrap();
    let mut p = r.schedule_connection_update(u, 24, 250, 8000).unwrap();
    let mut io = r.into_inner();
    io.mem[0x20] = 105;
    let mut r = Radio::new(Advance(io), 1).unwrap();
    assert_eq!(
        r.poll_connection_update(&mut p, Some(105)),
        Err(RadioError::UpdateSetupCrossed)
    );
    let mut io = Io::new(106);
    io.mem[0x22] = 1;
    let mut r = Radio::new(io, 1).unwrap();
    assert!(matches!(
        r.schedule_connection_update(u, 24, 250, 8000),
        Err(RadioError::InvalidParameters)
    ));
}

#[test]
fn test_should_keep_update_deadline_when_an_ambiguous_end_token_is_withheld() {
    let mut r = Radio::new(Io::new(100), 1).unwrap();
    let mut pending = r
        .schedule_connection_update(update(106), 24, 250, 8000)
        .unwrap();
    let mut io = r.into_inner();
    io.mem[0x20] = 105;
    io.mem[1] = 32;
    let mut r = Radio::new(io, 1).unwrap();
    let e = r.take_connection_events(Some(103)).unwrap();
    assert_eq!(e.ended_counter(), None);
    assert_eq!(
        r.poll_connection_update(&mut pending, e.ended_counter())
            .unwrap(),
        UpdateProgress::Pending
    );
    let mut io = r.into_inner();
    assert!(!io.writes.iter().any(|(o, _)| *o == 0x36));
    io.mem[0x20] = 106;
    let mut r = Radio::new(io, 1).unwrap();
    assert_eq!(
        r.poll_connection_update(&mut pending, None),
        Err(RadioError::UpdateSetupTimeout {
            counter: 106,
            instant: 106
        })
    );
}

#[test]
fn test_should_remember_a_pre_ack_boundary_across_snapshot_retries() {
    #[derive(Debug)]
    struct Scripted {
        io: Io,
        low_reads: usize,
    }
    impl ByteRegisters for Scripted {
        type Error = ();
        fn read8(&mut self, r: Register) -> Result<u8, ()> {
            if r.offset() == 0x20 {
                let value = if self.low_reads == 0 { 104 } else { 105 };
                self.low_reads += 1;
                Ok(value)
            } else {
                self.io.read8(r)
            }
        }
        fn write8(&mut self, r: Register, v: u8) -> Result<(), ()> {
            self.io.write8(r, v)
        }
    }
    let mut io = Io::new(104);
    io.mem[1] = 32; // Sticky END104 survives while raw counter advances to105.
    let mut r = Radio::new(Scripted { io, low_reads: 0 }, 1).unwrap();
    let event = r.take_connection_events(Some(104)).unwrap();
    assert_eq!((event.counter(), event.ended_counter()), (105, None));
    assert_eq!(event.setup_counter(), None);
    assert_eq!(event.timing_ambiguity(), 4);
    assert_eq!(r.into_inner().io.writes, [(1, 32)]);
}

#[test]
fn test_should_preserve_setup_deadline_context_for_host_diagnostics() {
    let mut r = Radio::new(Io::new(100), 1).unwrap();
    let mut p = r
        .schedule_connection_update(update(106), 24, 250, 8000)
        .unwrap();
    let mut io = r.into_inner();
    io.mem[0x20] = 106;
    let mut r = Radio::new(io, 1).unwrap();
    assert_eq!(
        r.poll_connection_update(&mut p, None),
        Err(RadioError::UpdateSetupTimeout {
            counter: 106,
            instant: 106
        })
    );
}

#[test]
fn test_should_qualify_end_or_sleep_for_setup_but_never_ambiguous_or_rx_only_status() {
    for (bits, previous, qualified) in [
        (32, 104, true),
        (64, 104, true),
        (96, 104, true),
        (192, 104, true),
        (2, 104, false),
        (64, 103, false),
    ] {
        let mut io = Io::new(105);
        io.mem[1] = bits;
        let mut r = Radio::new(io, 1).unwrap();
        let e = r.take_connection_events(Some(previous)).unwrap();
        assert_eq!(e.setup_counter(), qualified.then_some(105));
        if bits & 32 == 0 {
            assert_eq!(e.ended_counter(), None);
        }
    }
}
