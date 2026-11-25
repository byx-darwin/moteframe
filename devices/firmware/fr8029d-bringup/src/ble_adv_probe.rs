//! SRAM advertising experiments for warm and pre-application FR8029D startup.
#![no_std]
#![no_main]

#[cfg(not(feature = "ble-gatt-board"))]
use core::sync::atomic::{Ordering, fence};
use core::{
    arch::{asm, global_asm},
    panic::PanicInfo,
    ptr::{read_volatile, write_volatile},
};
#[cfg(all(feature = "ble-connect-probe", not(feature = "ble-gatt-board")))]
use fr8029d_bringup::advertising::{CONNECTABLE_ADDRESS, CONNECTABLE_DATA, connectable_control};
#[cfg(not(feature = "ble-gatt-board"))]
use fr8029d_bringup::advertising::{
    CONTROL_OFFSET, TX_PAYLOAD_OFFSET, advance_clock, advertising_slot, slot_complete,
};
#[cfg(not(feature = "ble-connect-probe"))]
use fr8029d_bringup::advertising::{TX_OFFSET, advertising_control, tx_descriptor};
use fr8029d_bringup::{
    XIP_ENTRY,
    advertising::{CLOCK_MASK, SLOT_STRIDE, status_slot},
};

global_asm!(
    ".set __fr29_use_freertos, {freertos}",
    ".set __fr29_use_event_wake, {event_wake}",
    include_str!("startup.s"),
    freertos = const cfg!(feature = "ble-gatt-freertos") as u8,
    event_wake = const cfg!(all(feature = "ble-gatt-board", feature = "ble-gatt-event-driven")) as u8,
);

#[cfg(feature = "ble-adv-cold")]
mod cold_radio;

#[cfg(feature = "ble-connect-probe")]
mod connect_probe;
#[cfg(feature = "ble-connect-probe")]
mod connect_radio;
#[cfg(feature = "ble-gatt-board")]
mod gatt_board_radio;
#[cfg(feature = "ble-gatt-board")]
mod gatt_board_time;
#[cfg(all(feature = "ble-gatt-board", feature = "ble-gatt-event-driven"))]
mod gatt_board_wake;

#[cfg(feature = "ble-adv-cold")]
fn read8(address: usize) -> u8 {
    // SAFETY: Cold initialization uses only byte-addressed MMIO registers.
    unsafe { read_volatile(address as *const u8) }
}

#[cfg(feature = "ble-adv-cold")]
fn write8(address: usize, value: u8) {
    // SAFETY: Cold initialization uses only byte-addressed MMIO registers.
    unsafe { write_volatile(address as *mut u8, value) };
}

const PACKET_RAM: usize = 0x4001_0000;
#[cfg(not(feature = "ble-gatt-board"))]
const BASEBAND_ID: usize = 0x4000_0004;
#[cfg(not(feature = "ble-gatt-board"))]
const IRQ_MASK: usize = 0x4000_0018;
const IRQ_STATUS: usize = 0x4000_001c;
const IRQ_ACK: usize = 0x4000_0020;
const EVENT_STATUS: usize = 0x4000_0024;
const CLOCK: usize = 0x4000_0100;
const SUBMIT: usize = 0x4000_0110;
const IRQ_EVENT_END: u32 = 1 << 15;
const CLOCK_LATCH: u32 = 1 << 31;
#[cfg(not(feature = "ble-adv-cold"))]
const EVENT_LIMIT: u32 = 460;
#[cfg(not(feature = "ble-adv-cold"))]
const SLOT_LEAD_TICKS: u32 = 2;
#[cfg(feature = "ble-adv-cold")]
const SLOT_LEAD_TICKS: u32 = 12;
#[cfg(all(feature = "ble-adv-cold", not(feature = "ble-gatt-board")))]
const EVENT_LIMIT: u32 = 2500;
#[cfg(all(
    feature = "ble-adv-cold",
    not(any(feature = "ble-adv-cold-xip", feature = "ble-connect-probe"))
))]
const COLD_DATA: &[u8; 15] = b"\x02\x01\x06\x0b\x09FR29-CLD03";
#[cfg(all(feature = "ble-adv-cold-xip", not(feature = "ble-connect-probe")))]
const COLD_DATA: &[u8; 15] = b"\x02\x01\x06\x0b\x09FR29-BOOT1";

unsafe extern "C" {
    static mut __diagnostics_start: [u32; 64];
    #[cfg(feature = "ble-gatt-board")]
    static __stack_bottom: u8;
    #[cfg(feature = "ble-gatt-freertos")]
    fn fr29_freertos_start(entry: extern "C" fn(*mut core::ffi::c_void));
}

// Keep the MMIO access inside the SRAM-resident QSPI transaction.
#[cfg_attr(
    feature = "ble-adv-cold-xip",
    inline(always),
    allow(clippy::inline_always)
)]
fn read32(address: usize) -> u32 {
    // SAFETY: Call sites use only word-aligned FR8029D baseband registers mapped on the pinned board.
    unsafe { read_volatile(address as *const u32) }
}

#[cfg_attr(
    feature = "ble-adv-cold-xip",
    inline(always),
    allow(clippy::inline_always)
)]
fn write32(address: usize, value: u32) {
    // SAFETY: Call sites use only the documented command and acknowledgement registers, with IRQs masked.
    unsafe { write_volatile(address as *mut u32, value) };
}

fn write_record(base: usize, words: &[u16]) {
    // SAFETY: Callers supply aligned, nonoverlapping fixed Packet RAM records during exclusive ownership.
    unsafe {
        for (index, &value) in words.iter().enumerate() {
            write_volatile((base as *mut u16).add(index), value);
        }
    }
}

fn diagnostic(index: usize, value: u32) {
    // SAFETY: The linker reserves 64 words. All producers use constant bounded indices.
    unsafe {
        write_volatile(
            (&raw mut __diagnostics_start).cast::<u32>().add(index),
            value,
        );
    }
}

#[cfg(feature = "ble-gatt-board")]
fn increment(index: usize) {
    // SAFETY: Each trace index has one producer on this core. IRQ counters
    // use separate indices from the foreground counters.
    let previous =
        unsafe { read_volatile((&raw const __diagnostics_start).cast::<u32>().add(index)) };
    diagnostic(index, previous.saturating_add(1));
}

fn clock() -> Option<u32> {
    write32(CLOCK, read32(CLOCK) | CLOCK_LATCH);
    for _ in 0..1024 {
        let time = read32(CLOCK);
        if time & CLOCK_LATCH == 0 {
            return Some(time & CLOCK_MASK);
        }
    }
    None
}

#[cfg(not(feature = "ble-gatt-board"))]
fn delay_ticks(ticks: u32) -> Option<u32> {
    let start = clock()?;
    for _ in 0..2_000_000 {
        let now = clock()?;
        if now.wrapping_sub(start) & CLOCK_MASK >= ticks {
            return Some(now);
        }
    }
    None
}

fn exchange_slot(index: u32) -> usize {
    PACKET_RAM + index as usize * SLOT_STRIDE
}

#[cfg(not(feature = "ble-gatt-board"))]
fn wait_event(index: u32) -> Result<(), u32> {
    for _ in 0..300_000 {
        #[cfg(feature = "ble-adv-cold")]
        if read32(0x4000_0010) & (1 << 16) != 0 {
            diagnostic(15, read32(0x4000_0060));
            return Err(15);
        }
        let irq = read32(IRQ_STATUS);
        if irq & IRQ_EVENT_END == 0 {
            continue;
        }
        let status = read32(EVENT_STATUS);
        write32(IRQ_ACK, IRQ_EVENT_END);
        diagnostic(6, status);
        if status & 2 == 0 || status_slot(status) != index {
            continue;
        }
        // SAFETY: index is masked to the 16 hardware entries. The status flag is a halfword.
        let flags = unsafe { read_volatile(exchange_slot(index) as *const u16) };
        diagnostic(7, u32::from(flags));
        return if slot_complete(flags) { Ok(()) } else { Err(5) };
    }
    Err(4)
}

#[cfg(not(feature = "ble-gatt-board"))]
fn advertise(boot_vtor: u32) -> Result<(), u32> {
    if boot_vtor != XIP_ENTRY {
        return Err(2);
    }
    #[cfg(feature = "ble-adv-cold")]
    cold_radio::init()?;

    if read32(BASEBAND_ID) != 0x1949_1001 || read32(IRQ_MASK) & IRQ_EVENT_END == 0 {
        return Err(2);
    }
    // Let the previous firmware's final event finish with the CPU scheduler stopped.
    let start = delay_ticks(64).ok_or(3_u32)?;
    diagnostic(5, start);
    #[cfg(feature = "ble-adv-cold")]
    let mut index = 0;
    #[cfg(not(feature = "ble-adv-cold"))]
    let mut index = (read32(SUBMIT) + 1) & 15;
    write32(IRQ_ACK, IRQ_EVENT_END);
    #[cfg(feature = "ble-connect-probe")]
    let (control, adv_data) = (
        connectable_control(CONNECTABLE_ADDRESS),
        CONNECTABLE_DATA.as_slice(),
    );
    #[cfg(all(feature = "ble-adv-cold", not(feature = "ble-connect-probe")))]
    let (control, adv_data) = (advertising_control(), COLD_DATA.as_slice());
    #[cfg(not(feature = "ble-adv-cold"))]
    let (control, adv_data) = (
        advertising_control(),
        fr8029d_bringup::advertising::ADV_DATA.as_slice(),
    );
    write_record(PACKET_RAM + usize::from(CONTROL_OFFSET), &control);
    let payload_address = PACKET_RAM + usize::from(TX_PAYLOAD_OFFSET);
    // SAFETY: The payload fits the fixed Packet RAM buffer, which has not been submitted.
    unsafe {
        for (index, &byte) in adv_data.iter().enumerate() {
            write_volatile((payload_address as *mut u8).add(index), byte);
        }
    }
    #[cfg(feature = "ble-connect-probe")]
    let mut receiver = connect_probe::Receiver::start();

    let mut jitter = start.wrapping_add(1);
    for count in 0..EVENT_LIMIT {
        #[cfg(not(feature = "ble-connect-probe"))]
        write_record(PACKET_RAM + usize::from(TX_OFFSET), &tx_descriptor());
        let now = clock().ok_or(3_u32)?;
        write_record(
            exchange_slot(index),
            &advertising_slot(advance_clock(now, SLOT_LEAD_TICKS)),
        );
        diagnostic(10, read32(0x4000_0120));
        diagnostic(11, read32(0x4000_0140));
        diagnostic(12, read32(0x4000_0144));
        diagnostic(13, read32(0x4000_0150));
        diagnostic(14, read32(0x4000_0000));
        fence(Ordering::Release);
        write32(SUBMIT, 0x8000_0000 | index);
        diagnostic(2, 1);
        diagnostic(3, count + 1);
        diagnostic(8, index);
        let result = wait_event(index);
        #[cfg(feature = "ble-connect-probe")]
        if matches!(result, Ok(()) | Err(5))
            && let Some((request, time)) = receiver.drain(CONNECTABLE_ADDRESS)?
        {
            return connect_radio::probe_link(&mut receiver, request, time, (index + 1) & 15);
        }
        result?;
        diagnostic(4, count + 1);
        index = (index + 1) & 15;
        jitter = jitter.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        delay_ticks(320 + ((jitter >> 24) % 33)).ok_or(3_u32)?;
    }
    Ok(())
}

#[unsafe(no_mangle)]
extern "C" fn fr8029d_probe_main(boot_vtor: u32) -> ! {
    let words = if cfg!(feature = "ble-gatt-board") {
        64
    } else if cfg!(feature = "ble-connect-probe") {
        40
    } else {
        16
    };
    for index in 0..words {
        diagnostic(index, 0);
    }
    diagnostic(1, 1);
    diagnostic(9, boot_vtor);
    diagnostic(0, u32::from_le_bytes(*b"FA1M"));
    #[cfg(feature = "ble-gatt-board")]
    {
        if boot_vtor != XIP_ENTRY {
            diagnostic(2, 2);
            park();
        }
        // SAFETY: The symbol is the lower limit of the linker-reserved MSP
        // stack. The probe always runs in privileged Thread mode on MSP.
        unsafe {
            asm!("msr MSPLIM, {limit}", limit = in(reg) &raw const __stack_bottom,
                 options(nostack, preserves_flags));
        }
        #[cfg(feature = "ble-gatt-freertos")]
        {
            // SAFETY: Called once on the startup MSP. The kernel owns the
            // static task stacks and passes its unused task parameter to Rust.
            unsafe { fr29_freertos_start(run_ble_task) };
            diagnostic(2, 3);
            park()
        }
        #[cfg(not(feature = "ble-gatt-freertos"))]
        gatt_board_time::run(gatt_board_radio::Radio::new())
    }
    #[cfg(not(feature = "ble-gatt-board"))]
    run_advertisement_probe(boot_vtor)
}

#[cfg(feature = "ble-gatt-freertos")]
extern "C" fn run_ble_task(_parameter: *mut core::ffi::c_void) {
    gatt_board_time::run(gatt_board_radio::Radio::new())
}

#[cfg(not(feature = "ble-gatt-board"))]
fn run_advertisement_probe(boot_vtor: u32) -> ! {
    let state = match advertise(boot_vtor) {
        Ok(()) => 6,
        Err(code) => code,
    };
    diagnostic(2, state);
    park()
}

#[panic_handler]
fn panic(_info: &PanicInfo<'_>) -> ! {
    park()
}

fn park() -> ! {
    loop {
        // SAFETY: WFI does not dereference memory or change the stack and waits with IRQs masked.
        unsafe { asm!("wfi", options(nomem, nostack, preserves_flags)) };
    }
}
