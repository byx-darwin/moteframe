//! The foreground's FR8029D radio operations.
use fr8029d_chip::radio;

pub(super) fn clock() -> Option<u32> {
    // SAFETY: The foreground owns the scheduler clock latch.
    unsafe { radio::clock() }
}

#[cfg(feature = "ble-adv-cold")]
pub(super) fn initialize() -> Result<(), u32> {
    // SAFETY: Reset has initialized the SRAM commands. This foreground owns
    // the FR8029D radio and QSPI during the boot-time trim read.
    unsafe {
        radio::initialize::<
            { cfg!(feature = "ble-gatt-xip") },
            { cfg!(all(feature = "ble-gatt-freertos", feature = "ble-gatt-xip")) },
        >()
    }
}

#[cfg(feature = "ble-gatt-board")]
pub(super) fn configure_baseband() -> Result<(), u32> {
    // SAFETY: The radio owner discards all old descriptors after reset.
    unsafe { radio::configure_baseband() }
}

#[cfg(not(feature = "ble-adv-cold"))]
pub(super) fn pending_slot() -> u32 {
    // SAFETY: The standalone foreground reads the exclusive scheduler slot.
    unsafe { radio::pending_slot() }
}

#[cfg(not(feature = "ble-gatt-board"))]
pub(super) fn clear_completed_event() {
    // SAFETY: The standalone foreground leaves no outstanding completion.
    unsafe { radio::clear_completed_event() };
}

pub(super) fn take_event_end() -> Option<u32> {
    // SAFETY: Only the foreground acknowledges an event it owns.
    unsafe { radio::take_event_end() }
}

#[cfg(feature = "ble-adv-cold")]
pub(super) fn scheduler_fault() -> Option<u32> {
    // SAFETY: The exclusive radio owner checks the scheduler.
    unsafe { radio::scheduler_fault() }
}

#[cfg(feature = "ble-connect-probe")]
pub(super) fn submit_slot(slot: u32, words: &[u16; 8]) {
    // SAFETY: The radio owner wraps the slot modulo 16 and submits only when idle.
    unsafe { radio::submit_slot(slot, words) };
}

#[cfg(not(feature = "ble-gatt-board"))]
pub(super) fn arm_slot(slot: u32) {
    // SAFETY: The standalone probe has initialized this slot and wraps it modulo 16.
    unsafe { radio::arm_slot(slot) };
}

#[cfg(feature = "ble-connect-probe")]
pub(super) fn set_rx_queue(offset: u16) {
    // SAFETY: The receiver installs its new descriptors while the scheduler is idle.
    unsafe { radio::set_rx_queue(offset) };
}

pub(super) fn write_record(base: usize, words: &[u16]) {
    // SAFETY: Callers use aligned, nonoverlapping packet RAM records they own.
    unsafe { radio::write_record(base, words) };
}

#[cfg(feature = "ble-connect-probe")]
pub(super) fn packet_byte(address: usize) -> u8 {
    // SAFETY: The receiver validates the descriptor's packet RAM buffer.
    unsafe { radio::packet_byte(address) }
}

pub(super) fn set_packet_byte(address: usize, byte: u8) {
    // SAFETY: All callers fill an owned packet buffer before submitting it.
    unsafe { radio::set_packet_byte(address, byte) };
}

pub(super) fn packet_halfword(address: usize) -> u16 {
    // SAFETY: Probes use aligned fields of installed packet RAM records.
    unsafe { radio::packet_halfword(address) }
}

#[cfg(feature = "ble-connect-probe")]
pub(super) fn set_packet_halfword(address: usize, value: u16) {
    // SAFETY: Probes write aligned fields on CPU-owned descriptors and records.
    unsafe { radio::set_packet_halfword(address, value) };
}

#[cfg(feature = "ble-connect-probe")]
pub(super) fn packet_word(address: usize) -> u32 {
    // SAFETY: The receiver reads aligned timestamp words in owned descriptors.
    unsafe { radio::packet_word(address) }
}

#[cfg(feature = "ble-gatt-board")]
pub(super) fn set_phy(control: u16, rates: u8) {
    // SAFETY: The link owns this connection record and applies the rates at the instant.
    unsafe { radio::set_phy(control, rates) };
}
