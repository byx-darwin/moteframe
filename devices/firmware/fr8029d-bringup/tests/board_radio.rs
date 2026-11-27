//! Exercises the board transport with a scripted baseband register boundary.
#![cfg(feature = "ble-gatt-probe")]

use ble_controller::{
    Advertisement, AdvertisingData, AdvertisingParameters, LinkBackend, RandomAddress,
};
use fr8029d_bringup::advertising::status_slot;
use fr8029d_bringup::connection::PacketTime;
use std::cell::{Cell, RefCell};

#[path = "../src/connect_probe.rs"]
mod connect_probe;
#[allow(
    dead_code,
    reason = "The standalone link probe is not part of the GATT backend"
)]
#[path = "../src/connect_radio.rs"]
mod connect_radio;
#[path = "../src/gatt_board_radio.rs"]
mod gatt_board_radio;

const CLOCK_MASK: u32 = fr8029d_bringup::advertising::CLOCK_MASK;
const SLOT_LEAD_TICKS: u32 = 12;
const PACKET_RAM: usize = 0x4001_0000;
const SUBMIT: usize = 0x4000_0110;
const IRQ_STATUS: usize = 0x4000_001c;
const IRQ_ACK: usize = 0x4000_0020;
const EVENT_STATUS: usize = 0x4000_0024;
const IRQ_EVENT_END: u32 = 1 << 15;

thread_local! {
    static RAM: RefCell<[u8; 0x4000]> = const { RefCell::new([0; 0x4000]) };
    static EVENT_CLOCK: Cell<Option<u32>> = const { Cell::new(None) };
    static COMPLETED_SLOT: Cell<Option<u32>> = const { Cell::new(None) };
    static SUBMITTED_SLOTS: Cell<u32> = const { Cell::new(0) };
    static DIAGNOSTICS: Cell<[u32; 64]> = const { Cell::new([0; 64]) };
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "Matches the hardware clock interface"
)]
fn clock() -> Option<u32> {
    Some(EVENT_CLOCK.get().unwrap_or(0))
}

fn read32(address: usize) -> u32 {
    if address >= PACKET_RAM {
        return u32::from(test_halfword(address)) | (u32::from(test_halfword(address + 2)) << 16);
    }
    if EVENT_CLOCK.get().is_some() {
        return match (address, COMPLETED_SLOT.get()) {
            (IRQ_STATUS, Some(_)) => IRQ_EVENT_END,
            (EVENT_STATUS, Some(slot)) => 2 | (slot << 24),
            _ => 0,
        };
    }
    // A slot with no completion still owns the scheduler until reset.
    u32::from(address == 0x4000_0010 && SUBMITTED_SLOTS.get() > 1) << 16
}

fn write32(address: usize, _: u32) {
    if address == IRQ_ACK {
        COMPLETED_SLOT.set(None);
    }
    if address == SUBMIT {
        SUBMITTED_SLOTS.set(SUBMITTED_SLOTS.get() + 1);
    }
}
fn write8(address: usize, value: u8) {
    RAM.with_borrow_mut(|ram| ram[address - PACKET_RAM] = value);
}
fn read8(address: usize) -> u8 {
    RAM.with_borrow(|ram| ram[address - PACKET_RAM])
}
fn test_halfword(address: usize) -> u16 {
    u16::from_le_bytes([read8(address), read8(address + 1)])
}
fn test_set_halfword(address: usize, value: u16) {
    let [low, high] = value.to_le_bytes();
    write8(address, low);
    write8(address + 1, high);
}
fn write_record(address: usize, words: &[u16]) {
    for (i, word) in words.iter().enumerate() {
        test_set_halfword(address + i * 2, *word);
    }
}
fn diagnostic(index: usize, value: u32) {
    let mut words = DIAGNOSTICS.get();
    words[index] = value;
    DIAGNOSTICS.set(words);
}
fn increment(index: usize) {
    diagnostic(index, DIAGNOSTICS.get()[index].saturating_add(1));
}

fn exchange_slot(slot: u32) -> usize {
    PACKET_RAM + slot as usize * 16
}

#[allow(
    clippy::unnecessary_wraps,
    reason = "Mocks the fallible board radio interface"
)]
mod cold_radio {
    pub(super) fn init() -> Result<(), u32> {
        Ok(())
    }

    pub(super) fn configure_baseband() -> Result<(), u32> {
        super::SUBMITTED_SLOTS.set(0);
        Ok(())
    }
}

mod gatt_board_time {
    pub(super) fn rebase_clock(_: u32) {}
}

#[cfg(feature = "ble-gatt-event-driven")]
mod gatt_board_wake {
    use fr8029d_bringup::wake::RadioDeadline;

    pub(super) fn set_radio_deadline(_: Option<RadioDeadline>) {}
    pub(super) fn record_connection_lead(_: u32) {}
}

#[test]
fn board_offers_two_m_unless_explicitly_fixed_to_one_m() {
    assert_eq!(
        gatt_board_radio::Radio::new().supports_two_m(),
        !cfg!(feature = "ble-gatt-1m-only")
    );
}

#[test]
#[allow(clippy::unwrap_used, reason = "Unexpected radio fixture failure")]
fn reset_recovers_the_radio_after_an_advertising_slot_never_completes() {
    let mut radio = gatt_board_radio::Radio::new();
    let advertisement = Advertisement::new(
        RandomAddress::new(gatt_board_radio::GATT_ADDRESS).unwrap(),
        AdvertisingParameters::new(160, 160, 7).unwrap(),
        AdvertisingData::new(b"\x02\x01\x06\x0a\x09FR29-GAT2").unwrap(),
        AdvertisingData::new(&[]).unwrap(),
    );
    radio.reset().unwrap();
    radio.advertise(&advertisement).unwrap();
    assert!(radio.poll().unwrap().is_none());

    assert_eq!(radio.reset(), Ok(()));
    assert_eq!(radio.advertise(&advertisement), Ok(()));
    assert!(radio.poll().unwrap().is_none());
    assert!(radio.poll().unwrap().is_none());
}

#[test]
fn connect_request_diagnostics_distinguish_hardware_and_decoder_rejections() {
    let advertiser = gatt_board_radio::GATT_ADDRESS;
    let mut packet = connect_probe::Packet {
        header: 0x2285,
        status: 1,
        time: PacketTime {
            coarse: 50,
            fine_countdown: 400,
        },
        link_id: 0,
        payload: [0; 34],
    };
    packet.payload[6..12].copy_from_slice(&advertiser);
    packet.payload[19] = 3;
    packet.payload[22] = 24;
    packet.payload[26] = 72;
    packet.payload[28] = 3;
    packet.payload[33] = 5;

    assert!(connect_probe::Receiver::inspect_advertisement(&packet, advertiser).is_none());
    assert_eq!(&DIAGNOSTICS.get()[36..39], &[1, 0, 1]);
    packet.status = 0;
    packet.payload[6] ^= 1;
    assert!(connect_probe::Receiver::inspect_advertisement(&packet, advertiser).is_none());
    assert_eq!(&DIAGNOSTICS.get()[36..39], &[1, 1, 0x8000_0002]);
    packet.payload[6] ^= 1;
    assert!(connect_probe::Receiver::inspect_advertisement(&packet, advertiser).is_some());
    assert_eq!(DIAGNOSTICS.get()[17], 1);
}

#[cfg(all(feature = "ble-gatt-mac-probe", not(feature = "ble-gatt-freertos")))]
#[test]
fn bare_mac_probe_uses_the_full_legacy_data_payload() {
    assert_eq!(gatt_board_radio::Radio::new().max_acl_fragment_len(), 27);
}

#[cfg(feature = "ble-gatt-dle")]
#[test]
fn board_offers_full_length_only_when_enabled_and_keeps_rx_queue_compact() {
    assert!(gatt_board_radio::Radio::new().supports_data_length());
    assert_eq!(gatt_board_radio::Radio::new().max_acl_fragment_len(), 27);
    assert!(size_of::<gatt_board_radio::Radio>() < 1024);
}

#[cfg(all(
    feature = "mic1-acquisition",
    feature = "ble-gatt-freertos",
    feature = "ble-gatt-xip",
    not(feature = "ble-gatt-dle")
))]
#[test]
fn short_mic1_rtos_xip_uses_the_sram_trial_link_profile() {
    assert_eq!(gatt_board_radio::Radio::new().max_acl_fragment_len(), 15);
    assert_eq!(
        gatt_board_radio::Radio::new().supports_two_m(),
        !cfg!(feature = "ble-gatt-1m-only")
    );
}
