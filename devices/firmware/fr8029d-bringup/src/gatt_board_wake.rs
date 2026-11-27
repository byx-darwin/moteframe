//! FR8029D baseband completion and TIMER0 wakeup for one BLE owner.

#[cfg(not(feature = "ble-gatt-freertos"))]
use super::read32;
use super::{
    diagnostic, increment,
    radio_hardware::{clock_snapshot, event_end_pending},
    write8, write32,
};
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
use fr8029d_chip::timer0;

const IRQ_ENABLE: usize = 0xe000_e100;
const IRQ_DISABLE: usize = 0xe000_e180;
const IRQ_CLEAR_PENDING: usize = 0xe000_e280;
const IRQ_PRIORITY: usize = 0xe000_e400;
const RADIO_IRQ: u32 = 1;
const TIMER_IRQ: u32 = 1 << timer0::IRQ_NUMBER;
// FR802x implements three priority bits; 4 meets the FreeRTOS syscall ceiling.
const IRQ_PRIORITY_VALUE: u8 = 0x80;

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
    write8(
        IRQ_PRIORITY + usize::from(timer0::IRQ_NUMBER),
        IRQ_PRIORITY_VALUE,
    );
    // SAFETY: TIMER0 is reserved for this wake path. Its NVIC line is masked.
    unsafe { timer0::initialize() };
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
    let (coarse, fine_countdown) = clock_snapshot()?;
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
    // SAFETY: This is the TIMER0 update handler.
    unsafe { timer0::acknowledge_update() };
    increment(TIMER_IRQ_COUNT);
    wake_from_irq();
}

fn stop_timer() {
    // SAFETY: The wake owner stops TIMER0 under the critical section.
    unsafe { timer0::stop() };
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
        event_end_pending()
    }) {
        write32(IRQ_DISABLE, RADIO_IRQ);
        return false;
    }
    if let Some(delay) = delay {
        // SAFETY: The timer is stopped and PRIMASK excludes the handler.
        unsafe { timer0::arm(delay) };
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
