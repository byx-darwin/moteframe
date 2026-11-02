// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Line-state and failure regressions for the real pointer reader.
use moteframe_i2c::{BitBangI2c, CheckedReadError, I2cError, Line, NormalAddress7};
use std::{cell::RefCell, rc::Rc};
#[derive(Debug)]
// Independent wire levels and injected fault switches form the simulation state.
#[allow(clippy::struct_excessive_bools)]
struct State {
    scl: bool,
    sda: bool,
    slave_release: bool,
    starts: usize,
    stops: usize,
    releases: usize,
    reads: usize,
    pauses: usize,
    fault: Option<usize>,
    nack_byte: Option<usize>,
    stick_stop: Option<usize>,
    stop_attempts: usize,
    release_after: Option<u16>,
    held_samples: u16,
    post_stop_hold: bool,
    hold_clock_after_stop: bool,
    active: bool,
    read_mode: bool,
    cycle: usize,
    byte: u8,
    commands: Vec<u8>,
    read_index: usize,
    acks: Vec<bool>,
    data: [u8; 2],
    events: Vec<String>,
}
impl State {
    fn new() -> Self {
        Self {
            scl: true,
            sda: true,
            slave_release: true,
            starts: 0,
            stops: 0,
            releases: 0,
            reads: 0,
            pauses: 0,
            fault: None,
            nack_byte: None,
            stick_stop: None,
            stop_attempts: 0,
            release_after: None,
            held_samples: 0,
            post_stop_hold: false,
            hold_clock_after_stop: false,
            active: false,
            read_mode: false,
            cycle: 0,
            byte: 0,
            commands: vec![],
            read_index: 0,
            acks: vec![],
            data: [0xa5, 0x3c],
            events: vec![],
        }
    }
    fn actual_data(&self) -> bool {
        self.sda && self.slave_release
    }
    fn rising(&mut self) {
        if !self.active {
            return;
        }
        self.cycle += 1;
        if self.read_mode {
            if self.cycle == 9 {
                self.acks.push(self.sda);
            }
        } else if self.cycle <= 8 {
            self.byte = (self.byte << 1) | u8::from(self.actual_data());
        } else {
            self.commands.push(self.byte);
        }
    }
    fn falling(&mut self) {
        if !self.active {
            return;
        }
        if self.read_mode {
            if self.cycle == 9 {
                self.read_index += 1;
                self.cycle = 0;
            }
            if self.cycle == 8 {
                self.slave_release = true;
            } else {
                self.slave_release = self.data.get(self.read_index).copied().unwrap_or(0xff)
                    & (0x80 >> self.cycle)
                    != 0;
            }
        } else if self.cycle == 8 {
            self.slave_release = self.nack_byte == Some(self.commands.len());
        } else if self.cycle == 9 {
            let read = self.commands.len() == 3 && self.byte & 1 != 0;
            self.cycle = 0;
            self.byte = 0;
            self.read_mode = read;
            self.slave_release = if read { self.data[0] & 0x80 != 0 } else { true };
        }
    }
}
#[derive(Debug)]
struct Pin {
    clock: bool,
    shared: Rc<RefCell<State>>,
}
impl Line for Pin {
    fn low(&mut self) {
        let mut s = self.shared.borrow_mut();
        s.events.push(if self.clock { "C0" } else { "D0" }.into());
        if self.clock {
            if s.scl {
                s.scl = false;
                s.falling();
            }
        } else {
            let was = s.actual_data();
            s.sda = false;
            if s.scl && was {
                s.starts += 1;
                s.active = true;
                s.read_mode = false;
                s.cycle = 0;
                s.byte = 0;
            }
        }
    }
    fn release(&mut self) {
        let mut s = self.shared.borrow_mut();
        s.events.push(if self.clock { "CZ" } else { "DZ" }.into());
        if self.clock {
            s.releases += 1;
            let high = s.fault != Some(s.releases);
            if !s.scl && high {
                s.scl = true;
                s.rising();
            } else {
                s.scl = high;
            }
        } else {
            let was = s.actual_data();
            s.sda = true;
            // Model the actual wired-AND level, including a target holding either STOP low.
            if s.scl && !was {
                s.stop_attempts += 1;
                if s.stick_stop == Some(s.stop_attempts) {
                    s.slave_release = false;
                    s.post_stop_hold = true;
                }
            }
            if s.scl && !was && s.actual_data() {
                s.stops += 1;
                s.active = false;
                s.slave_release = true;
                if s.hold_clock_after_stop {
                    s.scl = false;
                }
            }
        }
    }
    fn high(&self) -> bool {
        let mut s = self.shared.borrow_mut();
        s.reads += 1;
        if self.clock {
            s.scl
        } else {
            if s.post_stop_hold {
                s.held_samples += 1;
                if s.release_after == Some(s.held_samples) {
                    s.slave_release = true;
                    s.post_stop_hold = false;
                }
            }
            s.actual_data()
        }
    }
}
fn fixture() -> (BitBangI2c<Pin, Pin, impl FnMut()>, Rc<RefCell<State>>) {
    let s = Rc::new(RefCell::new(State::new()));
    let delay = s.clone();
    let bus = BitBangI2c::new(
        Pin {
            clock: true,
            shared: s.clone(),
        },
        Pin {
            clock: false,
            shared: s.clone(),
        },
        move || delay.borrow_mut().pauses += 1,
        3,
    )
    .unwrap_or_else(|error| panic!("{error:?}"));
    {
        let mut s = s.borrow_mut();
        s.releases = 0;
        s.events.clear();
    }
    (bus, s)
}
#[test]
fn electrical_multi_byte_success() {
    let (mut b, s) = fixture();
    let mut out = [0; 2];
    assert_eq!(b.read(0x15, 1, &mut out), Ok(()));
    assert_eq!(out, [0xa5, 0x3c]);
    let s = s.borrow();
    assert_eq!(s.commands, [0x2a, 1, 0x2b]);
    assert_eq!(s.acks, [false, true]);
    assert_eq!((s.starts, s.stops, s.releases), (2, 2, 49));
    println!(
        "success commands={:?}, read={out:?}, ACK/NACK={:?}, starts={}, stops={}, clock_waits={}",
        s.commands, s.acks, s.starts, s.stops, s.releases
    );
}
#[test]
fn timeout_at_every_clock_wait_releases_and_returns_timeout() {
    for n in 1..=49 {
        let (mut b, s) = fixture();
        s.borrow_mut().fault = Some(n);
        let mut out = [0xee; 2];
        assert_eq!(
            b.read(0x15, 1, &mut out),
            Err(I2cError::ClockTimeout),
            "wait {n}"
        );
        let s = s.borrow();
        assert!((s.scl || n == 49) && s.sda, "wait {n}");
        assert!(s.reads < 150, "finite reads wait {n}");
        if n <= 39 {
            assert_eq!(out[0], 0xee, "first byte incomplete {n}");
        } else {
            assert_eq!(out[0], 0xa5);
        }
        if n <= 48 {
            assert_eq!(out[1], 0xee);
        } else {
            assert_eq!(out[1], 0x3c);
        }
    }
    println!(
        "49 one-shot timeout injection positions passed; including START, write bit/ACK, pointer STOP, read bit/ACK, final STOP"
    );
}
#[test]
fn nack_at_all_three_written_bytes_retains_first_error() {
    for n in 0..3 {
        let (mut b, s) = fixture();
        s.borrow_mut().nack_byte = Some(n);
        let mut out = [0xee; 2];
        assert_eq!(b.read(0x15, 1, &mut out), Err(I2cError::Nack));
        assert_eq!(out, [0xee; 2]);
        let s = s.borrow();
        assert_eq!(s.commands.len(), n + 1);
        assert!(s.scl && s.sda);
    }
}
#[test]
fn busy_has_cleanup_io() {
    let (mut b, s) = fixture();
    s.borrow_mut().slave_release = false;
    assert_eq!(b.read(0x15, 1, &mut [0]), Err(I2cError::Busy));
    let s = s.borrow();
    assert!(s.events.iter().any(|x| x == "D0"));
    println!("Busy events={:?}, stops={}", s.events, s.stops);
}
#[test]
fn stuck_sda_final_stop_still_returns_success() {
    let (mut b, s) = fixture();
    s.borrow_mut().stick_stop = Some(2);
    let mut out = [0; 2];
    assert_eq!(b.read(0x15, 1, &mut out), Ok(()));
    let s = s.borrow();
    assert!(!s.actual_data());
    assert!(s.sda);
    assert_eq!(s.stops, 1);
    println!(
        "reported Ok with actual SDA low, only {} STOP observed, master SDA released={}",
        s.stops, s.sda
    );
}
#[test]
fn transient_timeout_does_not_latch_poison() {
    let (mut b, s) = fixture();
    s.borrow_mut().fault = Some(1);
    assert_eq!(b.read(0x15, 1, &mut [0]), Err(I2cError::ClockTimeout));
    *s.borrow_mut() = State::new();
    assert_eq!(b.read(0x15, 1, &mut [0; 2]), Ok(()));
    println!("same object resumes immediately after transient timeout; no Faulted guard");
}
#[test]
fn invalid_has_no_line_calls_and_zero_budget_drops_owned_resources() {
    struct Owned(Rc<std::cell::Cell<usize>>);
    impl Drop for Owned {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }
    impl Line for Owned {
        fn low(&mut self) {
            panic!("invalid constructor IO")
        }
        fn release(&mut self) {
            panic!("invalid constructor IO")
        }
        fn high(&self) -> bool {
            panic!("invalid constructor IO")
        }
    }
    let (mut b, s) = fixture();
    assert_eq!(b.read(128, 1, &mut [0]), Err(I2cError::Invalid));
    assert_eq!(b.read(0x15, 1, &mut []), Err(I2cError::Invalid));
    assert!(s.borrow().events.is_empty());
    assert_eq!(s.borrow().pauses, 0);
    let dropped = Rc::new(std::cell::Cell::new(0));
    let result = BitBangI2c::new(Owned(dropped.clone()), Owned(dropped.clone()), || {}, 0);
    assert!(matches!(result, Err(I2cError::Invalid)));
    assert_eq!(dropped.get(), 2);
    println!("zero-budget new: no I/O but both owned lines dropped, no resources returned");
}
#[test]
fn clock_stretch_can_succeed_on_last_poll_each_wait() {
    #[derive(Debug)]
    struct Stretch {
        inner: Pin,
        attempts: std::cell::Cell<u16>,
    }
    impl Line for Stretch {
        fn low(&mut self) {
            self.inner.low();
        }
        fn release(&mut self) {
            self.attempts.set(0);
            let mut s = self.inner.shared.borrow_mut();
            s.releases += 1;
            s.scl = false;
        }
        fn high(&self) -> bool {
            let n = self.attempts.get() + 1;
            self.attempts.set(n);
            if n < 3 {
                return false;
            }
            let mut s = self.inner.shared.borrow_mut();
            if !s.scl {
                s.scl = true;
                s.rising();
            }
            true
        }
    }
    let state = Rc::new(RefCell::new(State::new()));
    let delay = state.clone();
    let mut b = BitBangI2c::new(
        Stretch {
            inner: Pin {
                clock: true,
                shared: state.clone(),
            },
            attempts: std::cell::Cell::new(0),
        },
        Pin {
            clock: false,
            shared: state.clone(),
        },
        move || delay.borrow_mut().pauses += 1,
        3,
    )
    .unwrap_or_else(|error| panic!("{error:?}"));
    state.borrow_mut().releases = 0;
    let mut out = [0; 2];
    assert_eq!(b.read(0x15, 1, &mut out), Ok(()));
    assert_eq!(out, [0xa5, 0x3c]);
    assert_eq!(state.borrow().pauses, 100 + 49 * 2);
    println!("last-poll success at all 49 waits: 198 delay calls, read={out:?}");
}

#[test]
fn checked_read_observes_both_stops_and_rejects_held_sda() {
    for stop in [1, 2] {
        let (mut bus, state) = fixture();
        state.borrow_mut().stick_stop = Some(stop);
        let mut out = [0xee; 2];
        assert_eq!(
            bus.read_checked(
                NormalAddress7::new(0x15).unwrap_or_else(|error| panic!("{error:?}")),
                1,
                &mut out
            ),
            Err(CheckedReadError::BusNotIdle)
        );
        let state = state.borrow();
        assert!(state.sda);
        assert!(state.scl);
        assert!(!state.actual_data());
        assert_eq!(state.commands.len(), if stop == 1 { 2 } else { 3 });
        assert_eq!(out, if stop == 1 { [0xee; 2] } else { [0xa5, 0x3c] });
        assert!(state.reads < 100, "checked completion must be bounded");
    }
}
#[test]
fn checked_read_accepts_last_budget_sda_release() {
    let (mut bus, state) = fixture();
    {
        let mut s = state.borrow_mut();
        s.stick_stop = Some(1);
        s.release_after = Some(3);
    }
    let mut out = [0; 2];
    assert_eq!(
        bus.read_checked(
            NormalAddress7::new(0x15).unwrap_or_else(|error| panic!("{error:?}")),
            1,
            &mut out
        ),
        Ok(())
    );
    assert_eq!(out, [0xa5, 0x3c]);
    assert_eq!(state.borrow().held_samples, 3);
}
#[test]
fn checked_read_preserves_nack_when_cleanup_sda_is_stuck() {
    for nack in 0..3 {
        let (mut bus, state) = fixture();
        {
            let mut s = state.borrow_mut();
            s.nack_byte = Some(nack);
            s.stick_stop = Some(if nack == 2 { 2 } else { 1 });
        }
        let mut out = [0xee; 2];
        assert_eq!(
            bus.read_checked(
                NormalAddress7::new(0x15).unwrap_or_else(|error| panic!("{error:?}")),
                1,
                &mut out
            ),
            Err(CheckedReadError::Protocol(I2cError::Nack))
        );
        assert_eq!(out, [0xee; 2]);
        let s = state.borrow();
        assert!(s.sda && s.scl);
        assert!(!s.actual_data());
    }
}
#[test]
fn checked_empty_read_is_zero_io() {
    let (mut bus, state) = fixture();
    assert_eq!(
        bus.read_checked(
            NormalAddress7::new(0x15).unwrap_or_else(|error| panic!("{error:?}")),
            1,
            &mut []
        ),
        Err(CheckedReadError::Protocol(I2cError::Invalid))
    );
    let state = state.borrow();
    assert!(state.events.is_empty());
    assert_eq!(state.pauses, 0);
}
#[test]
fn checked_success_has_two_stops_and_final_nack() {
    let (mut bus, state) = fixture();
    let mut out = [0; 2];
    assert_eq!(
        bus.read_checked(
            NormalAddress7::new(0x15).unwrap_or_else(|error| panic!("{error:?}")),
            1,
            &mut out
        ),
        Ok(())
    );
    assert_eq!(out, [0xa5, 0x3c]);
    let s = state.borrow();
    assert_eq!((s.starts, s.stops), (2, 2));
    assert_eq!(s.acks, [false, true]);
}

#[test]
fn checked_timeout_at_every_clock_wait_is_bounded_and_preserves_protocol_error() {
    for wait in 1..=49 {
        let (mut bus, state) = fixture();
        state.borrow_mut().fault = Some(wait);
        let mut out = [0xee; 2];
        assert_eq!(
            bus.read_checked(
                NormalAddress7::new(0x15).unwrap_or_else(|error| panic!("{error:?}")),
                1,
                &mut out
            ),
            Err(CheckedReadError::Protocol(I2cError::ClockTimeout)),
            "wait {wait}"
        );
        let s = state.borrow();
        assert!(s.sda);
        assert!(s.reads < 180);
        if wait <= 39 {
            assert_eq!(out[0], 0xee);
        } else {
            assert_eq!(out[0], 0xa5);
        }
        if wait <= 48 {
            assert_eq!(out[1], 0xee);
        } else {
            assert_eq!(out[1], 0x3c);
        }
    }
}
#[test]
fn checked_completion_samples_clock_again_after_data_release() {
    let (mut bus, state) = fixture();
    state.borrow_mut().hold_clock_after_stop = true;
    assert_eq!(
        bus.read_checked(
            NormalAddress7::new(0x15).unwrap_or_else(|error| panic!("{error:?}")),
            1,
            &mut [0; 2]
        ),
        Err(CheckedReadError::BusNotIdle)
    );
    let s = state.borrow();
    assert_eq!(s.commands.len(), 2);
    assert!(!s.scl);
    assert!(s.sda);
    assert!(s.reads < 70);
}
