//! FR8029D DEV0.2 MIC1 acquisition, owned by the board event loop.
//!
//! C45/C46 connect MIC1 to dedicated `MIC_INP`/`MIC_INN` (pins 46/47).
//! R71 supplies `MIC_BIAS` from pin 60. PC14/PC15 are separate ASP inputs.
//! The analog settings follow the `FR802x` SDK v0.3.6 SARADC codec example.
//! See also reference manual v1.5, §§18.4.2, 18.4.5–6 for slot timing and FIFO.

use core::{
    ptr::{read_volatile, write_volatile},
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
};

const ADC: usize = 0x500e_0000;
const ASP: usize = 0x500f_0000;
const SYS: usize = 0xe005_3000;
const FRSPIM: usize = 0xe008_0000;
const CONTROL: usize = ADC;
const CONFIG: usize = ADC + 0x04;
const FIFO_COUNT: usize = ADC + 0x54;
const RAW_STATUS: usize = ADC + 0x60;
const FIFO_FULL: u32 = 1 << 1;
const CONVERT_ERROR: u32 = 1 << 5;
const ADC_GATES: u32 = 3 << 14;
const PMU0_MASK: u32 = 0x33; // REF_OP_PD, IBIAS_PD, MICBIAS_PWR_EN, PWR
const PMU1_MASK: u32 = 0xb9; // SARADC, PGA, REF, IBIAS and ASP power
const CLOCK_DIV: u32 = 250;
const SLOTS: u32 = 16;

/// Nominal delivered sample rate. Each 30-clock conversion uses 1 SOC +
/// 16 sample + 12 conversion + 1 EOC. Only slot zero enters the FIFO.
pub const SAMPLE_RATE_HZ: u32 = 24_000_000 / CLOCK_DIV / 30 / SLOTS;

static RUNNING: AtomicBool = AtomicBool::new(false);
static OVERRUN: AtomicBool = AtomicBool::new(false);
static SAVED_POWER: AtomicU32 = AtomicU32::new(0);

fn read(address: usize) -> u32 {
    // SAFETY: internal callers use aligned FR8029D peripheral registers.
    unsafe { read_volatile(address as *const u32) }
}

fn write(address: usize, value: u32) {
    // SAFETY: internal callers use aligned writable FR8029D peripheral registers.
    unsafe { write_volatile(address as *mut u32, value) }
}

fn change(address: usize, mask: u32, bits: u32) {
    write(address, (read(address) & !mask) | bits);
}

fn pmu(address: u8, value: Option<u32>) -> Option<u32> {
    // The PMU serial port is also used by board power management.
    critical_section::with(|_| {
        let mut command = (u32::from(address) << 16) | 0x11; // GO, one byte, PMU
        if let Some(data) = value {
            write(FRSPIM + 4, data);
            command |= 1 << 8;
        }
        write(FRSPIM, command);
        (0..512).find(|_| read(FRSPIM) & 3 == 2)?;
        Some(read(FRSPIM + 8) & 0xff)
    })
}

fn restore_pmu(saved: u32) {
    for (address, mask, old) in [(0xef, PMU1_MASK, saved >> 8), (0xee, PMU0_MASK, saved)] {
        if let Some(now) = pmu(address, None) {
            let _ = pmu(address, Some((now & !mask) | (old & mask)));
        }
    }
}

fn settle() {
    // At least 5 us even at the maximum 96 MHz core clock; independent of SysTick.
    for _ in 0..512 {
        core::hint::spin_loop();
    }
}

/// Enable continuous hardware sampling. Returns false on a PMU or reset timeout.
pub fn init() -> bool {
    if RUNNING.load(Ordering::Relaxed) {
        return true;
    }
    change(SYS + 0x14, 0, 1 << 3); // shared PMU serial clock
    let (Some(power0), Some(power1)) = (pmu(0xee, None), pmu(0xef, None)) else {
        return false;
    };
    let saved = power0 | (power1 << 8) | (read(SYS + 0x24) & ADC_GATES) << 2;
    if pmu(0xee, Some((power0 & !PMU0_MASK) | 3)).is_none()
        || pmu(0xef, Some(power1 | PMU1_MASK)).is_none()
    {
        restore_pmu(saved);
        return false;
    }
    SAVED_POWER.store(saved, Ordering::Relaxed);
    change(SYS + 0x18, 1 << 20, 0); // 24 MHz crystal
    change(SYS + 0x20, 15 << 8, 0); // APB SARADC divider /1
    change(SYS + 0x24, 0, ADC_GATES);
    settle();
    change(ASP + 4, 1 << 5, 1 << 6); // power up SARADC, enable pads
    settle();
    change(ASP + 4, 0, 1 << 8); // release analog reset
    change(CONTROL, 0xc3, 0); // stop conversions, power on, assert analog reset
    change(CONTROL, 0, 1 << 7);
    if !(0..512).any(|_| read(CONTROL) & (1 << 7) != 0) {
        power_down(saved);
        return false;
    }
    change(CONTROL, 0, 1 << 3); // digital reset
    change(CONTROL, 0, 1 << 4); // FIFO reset
    change(ASP + 4, 0x1c, 1 << 9); // enable mic bias, PGA, PGA bias, reference buffer
    change(ASP + 0x34, 1 << 3, 0); // dedicated MIC inputs
    change(ASP + 0x38, 15, 0); // SDK codec example: 0.8 V MIC_BIAS
    change(ASP + 0x40, 15, 7); // PGA gain
    change(ASP + 0x44, 31, 5); // PGA switches
    change(ASP + 0x48, 3 << 14, 0); // reference buffer unity gain
    change(ASP + 0x4c, 7 << 8, 2 << 8); // select internal reference
    change(ASP + 0x50, 15 << 4, 15 << 4); // 16 sample clocks
    change(ASP + 0x54, 3 << 8, 3 << 8); // 1.2 V reference
    change(CONTROL, 0x7f00, 0x7e00); // dynamic SF/ATT/DIFF/CH/RSTN/PD
    change(CONFIG, 0xff1f_1f0d, ((SLOTS - 1) << 8) | 0x0c);
    write(ADC + 0x08, 0); // unscaled, unsigned 12-bit ADC result
    write(ADC + 0x0c, 0xff00 | (CLOCK_DIV / 2)); // no interval between slots
    for slot in 0..SLOTS as usize {
        write(ADC + 0x64 + slot * 4, (1 << 22) | (0x0c << 8) | 0x0d);
    }
    write(ADC + 0x4c, 0); // polling only, DMA disabled
    write(ADC + 0x58, 0); // no ADC IRQ
    write(ADC + 0x5c, CONVERT_ERROR);
    OVERRUN.store(false, Ordering::Relaxed);
    change(CONTROL, 0, 1); // free-running loop, slot 0 to FIFO
    RUNNING.store(true, Ordering::Relaxed);
    true
}

fn power_down(saved: u32) {
    change(CONTROL, 3, (1 << 4) | (1 << 6));
    change(ASP + 4, (1 << 9) | (1 << 8) | (1 << 6), 0x3c);
    change(SYS + 0x24, ADC_GATES, (saved >> 2) & ADC_GATES);
    restore_pmu(saved);
}

/// Stop converting, discard queued samples and release microphone power.
pub fn stop() {
    if RUNNING.swap(false, Ordering::Relaxed) {
        power_down(SAVED_POWER.load(Ordering::Relaxed));
    }
}

/// Pop at most one raw 12-bit sample. Never waits for a conversion.
pub fn poll_sample() -> Option<u16> {
    if !RUNNING.load(Ordering::Relaxed) {
        return None;
    }
    let count = read(FIFO_COUNT);
    let status = read(RAW_STATUS);
    if count >= 64 || status & (FIFO_FULL | CONVERT_ERROR) != 0 {
        OVERRUN.store(true, Ordering::Relaxed);
        if status & CONVERT_ERROR != 0 {
            write(ADC + 0x5c, CONVERT_ERROR);
        }
    }
    (count != 0).then(|| (read(ADC + 0x48) & 0x0fff) as u16)
}

/// Report and clear an observed FIFO-full or conversion-error episode. The
/// hardware has no dropped-sample counter; FIFO full is a conservative signal.
pub fn take_overflow() -> bool {
    OVERRUN.swap(false, Ordering::Relaxed)
}
