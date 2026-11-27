//! Register access for the FR8029D modem and baseband.

use super::{CLOCK_MASK, IRQ_EVENT_END, IRQ_STATUS, PACKET_RAM, SLOT_STRIDE, phy_control_word};
use core::{
    ptr::{read_volatile, write_volatile},
    sync::atomic::{Ordering, fence},
};

#[path = "cold.rs"]
mod cold;

const BASEBAND: usize = 0x4000_0000;
const IRQ_ACK: usize = BASEBAND + 0x20;
const EVENT_STATUS: usize = BASEBAND + 0x24;
const RX_QUEUE: usize = BASEBAND + 0x28;
const CLOCK: usize = BASEBAND + 0x100;
const SUBMIT: usize = BASEBAND + 0x110;
const CLOCK_LATCH: u32 = 1 << 31;

#[cfg_attr(feature = "radio-xip", inline(always))]
fn read32(address: usize) -> u32 {
    // SAFETY: Private callers use aligned registers or owned packet RAM.
    unsafe { read_volatile(address as *const u32) }
}

#[cfg_attr(feature = "radio-xip", inline(always))]
fn write32(address: usize, value: u32) {
    // SAFETY: Private callers use aligned writable chip registers.
    unsafe { write_volatile(address as *mut u32, value) };
}

fn read8(address: usize) -> u8 {
    // SAFETY: Private callers use byte-addressable chip registers or packet RAM.
    unsafe { read_volatile(address as *const u8) }
}

fn write8(address: usize, value: u8) {
    // SAFETY: Private callers use byte-addressable chip registers or packet RAM.
    unsafe { write_volatile(address as *mut u8, value) };
}

/// Load factory RF trim and initialize the FR8029D modem and baseband.
///
/// `WAIT_FOR_IDLE` keeps execution in SRAM on a busy timeout. `MASK_IRQ`
/// excludes interrupt handlers during the trim read.
///
/// # Safety
///
/// The caller owns the FR8029D radio, modem, clocks and QSPI during boot.
/// Enable `radio-xip` and load the SRAM command from the image before calling
/// from Flash. The caller must also make exception paths safe during QSPI access.
///
/// # Errors
///
/// Returns a startup error if the clock, identity, trim or reset is invalid.
pub unsafe fn initialize<const WAIT_FOR_IDLE: bool, const MASK_IRQ: bool>() -> Result<(), u32> {
    cold::init::<WAIT_FOR_IDLE, MASK_IRQ>()
}

/// Reset the scheduler and clear its packet RAM.
///
/// # Safety
///
/// Only the exclusive radio owner may reset the baseband. All outstanding
/// descriptors become invalid.
///
/// # Errors
///
/// Returns the baseband reset timeout.
#[inline]
pub unsafe fn configure_baseband() -> Result<(), u32> {
    cold::configure_baseband()
}

/// Latch the scheduler's coarse clock.
///
/// # Safety
///
/// Only call from the foreground that owns the baseband registers.
#[inline]
pub unsafe fn clock() -> Option<u32> {
    write32(CLOCK, read32(CLOCK) | CLOCK_LATCH);
    for _ in 0..1024 {
        let time = read32(CLOCK);
        if time & CLOCK_LATCH == 0 {
            return Some(time & CLOCK_MASK);
        }
    }
    None
}

/// Get the last scheduler slot selected by the submit register.
///
/// # Safety
///
/// The caller has exclusive access to the FR8029D scheduler.
#[inline]
pub unsafe fn pending_slot() -> u32 {
    read32(SUBMIT)
}

/// Clear the previous scheduler's completion interrupt.
///
/// # Safety
///
/// The caller owns the event and has consumed or discarded its completion.
#[inline]
pub unsafe fn clear_completed_event() {
    write32(IRQ_ACK, IRQ_EVENT_END);
}

/// Read and acknowledge a pending scheduler completion.
///
/// # Safety
///
/// Call only from the event owner. The IRQ handler must leave acknowledgement
/// to this owner.
#[inline]
pub unsafe fn take_event_end() -> Option<u32> {
    if read32(IRQ_STATUS) & IRQ_EVENT_END == 0 {
        return None;
    }
    let status = read32(EVENT_STATUS);
    write32(IRQ_ACK, IRQ_EVENT_END);
    Some(status)
}

/// Read the scheduler error word when it reports a fault.
///
/// # Safety
///
/// The caller owns the baseband register bank.
#[inline]
pub unsafe fn scheduler_fault() -> Option<u32> {
    (read32(BASEBAND + 0x10) & (1 << 16) != 0).then(|| read32(BASEBAND + 0x60))
}

/// Set the receive queue to a packet RAM descriptor list.
///
/// # Safety
///
/// The scheduler must be idle. The first descriptor and all queue links
/// must refer to initialized, aligned records in packet RAM.
#[inline]
pub unsafe fn set_rx_queue(offset: u16) {
    write32(RX_QUEUE + 4, 0);
    write32(RX_QUEUE, u32::from(offset / 4));
}

/// Write an aligned sequence of halfwords into packet RAM.
///
/// # Safety
///
/// The entire record must fit in the caller-owned part of packet RAM.
#[inline]
pub unsafe fn write_record(base: usize, words: &[u16]) {
    for (i, &word) in words.iter().enumerate() {
        // SAFETY: The address calculation stays within the validated record.
        unsafe { write_volatile((base as *mut u16).add(i), word) };
    }
}

/// Publish an already initialized event slot to the scheduler.
///
/// # Safety
///
/// Slot must be in 0..16 and exclusively owned by the caller until submitted.
#[inline]
pub unsafe fn arm_slot(slot: u32) {
    fence(Ordering::Release);
    write32(SUBMIT, 0x8000_0000 | slot);
}

/// Write and publish an event slot to the scheduler.
///
/// # Safety
///
/// Slot must be in 0..16 and exclusively owned by the caller until submitted.
#[inline]
pub unsafe fn submit_slot(slot: u32, words: &[u16; 8]) {
    // SAFETY: The caller owns this scheduler record before publishing it.
    unsafe {
        write_record(PACKET_RAM + slot as usize * SLOT_STRIDE, words);
        arm_slot(slot);
    }
}

/// Read an owned packet RAM byte.
///
/// # Safety
///
/// The address must be in the CPU-owned packet RAM record.
#[inline]
pub unsafe fn packet_byte(address: usize) -> u8 {
    read8(address)
}

/// Write an owned packet RAM byte.
///
/// # Safety
///
/// The address must be in the CPU-owned packet RAM record.
#[inline]
pub unsafe fn set_packet_byte(address: usize, value: u8) {
    write8(address, value);
}

/// Read an owned, aligned packet RAM halfword.
///
/// # Safety
///
/// The address must be aligned and the halfword must be in an owned record.
#[inline]
pub unsafe fn packet_halfword(address: usize) -> u16 {
    // SAFETY: The caller validates alignment and ownership.
    unsafe { read_volatile(address as *const u16) }
}

/// Write an owned, aligned packet RAM halfword.
///
/// # Safety
///
/// The address must be aligned and the halfword must be in an owned record.
#[inline]
pub unsafe fn set_packet_halfword(address: usize, value: u16) {
    // SAFETY: The caller validates alignment and ownership.
    unsafe { write_volatile(address as *mut u16, value) };
}

/// Read an owned, aligned packet RAM word.
///
/// # Safety
///
/// The address must be aligned and the word must be in an owned record.
#[inline]
pub unsafe fn packet_word(address: usize) -> u32 {
    read32(address)
}

/// Apply the selected PHY rates to an installed connection record.
///
/// # Safety
///
/// The caller must own the control record in packet RAM at the supplied
/// offset. Apply the change at its connection instant before submitting a slot.
#[inline]
pub unsafe fn set_phy(control: u16, rates: u8) {
    let address = PACKET_RAM + usize::from(control) + 6;
    // SAFETY: This is the aligned rate word of the caller's connection record.
    unsafe { set_packet_halfword(address, phy_control_word(packet_halfword(address), rates)) };
}
