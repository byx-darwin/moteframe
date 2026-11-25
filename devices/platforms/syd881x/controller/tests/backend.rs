//! Byte-register replay through the actual SYD adapter and radio algorithms.
#![allow(clippy::unwrap_used, reason = "Unexpected fixture setup failure")]
use ble_controller::*;
use ble_controller_syd881x::SydBackend;
use std::{cell::RefCell, collections::VecDeque, rc::Rc};
use syd881x_radio::{Bank, ByteRegisters, ConfigurationTable, Radio, Register};
#[derive(Debug)]
struct Chip {
    ll: [u8; 256],
    rx: VecDeque<VecDeque<u8>>,
    tx: Vec<u8>,
    writes: Vec<(Bank, u8, u8)>,
    rf_address: u8,
    fault: bool,
    setup_after_status: bool,
    interrupt_after_reset: u8,
}
impl Default for Chip {
    fn default() -> Self {
        Self {
            ll: {
                let mut v = [0; 256];
                v[3] = 2;
                v
            },
            rx: VecDeque::new(),
            tx: Vec::new(),
            writes: Vec::new(),
            rf_address: 0,
            fault: false,
            setup_after_status: false,
            interrupt_after_reset: 0,
        }
    }
}
#[derive(Clone, Debug, Default)]
struct Io(Rc<RefCell<Chip>>);
impl ByteRegisters for Io {
    type Error = ();
    fn read8(&mut self, r: Register) -> Result<u8, ()> {
        let mut c = self.0.borrow_mut();
        if c.fault {
            return Err(());
        }
        if r.bank() == Bank::Misc {
            return Ok(match r.offset() {
                0x1a => 1,
                0x1c => match c.rf_address {
                    0x3b => 2,
                    0x0a => 4,
                    _ => 0,
                },
                _ => 0,
            });
        }
        if r.bank() != Bank::Ll {
            return Ok(0);
        }
        if r.offset() == 3 && c.setup_after_status {
            c.ll[1] |= 8;
            c.setup_after_status = false;
        }
        Ok(match r.offset() {
            3 => (c.ll[3] & 3) | if c.rx.is_empty() { 8 } else { 0 },
            0xc4 => c.rx.front_mut().and_then(VecDeque::pop_front).ok_or(())?,
            o => c.ll[usize::from(o)],
        })
    }
    fn write8(&mut self, r: Register, v: u8) -> Result<(), ()> {
        let mut c = self.0.borrow_mut();
        if c.fault {
            return Err(());
        }
        c.writes.push((r.bank(), r.offset(), v));
        if r.bank() == Bank::Misc && r.offset() == 0x18 {
            c.rf_address = v;
        }
        if r.bank() == Bank::Ll {
            match r.offset() {
                0 => {
                    c.ll[0] = v & !3;
                    if v & 2 != 0 {
                        c.ll[1] |= c.interrupt_after_reset;
                        c.interrupt_after_reset = 0;
                    }
                }
                1 => c.ll[1] &= !v,
                0xc0 => c.tx.push(v),
                0xc9 => {
                    c.rx.pop_front();
                }
                o => c.ll[usize::from(o)] = v,
            }
        }
        Ok(())
    }
}
fn setup() -> (SydBackend<'static, Io>, Io) {
    let io = Io::default();
    let table = ConfigurationTable::new(&[0, 0, 0, 0, 0, 0]).unwrap();
    let mut b = SydBackend::new(Radio::new(io.clone(), 4).unwrap(), table, 8000, |_| {});
    b.reset().unwrap();
    (b, io)
}
fn ad() -> Advertisement {
    Advertisement::new(
        RandomAddress::new([0x43, 0x52, 0x53, 0x54, 0, 0xc0]).unwrap(),
        AdvertisingParameters::new(160, 160, 1).unwrap(),
        AdvertisingData::new(&[2, 1, 6]).unwrap(),
        AdvertisingData::new(&[]).unwrap(),
    )
}
fn connect(b: &mut SydBackend<'_, Io>, io: &Io) {
    b.advertise(&ad()).unwrap();
    let payload = [
        1, 2, 3, 4, 5, 6, 0x43, 0x52, 0x53, 0x54, 0, 0xc0, 0x91, 0x5d, 0x65, 0x50, 0x4a, 0x53,
        0x5a, 3, 1, 0, 24, 0, 0, 0, 72, 0, 0xff, 0xff, 0xff, 0xff, 0x1f, 0x2c,
    ];
    let mut c = io.0.borrow_mut();
    c.rx.push_back([0x85, 34].into_iter().chain(payload).collect());
    c.ll[1] = 8 | 4;
    drop(c);
    assert!(matches!(b.poll().unwrap(),Some(LinkEvent::Connected(_,p)) if p.interval()==24));
}
#[test]
fn test_should_configure_request_before_emitting_connection_and_not_restart_advertising() {
    let (mut b, io) = setup();
    connect(&mut b, &io);
    assert_eq!(io.0.borrow().ll[0x2a], 24);
    assert_eq!(io.0.borrow().ll[0x2e], 74);
    let writes = io.0.borrow().writes.len();
    assert!(b.poll().unwrap().is_none());
    assert!(
        !io.0.borrow().writes[writes..]
            .iter()
            .any(|&(_, o, _)| o == 0xc0)
    );
}
#[test]
fn test_should_separate_full_admission_stale_tx_status_and_owned_ack_token() {
    let (mut b, io) = setup();
    connect(&mut b, &io);
    let f = LinkFrame::new(FrameKind::Start, &[0, 0, 4, 0]).unwrap();
    io.0.borrow_mut().ll[3] = 1;
    assert_eq!(b.transmit(123, f).unwrap(), TxAdmission::Busy);
    io.0.borrow_mut().ll[3] = 0;
    io.0.borrow_mut().ll[1] = 1;
    assert_eq!(
        b.transmit(123, f.with_more_data(true)).unwrap(),
        TxAdmission::Committed
    );
    assert!(b.poll().unwrap().is_none()); // Old pre-commit TXINT cannot return this credit.
    io.0.borrow_mut().ll[1] = 1;
    io.0.borrow_mut().ll[3] = 2;
    assert_eq!(b.poll().unwrap(), Some(LinkEvent::Acknowledged(123)));
    assert!(b.poll().unwrap().is_none());
    assert_eq!(io.0.borrow().tx.last(), Some(&0));
}
#[test]
fn test_should_route_owned_data_fragments_and_connection_loss() {
    let (mut b, io) = setup();
    connect(&mut b, &io);
    io.0.borrow_mut()
        .rx
        .push_back(VecDeque::from(vec![2, 0x85, 1, 0, 4, 0, 0x0a]));
    assert!(
        matches!(b.poll().unwrap(),Some(LinkEvent::Received(f)) if f.payload()==[1,0,4,0,0x0a])
    );
    io.0.borrow_mut().ll[1] = 16;
    assert_eq!(b.poll().unwrap(), Some(LinkEvent::Disconnected(8)));
    assert!(
        b.transmit(1, LinkFrame::new(FrameKind::Continuation, &[]).unwrap())
            .is_err()
    );
}
#[test]
fn test_should_report_access_failure_instead_of_busy_or_success() {
    let (mut b, io) = setup();
    connect(&mut b, &io);
    io.0.borrow_mut().fault = true;
    assert!(b.poll().is_err());
    assert!(b.reset().is_err());
}

#[test]
fn test_should_emit_parameter_update_only_after_observed_instant_and_arm_owned_map() {
    let (mut b, io) = setup();
    connect(&mut b, &io);
    io.0.borrow_mut().ll[0x20] = 100;
    io.0.borrow_mut().ll[1] = 32;
    assert!(b.poll().unwrap().is_none());
    b.connection_update(&[0, 3, 16, 0, 24, 0, 0, 0, 72, 0, 106, 0])
        .unwrap();
    assert!(b.poll().unwrap().is_none());
    assert!(
        b.connection_update(&[0, 3, 16, 0, 24, 0, 0, 0, 72, 0, 106, 0])
            .is_err()
    );
    for counter in 101..=105 {
        io.0.borrow_mut().ll[0x20] = counter;
        io.0.borrow_mut().ll[1] = 32;
        assert!(b.poll().unwrap().is_none());
    }
    assert_ne!(io.0.borrow().ll[0x36], 0);
    io.0.borrow_mut().ll[0x20] = 106;
    io.0.borrow_mut().ll[0x22] &= !1;
    assert!(
        matches!(b.poll().unwrap(),Some(LinkEvent::ParametersUpdated(p)) if p.interval()==24&&p.timeout()==72)
    );
    let live = io.0.borrow().ll[0x98];
    b.channel_map(&[1, 3, 0, 0, 0, 0, 112, 0]).unwrap();
    assert!(b.poll().unwrap().is_none());
    assert_eq!(io.0.borrow().ll[0xca], 3);
    assert_eq!(io.0.borrow().ll[0x98], live);
    assert_eq!(io.0.borrow().ll[0x22] & 2, 2);
}

#[test]
fn test_should_preserve_first_data_until_late_connset_then_emit_connection_first() {
    for during_snapshot in [false, true] {
        let (mut b, io) = setup();
        b.advertise(&ad()).unwrap();
        let payload = [
            1, 2, 3, 4, 5, 6, 0x43, 0x52, 0x53, 0x54, 0, 0xc0, 0x91, 0x5d, 0x65, 0x50, 0x4a, 0x53,
            0x5a, 3, 1, 0, 24, 0, 0, 0, 72, 0, 0xff, 0xff, 0xff, 0xff, 0x1f, 0x2c,
        ];
        io.0.borrow_mut()
            .rx
            .push_back([0x85, 34].into_iter().chain(payload).collect());
        assert!(b.poll().unwrap().is_none());
        io.0.borrow_mut()
            .rx
            .push_back(VecDeque::from(vec![2, 0x85, 1, 0, 4, 0, 0x0a]));
        io.0.borrow_mut().setup_after_status = during_snapshot;
        assert!(b.poll().unwrap().is_none()); // Own the FIFO data until status is visible.
        io.0.borrow_mut().ll[1] |= 8;
        assert!(matches!(b.poll().unwrap(), Some(LinkEvent::Connected(..))));
        assert!(
            matches!(b.poll().unwrap(),Some(LinkEvent::Received(f)) if f.payload()==[1,0,4,0,0x0a])
        );
        assert!(b.poll().unwrap().is_none());
    }
}

#[test]
fn test_should_prepare_update_on_sleep_boundary_like_pinned_rom_when_end_is_delayed() {
    let (mut b, io) = setup();
    connect(&mut b, &io);
    io.0.borrow_mut().ll[0x20] = 100;
    io.0.borrow_mut().ll[1] = 32;
    assert!(b.poll().unwrap().is_none());
    b.connection_update(&[0, 3, 16, 0, 24, 0, 0, 0, 72, 0, 106, 0])
        .unwrap();
    assert!(b.poll().unwrap().is_none());
    for counter in 101..=105 {
        io.0.borrow_mut().ll[0x20] = counter;
        io.0.borrow_mut().ll[1] = 64;
        assert!(b.poll().unwrap().is_none());
    }
    assert_eq!(io.0.borrow().ll[0x36], 14);
    io.0.borrow_mut().ll[0x20] = 106;
    io.0.borrow_mut().ll[0x22] &= !1;
    assert!(matches!(b.poll().unwrap(), Some(LinkEvent::ParametersUpdated(p)) if p.interval()==24));
}

#[test]
fn test_should_resume_advertising_after_lost_attempt_without_inventing_connection() {
    let (mut b, io) = setup();
    b.advertise(&ad()).unwrap();
    let bytes = io.0.borrow().tx.len();
    io.0.borrow_mut().ll[1] = 16;
    assert_eq!(b.poll(), Ok(None));
    assert!(io.0.borrow().tx.len() > bytes);
    assert_eq!(io.0.borrow().ll[0x10] & 1, 1);
    assert_eq!(&io.0.borrow().ll[0x88..0x8c], &[0xd6, 0xbe, 0x89, 0x8e]);
    assert_eq!(b.poll(), Ok(None));
}
#[test]
fn test_should_clear_partial_setup_and_deferred_data_before_new_attempt() {
    let (mut b, io) = setup();
    b.advertise(&ad()).unwrap();
    let request = [
        1, 2, 3, 4, 5, 6, 0x43, 0x52, 0x53, 0x54, 0, 0xc0, 0x91, 0x5d, 0x65, 0x50, 0x4a, 0x53,
        0x5a, 3, 1, 0, 24, 0, 0, 0, 72, 0, 0xff, 0xff, 0xff, 0xff, 0x1f, 0x2c,
    ];
    io.0.borrow_mut()
        .rx
        .push_back([0x85, 34].into_iter().chain(request).collect());
    assert_eq!(b.poll(), Ok(None));
    io.0.borrow_mut()
        .rx
        .push_back(VecDeque::from(vec![2, 0x85, 1, 0, 4, 0, 0x0a]));
    assert_eq!(b.poll(), Ok(None));
    io.0.borrow_mut().ll[1] = 8 | 16;
    assert_eq!(b.poll(), Ok(None));
    io.0.borrow_mut()
        .rx
        .push_back([0x85, 34].into_iter().chain(request).collect());
    assert_eq!(b.poll(), Ok(None)); // The old setup bit must not establish this attempt.
    io.0.borrow_mut().ll[1] = 8;
    assert!(matches!(b.poll().unwrap(), Some(LinkEvent::Connected(..))));
    assert_eq!(b.poll(), Ok(None)); // No data from the abandoned attempt leaks through.
}
#[test]
fn test_should_ignore_stale_loss_while_idle_without_starting_unsolicited_advertising() {
    let (mut b, io) = setup();
    io.0.borrow_mut().ll[1] = 16;
    assert_eq!(b.poll(), Ok(None));
    assert!(io.0.borrow().tx.is_empty());
}
#[test]
fn test_should_keep_recovery_hardware_failure_visible_instead_of_claiming_advertising() {
    let (mut b, io) = setup();
    b.advertise(&ad()).unwrap();
    let bytes = io.0.borrow().tx.len();
    io.0.borrow_mut().ll[1] = 16;
    io.0.borrow_mut().ll[3] = 1;
    assert_eq!(
        b.poll(),
        Err(ble_controller_syd881x::SydError::Radio(
            syd881x_radio::RadioError::FifoFull
        ))
    );
    assert_eq!(io.0.borrow().tx.len(), bytes);
}

#[test]
fn test_should_drain_unread_old_packets_and_clear_late_status_before_restarting() {
    for count in [1, 32] {
        let (mut b, io) = setup();
        b.advertise(&ad()).unwrap();
        for _ in 0..count {
            io.0.borrow_mut()
                .rx
                .push_back(VecDeque::from(vec![2, 0x80]));
        }
        io.0.borrow_mut().interrupt_after_reset = 8 | 32;
        io.0.borrow_mut().ll[1] = 16;
        assert_eq!(b.poll(), Ok(None));
        assert!(io.0.borrow().rx.is_empty());
        assert_eq!(io.0.borrow().ll[1], 0);
        assert_eq!(
            io.0.borrow()
                .writes
                .iter()
                .filter(|&&(_, o, _)| o == 0xc9)
                .count(),
            count
        );
        assert_eq!(b.poll(), Ok(None));
    }
}
#[test]
fn test_should_bound_stale_receive_drain_and_never_restart_with_leftover_packets() {
    let (mut b, io) = setup();
    b.advertise(&ad()).unwrap();
    let bytes = io.0.borrow().tx.len();
    for _ in 0..33 {
        io.0.borrow_mut()
            .rx
            .push_back(VecDeque::from(vec![2, 0x80]));
    }
    io.0.borrow_mut().ll[1] = 16;
    assert_eq!(b.poll(), Err(ble_controller_syd881x::SydError::Capacity));
    assert_eq!(io.0.borrow().rx.len(), 1);
    assert_eq!(io.0.borrow().tx.len(), bytes);
    assert_eq!(io.0.borrow().ll[0x10] & 1, 0);
}
#[test]
fn test_should_refuse_recovery_on_nonempty_transmit_or_malformed_stale_receive() {
    for malformed in [false, true] {
        let (mut b, io) = setup();
        b.advertise(&ad()).unwrap();
        let bytes = io.0.borrow().tx.len();
        if malformed {
            io.0.borrow_mut().rx.push_back(VecDeque::from(vec![0, 39]));
        } else {
            io.0.borrow_mut().ll[3] = 0;
        }
        io.0.borrow_mut().ll[1] = 16;
        assert!(matches!(
            b.poll(),
            Err(ble_controller_syd881x::SydError::Radio(_))
        ));
        assert_eq!(io.0.borrow().tx.len(), bytes);
        assert_eq!(io.0.borrow().ll[0x10] & 1, 0);
    }
}
