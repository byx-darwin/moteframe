//! FR8029D baseband completion and TIMER0 wakeup for one BLE owner.

use super::{IRQ_EVENT_END, IRQ_STATUS, clock, diagnostic, increment, read32, write8, write32};
#[cfg(not(feature = "ble-gatt-freertos"))]
use core::arch::asm;
use core::{
    cell::Cell,
    sync::atomic::{AtomicU32, Ordering},
    task::{RawWaker, RawWakerVTable, Waker},
};
use critical_section::Mutex;
use fr8029d_bringup::{
    connection::PacketTime,
    wake::{RadioDeadline, WakeSignal},
};

const IRQ_ENABLE: usize = 0xe000_e100;
const IRQ_DISABLE: usize = 0xe000_e180;
const IRQ_CLEAR_PENDING: usize = 0xe000_e280;
const IRQ_PRIORITY: usize = 0xe000_e400;
const RADIO_IRQ: u32 = 1;
const TIMER_IRQ: u32 = 1 << 16;
// FR802x implements three priority bits; 4 meets the FreeRTOS syscall ceiling.
const IRQ_PRIORITY_VALUE: u8 = 0x80;

const TIMER0: usize = 0x5000_0000;
const TIMER_CONTROL: usize = TIMER0;
const TIMER_SLAVE_MODE: usize = TIMER0 + 0x08;
const TIMER_INTERRUPT_ENABLE: usize = TIMER0 + 0x0c;
const TIMER_STATUS: usize = TIMER0 + 0x10;
const TIMER_GENERATE: usize = TIMER0 + 0x14;
const TIMER_PRESCALER: usize = TIMER0 + 0x28;
const TIMER_RELOAD: usize = TIMER0 + 0x2c;
const TIMER_REPETITION: usize = TIMER0 + 0x30;
const TIMER_ONE_SHOT: u32 = (1 << 3) | (1 << 2); // OPM, update interrupt on overflow
const TIMER_UPDATE: u32 = 1;
const MAX_DELAY_US: u32 = 65_536;
const CLOCK_FINE: usize = 0x4000_0104;

const RADIO_IRQ_COUNT: usize = 57;
const TIMER_IRQ_COUNT: usize = 58;
const WAIT_ATTEMPTS: usize = 59;
const MINIMUM_CONNECTION_LEAD: usize = 60;

static SIGNAL: WakeSignal = WakeSignal::new();
static RADIO_DEADLINE: Mutex<Cell<Option<RadioDeadline>>> = Mutex::new(Cell::new(None));
static MINIMUM_LEAD: AtomicU32 = AtomicU32::new(u32::MAX);

#[cfg(feature = "ble-gatt-freertos")]
unsafe extern "C" {
    fn fr29_notify_from_isr();
    fn fr29_wait_notification();
}

pub(super) fn init() {
    write32(IRQ_DISABLE, RADIO_IRQ | TIMER_IRQ);
    write32(IRQ_CLEAR_PENDING, RADIO_IRQ | TIMER_IRQ);
    write8(IRQ_PRIORITY, IRQ_PRIORITY_VALUE);
    write8(IRQ_PRIORITY + 16, IRQ_PRIORITY_VALUE);
    // TIMER0 supplies its own clock selector, divider and APB0 gate.
    let select = 0xe005_3018;
    let divider = 0xe005_301c;
    let gates = 0xe005_3024;
    write32(select, read32(select) & !3);
    write32(divider, read32(divider) & !15);
    write32(gates, read32(gates) | 1);
    write32(TIMER_CONTROL, TIMER_ONE_SHOT);
    write32(TIMER_SLAVE_MODE, 0);
    write32(TIMER_INTERRUPT_ENABLE, 0);
    write32(TIMER_PRESCALER, 23); // 24 MHz / 24 = 1 MHz
    write32(TIMER_REPETITION, 0);
    write32(TIMER_STATUS, !TIMER_UPDATE);
    write32(IRQ_ENABLE, TIMER_IRQ);
    diagnostic(MINIMUM_CONNECTION_LEAD, u32::MAX);
    #[cfg(not(feature = "ble-gatt-freertos"))]
    {
        let sleep_control = 0xe000_ed10;
        write32(sleep_control, read32(sleep_control) & !0x6); // normal sleep, return to Thread mode
        // SAFETY: VTOR and both interrupt handlers are ready. The probe owns NVIC.
        unsafe {
            asm!(
                "dsb sy",
                "cpsie i",
                "isb sy",
                options(nostack, preserves_flags)
            );
        }
    }
}

pub(super) fn snapshot() -> Option<PacketTime> {
    let coarse = clock()?;
    let fine_countdown = u16::try_from(read32(CLOCK_FINE) & 0x3ff).ok()?;
    if fine_countdown > PacketTime::FINE_AT_BOUNDARY {
        return None;
    }
    Some(PacketTime {
        coarse,
        fine_countdown,
    })
}

pub(super) fn begin_pass() {
    SIGNAL.take();
}

pub(super) fn record_connection_lead(lead: u32) {
    if lead < MINIMUM_LEAD.load(Ordering::Relaxed) {
        MINIMUM_LEAD.store(lead, Ordering::Relaxed);
        diagnostic(MINIMUM_CONNECTION_LEAD, lead);
    }
}

pub(super) fn set_radio_deadline(deadline: Option<RadioDeadline>) {
    critical_section::with(|cs| RADIO_DEADLINE.borrow(cs).set(deadline));
}

pub(super) fn radio_deadline() -> Option<RadioDeadline> {
    critical_section::with(|cs| RADIO_DEADLINE.borrow(cs).get())
}

pub(super) fn waker() -> Waker {
    fn raw() -> RawWaker {
        static ACTIONS: RawWakerVTable =
            RawWakerVTable::new(|_| raw(), |_| SIGNAL.signal(), |_| SIGNAL.signal(), |_| {});
        RawWaker::new(core::ptr::null(), &ACTIONS)
    }
    // SAFETY: The vtable touches one static atomic and owns no pointer data.
    unsafe { Waker::from_raw(raw()) }
}

fn wake_from_irq() {
    SIGNAL.signal();
    #[cfg(feature = "ble-gatt-freertos")]
    // SAFETY: These vectors use priority 4 and are armed by the running BLE task.
    unsafe {
        fr29_notify_from_isr();
    }
}

#[unsafe(no_mangle)]
extern "C" fn fr29_baseband_irq() {
    // Keep the level from retriggering. The BLE owner reads the event status
    // before ACK and enables the vector again when waiting on another slot.
    write32(IRQ_DISABLE, RADIO_IRQ);
    increment(RADIO_IRQ_COUNT);
    wake_from_irq();
}

#[unsafe(no_mangle)]
extern "C" fn fr29_timer_irq() {
    write32(TIMER_STATUS, !TIMER_UPDATE);
    increment(TIMER_IRQ_COUNT);
    wake_from_irq();
}

fn stop_timer() {
    write32(TIMER_CONTROL, TIMER_ONE_SHOT);
    write32(TIMER_INTERRUPT_ENABLE, 0);
    write32(TIMER_STATUS, !TIMER_UPDATE);
    write32(IRQ_CLEAR_PENDING, TIMER_IRQ);
}

// Called with PRIMASK set through critical_section::with. A new pending IRQ
// after the last check wakes WFI, or leaves a notification for the RTOS task.
fn prepare_wait(delay: Option<u32>, receiving: bool) -> bool {
    if !SIGNAL.should_wait(delay, || {
        if !receiving {
            return false;
        }
        write32(IRQ_CLEAR_PENDING, RADIO_IRQ);
        write32(IRQ_ENABLE, RADIO_IRQ);
        read32(IRQ_STATUS) & IRQ_EVENT_END != 0
    }) {
        write32(IRQ_DISABLE, RADIO_IRQ);
        return false;
    }
    if let Some(delay) = delay {
        write32(TIMER_RELOAD, delay.clamp(2, MAX_DELAY_US) - 1);
        write32(TIMER_GENERATE, TIMER_UPDATE);
        write32(TIMER_STATUS, !TIMER_UPDATE);
        write32(TIMER_INTERRUPT_ENABLE, TIMER_UPDATE);
        write32(TIMER_CONTROL, TIMER_ONE_SHOT | 1);
    }
    increment(WAIT_ATTEMPTS);
    true
}

pub(super) fn wait(delay: Option<u32>, receiving: bool) {
    let prepared = critical_section::with(|_| {
        if !prepare_wait(delay, receiving) {
            return false;
        }
        #[cfg(not(feature = "ble-gatt-freertos"))]
        // SAFETY: NVIC remains enabled under PRIMASK. Pending maskable wake
        // events break WFI and are handled when the saved mask is restored.
        unsafe {
            asm!("dsb sy", "wfi", "isb sy", options(nostack, preserves_flags));
        }
        true
    });
    if prepared {
        #[cfg(feature = "ble-gatt-freertos")]
        // SAFETY: This is the only task waiting on this counting notification.
        unsafe {
            fr29_wait_notification();
        }
        critical_section::with(|_| {
            stop_timer();
            write32(IRQ_DISABLE, RADIO_IRQ);
        });
    }
}
