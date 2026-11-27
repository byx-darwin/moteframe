//! MIC1 acquisition state used by the single foreground BLE runner.
use crate::{mic_config, mic1};
use core::cell::RefCell;
use critical_section::Mutex;

const CAPACITY: usize = 32;
struct Capture {
    running: bool,
    samples: [u16; CAPACITY],
    raw: u32,
    produced: u32,
    dropped: u32,
    full_polls: u32,
    queued: u8,
    skip: u16,
}

impl Capture {
    const fn new() -> Self {
        Self {
            running: false,
            samples: [0; CAPACITY],
            raw: 0,
            produced: 0,
            dropped: 0,
            full_polls: 0,
            queued: 0,
            skip: 0,
        }
    }

    fn push(&mut self, value: u16, divisor: u16) {
        self.raw = self.raw.wrapping_add(1);
        if self.skip > 0 {
            self.skip -= 1;
            return;
        }
        self.skip = divisor - 1;
        self.samples[self.produced as usize % CAPACITY] = value;
        self.produced = self.produced.wrapping_add(1);
        if usize::from(self.queued) == CAPACITY {
            self.dropped = self.dropped.saturating_add(1);
        } else {
            self.queued += 1;
        }
    }
}

static CAPTURE: Mutex<RefCell<Capture>> = Mutex::new(RefCell::new(Capture::new()));

fn state<R>(f: impl FnOnce(&mut Capture) -> R) -> R {
    critical_section::with(|cs| f(&mut CAPTURE.borrow_ref_mut(cs)))
}

pub(crate) fn poll_interval_us() -> Option<u32> {
    state(|s| s.running.then_some(2_000_000 / mic1::SAMPLE_RATE_HZ))
}

pub(crate) fn poll() {
    if poll_interval_us().is_none() {
        return;
    }
    let divisor = mic_config::value();
    for _ in 0..64 {
        let Some(value) = mic1::poll_sample() else {
            break;
        };
        state(|s| s.push(value, divisor));
    }
    if mic1::take_overflow() {
        state(|s| s.full_polls = s.full_polls.saturating_add(1));
    }
}

pub(crate) fn on_boot() {
    mic_config::restore();
}

pub(crate) fn on_disconnect() {
    stop();
    mic_config::maintenance();
}

fn stop() {
    if state(|s| core::mem::take(&mut s.running)) {
        mic1::stop();
    }
}

pub(crate) fn request(frame: &[u8], out: &mut [u8; 20]) -> bool {
    let [0xad, op @ 1..=5, lo, hi] = *frame else {
        return false;
    };
    let arg = u16::from_le_bytes([lo, hi]);
    out.fill(0);
    out[0] = 0xad;
    out[1] = op;
    match op {
        1 => {
            if poll_interval_us().is_none() {
                let ready = mic1::init();
                state(|s| {
                    *s = Capture::new();
                    s.running = ready;
                });
            }
        }
        2 => {
            poll();
            state(|s| {
                let start = s.produced.wrapping_sub(u32::from(s.queued));
                out[4..8].copy_from_slice(&start.to_le_bytes());
                let count = s.queued.min(5);
                out[3] = count;
                for i in 0..count {
                    let index = start.wrapping_add(u32::from(i)) as usize % CAPACITY;
                    let pos = 10 + usize::from(i) * 2;
                    out[pos..pos + 2].copy_from_slice(&s.samples[index].to_le_bytes());
                }
                s.queued -= count;
            });
        }
        3 => {
            poll();
            stop();
        }
        4 if (1..=64).contains(&arg) => {
            mic_config::apply(arg);
        }
        4 => out[2] |= 4,
        _ => {}
    }
    out[2] |= u8::from(mic_config::pending()) << 1;
    state(|s| {
        out[2] |= u8::from(s.running);
        if op != 2 {
            out[3] = s.queued;
            out[4..8].copy_from_slice(&s.raw.to_le_bytes());
            out[8..12].copy_from_slice(&s.produced.to_le_bytes());
            out[12..16].copy_from_slice(&s.dropped.to_le_bytes());
            out[16..18].copy_from_slice(
                &u16::try_from(s.full_polls)
                    .unwrap_or(u16::MAX)
                    .to_le_bytes(),
            );
            out[18..20].copy_from_slice(&mic_config::value().to_le_bytes());
        }
    });
    true
}
