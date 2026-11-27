//! Exercise the foreground MIC command handler with a simulated ADC and store.
#[path = "../src/mic_session.rs"]
mod mic_session;

mod mic1 {
    use std::{cell::Cell, cell::RefCell, collections::VecDeque};

    #[allow(dead_code)]
    pub const SAMPLE_RATE_HZ: u32 = 200;
    thread_local! {
        static FIFO: RefCell<VecDeque<u16>> = RefCell::default();
        static RUNNING: Cell<bool> = const { Cell::new(false) };
        static FULL: Cell<bool> = const { Cell::new(false) };
    }

    pub fn start_fixture(samples: core::ops::Range<u16>) {
        assert!(RUNNING.get());
        FIFO.with_borrow_mut(|q| q.extend(samples));
        FULL.set(true);
    }

    pub fn init() -> bool {
        assert!(!RUNNING.replace(true));
        true
    }

    pub fn poll_sample() -> Option<u16> {
        assert!(RUNNING.get());
        FIFO.with_borrow_mut(VecDeque::pop_front)
    }

    pub fn take_overflow() -> bool {
        FULL.replace(false)
    }

    pub fn stop() {
        assert!(RUNNING.replace(false));
    }

    pub fn running() -> bool {
        RUNNING.get()
    }
}

mod mic_config {
    use std::cell::Cell;

    thread_local! {
        static ACTIVE: Cell<u16> = const { Cell::new(0) };
        static DIRTY: Cell<bool> = const { Cell::new(false) };
        static SAVED: Cell<u16> = const { Cell::new(2) };
        static SAVES: Cell<usize> = const { Cell::new(0) };
    }

    pub fn value() -> u16 {
        ACTIVE.get()
    }

    pub fn pending() -> bool {
        DIRTY.get()
    }

    pub fn restore() {
        ACTIVE.set(SAVED.get());
    }

    pub fn apply(value: u16) {
        ACTIVE.set(value);
        DIRTY.set(true);
    }

    pub fn maintenance() {
        assert!(!super::mic1::running(), "ADC must stop before maintenance");
        if DIRTY.replace(false) {
            SAVED.set(ACTIVE.get());
            SAVES.set(SAVES.get() + 1);
        }
    }

    pub fn persisted() -> (u16, usize) {
        (SAVED.get(), SAVES.get())
    }
}

fn command(op: u8, arg: u16) -> [u8; 20] {
    let [lo, hi] = arg.to_le_bytes();
    let mut out = [0xff; 20];
    assert!(mic_session::request(&[0xad, op, lo, hi], &mut out));
    assert_eq!(out[..2], [0xad, op]);
    out
}

fn count(frame: &[u8; 20], offset: usize) -> u32 {
    u32::from_le_bytes(core::array::from_fn(|i| frame[offset + i]))
}

#[test]
fn ring_wrap_decimation_and_deferred_save_across_stop() {
    mic_session::on_boot();
    assert_eq!(mic_config::value(), 2);
    assert_eq!(command(1, 0)[2], 1);
    mic1::start_fixture(0..64);
    mic_session::poll();
    mic_session::poll();
    let initial = command(5, 0);
    assert_eq!(count(&initial, 4), 64);
    assert_eq!(count(&initial, 8), 32);
    assert_eq!(count(&initial, 12), 0);
    assert_eq!(initial[3], 32);
    assert_eq!(initial[16..18], 1u16.to_le_bytes());

    // Four more retained samples overwrite exactly the oldest four entries.
    mic1::start_fixture(64..72);
    mic_session::poll();
    mic_session::poll();
    let full = command(5, 0);
    assert_eq!(count(&full, 4), 72);
    assert_eq!(count(&full, 8), 36);
    assert_eq!(count(&full, 12), 4);
    assert_eq!(full[16..18], 2u16.to_le_bytes());

    // Read across the ring boundary, then verify the partially filled final notification.
    let mut sequence = 4;
    while sequence < 36 {
        let packet = command(2, 0);
        assert_eq!(count(&packet, 4), sequence);
        assert_eq!(u32::from(packet[3]), (36 - sequence).min(5));
        for sample in packet[10..].chunks_exact(2).take(usize::from(packet[3])) {
            assert_eq!(
                u32::from(u16::from_le_bytes([sample[0], sample[1]])),
                sequence * 2
            );
            sequence += 1;
        }
    }
    assert_eq!(command(2, 0)[3], 0);
    assert_eq!(command(4, 3)[2], 3);
    assert_eq!(mic_config::persisted(), (2, 0));
    let stopped = command(3, 0);
    assert_eq!(stopped[2], 2);
    assert_eq!(stopped[18..20], 3u16.to_le_bytes());
    assert_eq!(mic_config::persisted(), (2, 0));
    assert!(!mic1::running());
    mic_session::on_disconnect();
    assert_eq!(mic_config::persisted(), (3, 1));
    assert_eq!(command(5, 0)[2], 0);
}
