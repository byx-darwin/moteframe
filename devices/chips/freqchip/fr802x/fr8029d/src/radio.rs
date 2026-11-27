//! FR8029D modem, baseband and packet RAM.

pub mod records;
pub mod trim;

/// Base address of the 16 KiB baseband packet RAM window.
pub const PACKET_RAM: usize = 0x4001_0000;
/// Bytes in each exchange scheduler slot.
pub const SLOT_STRIDE: usize = 16;
/// Maximum value of the scheduler's coarse clock.
pub const CLOCK_MASK: u32 = 0x0fff_ffff;
/// Baseband interrupt status.
pub const IRQ_STATUS: usize = 0x4000_001c;
/// End-of-event interrupt bit.
pub const IRQ_EVENT_END: u32 = 1 << 15;

/// Merge the link's RX and TX rate bits while preserving baseband fields.
#[must_use]
pub const fn phy_control_word(current: u16, rates: u8) -> u16 {
    (current & !15) | (rates as u16 & 15)
}

#[cfg(all(feature = "radio", target_arch = "arm"))]
mod hardware;

#[cfg(all(feature = "radio", target_arch = "arm"))]
pub use hardware::{
    arm_slot, clear_completed_event, clock, clock_snapshot, configure_baseband, event_end_pending,
    initialize, packet_byte, packet_halfword, packet_word, pending_slot, scheduler_fault,
    set_packet_byte, set_packet_halfword, set_phy, set_rx_queue, submit_slot, take_event_end,
    write_record,
};

#[cfg(test)]
mod tests {
    use super::phy_control_word;

    #[test]
    fn applies_each_phy_direction_without_changing_the_rest_of_the_control_word() {
        for rates in [0, 1, 4, 5] {
            assert_eq!(phy_control_word(0xdef5, rates), 0xdef0 | u16::from(rates));
        }
    }
}
