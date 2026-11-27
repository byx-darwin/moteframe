//! FR8029D legacy advertising and connected LE 1M/2M transport.

use super::{
    CLOCK_MASK, PACKET_RAM, SLOT_LEAD_TICKS, clock,
    connect_probe::Receiver,
    connect_radio::{
        CONNECTED_RX_ERRORS, CONNECTION_CONTROL, EVENT_COMPLETION_TIMEOUT, LINK_ID, TX_DONE,
        prepare_link, read_end, submit, submit_link, wait_connection_event,
    },
    diagnostic, increment,
    radio_hardware::{
        configure_baseband, initialize, packet_halfword as halfword, set_packet_byte as write8,
        set_packet_halfword as set_halfword, set_phy,
    },
    write_record,
};
use ble_controller::{
    Advertisement, ConnectionParameters, FrameKind, LinkBackend, LinkEvent, LinkFrame, OwnedQueue,
    Peer, TxAdmission,
};
use core::sync::atomic::{Ordering, fence};
use fr8029d_bringup::{
    advertising::{
        CONTROL_OFFSET, RX_COUNT, SCAN_TX_OFFSET, TX_OFFSET, TX_PAYLOAD_OFFSET, advance_clock,
        advertising_slot, connectable_control,
    },
    connection::{ConnectionEvent, ConnectionRequest, ConnectionUpdate, PacketTime},
    phy::LinkPhy,
    wake::SlotWatchdog,
};
#[cfg(feature = "ble-gatt-event-driven")]
use {super::gatt_board_wake, fr8029d_bringup::wake::RadioDeadline};

#[cfg(feature = "ble-gatt-dle")]
use {
    super::connect_probe::{HeldRx, RxBuffer},
    ble_controller::{DataLength, LengthResponseStatus},
    core::sync::atomic::AtomicU32,
    fr8029d_bringup::connection::DataAirtime,
};

// SWD bench words for one session: effective TX/RX octets, TX/RX times,
// largest new RX descriptor, largest committed and largest ACKed TX lengths.
#[cfg(feature = "ble-gatt-dle")]
#[unsafe(no_mangle)]
static FR29_DLE_OBSERVATIONS: [AtomicU32; 5] = [const { AtomicU32::new(0) }; 5];

#[cfg(feature = "ble-gatt-dle")]
fn record_limits(limits: DataLength) {
    let pair = |tx, rx| u32::from(tx) | u32::from(rx) << 16;
    FR29_DLE_OBSERVATIONS[0].store(pair(limits.tx_octets, limits.rx_octets), Ordering::Relaxed);
    FR29_DLE_OBSERVATIONS[1].store(pair(limits.tx_time, limits.rx_time), Ordering::Relaxed);
}

#[cfg(feature = "ble-gatt-dle")]
#[derive(Debug)]
enum BoardEvent {
    Connected(Peer, ConnectionParameters),
    Received {
        kind: FrameKind,
        len: u8,
        buffer: RxBuffer,
        session: u32,
        rx_two_m: bool,
        length_response: bool,
    },
    Acknowledged(u32),
    Disconnected(u8),
    ParametersUpdated(ConnectionParameters),
    PhyUpdated(u8, u8),
}

#[cfg(not(feature = "ble-gatt-dle"))]
use ble_controller::LinkEvent as BoardEvent;

pub(super) const ADAPTIVE_PHY: bool = !cfg!(feature = "ble-gatt-1m-only");
const HALF_CLOCK: u32 = CLOCK_MASK.div_ceil(2);
const CONTROL: usize = PACKET_RAM + CONNECTION_CONTROL as usize;
const TX_HEAD_REGISTER: usize = CONTROL + 0x24;
const LEGACY_ADV_LEN: usize = 14;
const NOTIFY_ACK_SESSION: usize = 33;
const NOTIFY_ACK_LOW: usize = 34;
const NOTIFY_ACK_HIGH: usize = 35;
// Maintenance and ordinary applications expose different GATT databases.
// Keep their probe identities distinct so centrals rediscover the OTA characteristic.
const ADDRESS_SUFFIX: u8 = if cfg!(feature = "ble-gatt-ota") {
    if cfg!(feature = "ble-gatt-freertos") {
        0xc6
    } else {
        0xc5
    }
} else if cfg!(feature = "ble-gatt-freertos") {
    0xc2
} else if cfg!(feature = "ble-gatt-xip") {
    0xc1
} else {
    0xc0
};
pub(super) const GATT_ADDRESS: [u8; 6] = [ADDRESS_SUFFIX, 0x80, 0x29, 0x01, 0x25, 0xc3];

const RX_FRAMES: usize = 48;
const TX_COMMITS: usize = 49;
const TX_ACKS: usize = 50;
const UPDATE_REQUESTS: usize = 51;
const UPDATES_APPLIED: usize = 52;
const ATT_RX: usize = 53;
const ATT_TX: usize = 54;
const LAST_ATT_RX: usize = 55;
const LAST_ATT_TX: usize = 56;
const RADIO_TIMEOUTS: usize = 61;
const NOTIFY_ACK_TOKEN: usize = 62;
const RX_REPEATED: u16 = 0x0040;

fn received_pdu(status: u16, header: u16) -> Option<(FrameKind, bool)> {
    // Repeated packets still carry the central's event timestamp. Exclude
    // the payload separately so it is never delivered twice to the Host.
    if status & (CONNECTED_RX_ERRORS & !RX_REPEATED) != 0 {
        return None;
    }
    let kind = match header & 3 {
        1 => FrameKind::Continuation,
        2 => FrameKind::Start,
        3 => FrameKind::Control,
        _ => return None,
    };
    Some((kind, status & RX_REPEATED == 0))
}

#[cfg(feature = "ble-gatt-dle")]
fn is_length_response(packet: &HeldRx) -> bool {
    let len = usize::from(packet.header >> 8);
    packet.link_id == u16::from(LINK_ID)
        && received_pdu(packet.status, packet.header) == Some((FrameKind::Control, true))
        && len >= 2
        && DataLength::is_response_header(packet.payload.byte(0), packet.payload.byte(1), len)
}

#[cfg(test)]
mod tests {
    use super::{FrameKind, received_pdu};

    #[cfg(all(feature = "ble-gatt-dle", not(target_arch = "arm")))]
    fn advance_three_slots_with_full_ring(radio: &mut super::Radio) -> Result<(), u32> {
        use super::super::{COMPLETED_SLOT, EVENT_CLOCK, SUBMITTED_SLOTS};
        use super::*;
        radio.initialized = true;
        for _ in 0..3 {
            let link = radio.link.as_ref().ok_or(21_u32)?;
            let counter = link.counter;
            let start = link.event.start.coarse;
            EVENT_CLOCK.set(Some(start.wrapping_sub(3) & CLOCK_MASK));
            let previous = SUBMITTED_SLOTS.get();
            radio.service_backpressure()?;
            assert_eq!(SUBMITTED_SLOTS.get(), previous + 1);
            assert!(radio.pending.is_some());
            COMPLETED_SLOT.set(Some(radio.slot));
            EVENT_CLOCK.set(Some(start));
            radio.service_backpressure()?;
            assert!(radio.pending.is_none());
            assert_eq!(radio.link.as_ref().ok_or(21_u32)?.counter, counter + 1);
            assert_eq!(radio.events.len(), RX_COUNT);
        }
        EVENT_CLOCK.set(None);
        Ok(())
    }

    fn connected_fixture() -> Result<super::Radio, u32> {
        use super::*;
        let mut radio = Radio::new();
        radio.receiver = Some(Receiver::start());
        let request = ConnectionRequest {
            initiator: [1; 6],
            initiator_random: true,
            access_address: 0x1234_5678,
            crc_init: 0x00ab_cdef,
            window_size: 1,
            window_offset: 0,
            interval: 24,
            latency: 0,
            supervision_timeout: 500,
            channel_map: [255, 255, 255, 255, 31],
            hop: 8,
            sca: 7,
        };
        radio.link = Some(Link::start(
            request,
            PacketTime {
                coarse: 0,
                fine_countdown: 624,
            },
        )?);
        Ok(radio)
    }

    #[cfg(feature = "ble-gatt-dle")]
    fn ready_rx(index: usize, kind: u8, status: u16, coarse: u32, payload: &[u8]) {
        use super::*;
        use fr8029d_bringup::advertising::{RX_OFFSET, RX_WORDS};
        let rx = PACKET_RAM + usize::from(RX_OFFSET) + index * RX_WORDS * 2;
        let offset = usize::from(halfword(rx + 0x14));
        for (i, &byte) in payload.iter().enumerate() {
            write8(PACKET_RAM + offset + i, byte);
        }
        set_halfword(rx + 2, status);
        set_halfword(
            rx + 4,
            u16::from_le_bytes([kind, payload.len().to_le_bytes()[0]]),
        );
        for (i, byte) in coarse.to_le_bytes().into_iter().enumerate() {
            write8(rx + 8 + i, byte);
        }
        set_halfword(rx + 0xc, (u16::from(LINK_ID) << 11) | 0x0190);
        set_halfword(rx, halfword(rx) | 0x8000);
    }

    #[cfg(all(feature = "ble-gatt-dle", not(target_arch = "arm")))]
    #[test]
    fn length_response_clock_follows_the_first_ready_rx_after_commit() -> Result<(), u32> {
        use super::super::{COMPLETED_SLOT, EVENT_CLOCK};
        use super::*;
        let mut radio = connected_fixture()?;
        let request = [0x14, 251, 0, 0x48, 8, 251, 0, 0x48, 8];
        let response = [0x15, 251, 0, 0x48, 8, 251, 0, 0x48, 8];
        // A response already held by the FIFO cannot complete a new request.
        ready_rx(0, 3, 0, 127_900, &response);
        radio.connection_ended(127_900)?;
        assert!(matches!(radio.poll(), Ok(Some(LinkEvent::Connected(..)))));
        assert_eq!(
            radio.length_response_status()?,
            LengthResponseStatus::Pending
        );
        let transmit = |radio: &mut Radio, token| {
            radio.transmit_payload(token, FrameKind::Control, false, &request)
        };
        assert_eq!(transmit(&mut radio, 1), Ok(TxAdmission::Busy));
        assert!(matches!(radio.poll(), Ok(Some(LinkEvent::Received(_)))));
        assert_eq!(transmit(&mut radio, 1), Ok(TxAdmission::Committed));
        radio.initialized = true;
        let start = radio.link.as_ref().ok_or(21_u32)?.event.start.coarse;
        EVENT_CLOCK.set(Some(start - 3));
        radio.service_backpressure()?;
        assert!(radio.pending.is_some());
        ready_rx(1, 3, RX_REPEATED, 127_989, &response);
        ready_rx(2, 2, 0, 127_990, &response);
        ready_rx(3, 3, 0, 127_991, &[0x11, 0x16, 0x1a]);
        assert_eq!(
            radio.length_response_status()?,
            LengthResponseStatus::Pending
        );
        ready_rx(4, 3, 0, 127_999, &[0x07, 0x14]);
        ready_rx(5, 3, 0, 128_001, &response);
        // Poll the deadline before EVENT_END while RX_READY gives the CPU ownership.
        EVENT_CLOCK.set(Some(128_002));
        radio.service_backpressure()?;
        assert!(radio.pending.is_some());
        assert_eq!(radio.data_length_clock(), Some(128_002));
        assert_eq!(
            radio.length_response_status()?,
            LengthResponseStatus::Received(127_999)
        );
        COMPLETED_SLOT.set(Some(radio.slot));
        radio.service_backpressure()?;
        assert!(radio.pending.is_none());
        assert_eq!(transmit(&mut radio, 2), Ok(TxAdmission::Busy));
        assert_eq!(
            radio.length_response_status()?,
            LengthResponseStatus::Received(127_999)
        );
        while !radio.events.is_empty() {
            assert!(radio.poll()?.is_some());
        }
        assert_eq!(
            radio.length_response_status()?,
            LengthResponseStatus::Received(127_999)
        );

        let tx = PACKET_RAM + usize::from(TX_OFFSET);
        set_halfword(tx, halfword(tx) | TX_DONE);
        radio.connection_ended(128_005)?;
        assert_eq!(transmit(&mut radio, 2), Ok(TxAdmission::Committed));
        assert_eq!(
            radio.length_response_status()?,
            LengthResponseStatus::Pending
        );
        ready_rx(0, 3, 0, 128_010, &[0x0d, 0x1a]);
        radio.connection_ended(128_012)?;
        assert_eq!(
            radio.length_response_status()?,
            LengthResponseStatus::Received(128_010)
        );
        radio.stop()?;
        assert_eq!(
            radio.length_response_status()?,
            LengthResponseStatus::Pending
        );
        EVENT_CLOCK.set(None);
        Ok(())
    }

    #[cfg(all(feature = "ble-gatt-dle", not(target_arch = "arm")))]
    #[test]
    fn length_deadline_surfaces_invalid_ready_descriptor_offset() -> Result<(), u32> {
        use super::super::EVENT_CLOCK;
        use super::*;
        use fr8029d_bringup::advertising::RX_OFFSET;
        let mut radio = connected_fixture()?;
        let link = radio.link.as_mut().ok_or(21_u32)?;
        link.length_response = LengthResponse::Waiting;
        radio.initialized = true;
        EVENT_CLOCK.set(Some(link.event.start.coarse - 3));
        radio.service_backpressure()?;
        EVENT_CLOCK.set(None);
        assert!(radio.pending.is_some());
        ready_rx(0, 3, 0, 127_999, &[0x07, 0x14]);
        let rx = PACKET_RAM + usize::from(RX_OFFSET);
        set_halfword(rx + 0x14, 0xffff);
        assert_eq!(radio.length_response_status(), Err(17));
        Ok(())
    }

    #[test]
    fn late_unsubmitted_slots_advance_csa1_across_counter_and_channel_wrap() -> Result<(), u32> {
        use super::*;

        let mut radio = connected_fixture()?;
        let link = radio.link.as_mut().ok_or(21_u32)?;
        link.anchor = Some(link.request_time);
        link.counter = u16::MAX;
        let hop_state = CONTROL + 0x18;
        set_halfword(hop_state, 0x8800 | 32);
        for (counter, channel) in [(0, 3), (1, 11)] {
            let start = radio.link.as_ref().ok_or(21_u32)?.event.start.coarse;
            radio.schedule(start)?;
            assert!(radio.pending.is_none());
            assert_eq!(radio.link.as_ref().ok_or(21_u32)?.counter, counter);
            assert_eq!(halfword(hop_state), 0x8800 | channel);
        }
        let start = radio.link.as_ref().ok_or(21_u32)?.event.start.coarse;
        let now = start.wrapping_sub(2) & CLOCK_MASK;
        super::super::EVENT_CLOCK.set(Some(now));
        radio.schedule(now)?;
        assert!(radio.pending.is_some());
        // The baseband performs the next hop itself when this slot executes.
        assert_eq!(halfword(hop_state), 0x880b);
        Ok(())
    }

    #[cfg(all(
        feature = "mic1-acquisition",
        feature = "ble-gatt-xip",
        not(feature = "ble-gatt-dle")
    ))]
    #[test]
    fn preempted_slot_skips_instead_of_starting_a_completion_watchdog() -> Result<(), u32> {
        use super::super::{EVENT_CLOCK, SUBMITTED_SLOTS};
        use super::*;
        let mut radio = connected_fixture()?;
        let link = radio.link.as_mut().ok_or(21_u32)?;
        link.anchor = Some(link.request_time);
        for lag in [1_u32, 0, u32::MAX] {
            let start = radio.link.as_ref().ok_or(21_u32)?.event.start.coarse;
            EVENT_CLOCK.set(Some(start.wrapping_sub(lag) & CLOCK_MASK));
            radio.schedule(start.wrapping_sub(SLOT_LEAD_TICKS) & CLOCK_MASK)?;
            assert!(radio.pending.is_none());
        }
        assert_eq!(SUBMITTED_SLOTS.get(), 0);
        assert_eq!(radio.link.as_ref().ok_or(21_u32)?.counter, 3);
        let start = radio.link.as_ref().ok_or(21_u32)?.event.start.coarse;
        EVENT_CLOCK.set(Some(start.wrapping_sub(2) & CLOCK_MASK));
        radio.schedule(start.wrapping_sub(SLOT_LEAD_TICKS) & CLOCK_MASK)?;
        assert_eq!(SUBMITTED_SLOTS.get(), 1);
        assert!(radio.pending.is_some());
        Ok(())
    }

    #[cfg(feature = "ble-gatt-dle")]
    #[test]
    fn supervision_timeout_retires_held_descriptors_before_new_session() -> Result<(), u32> {
        use super::*;
        use fr8029d_bringup::advertising::RX_OFFSET;
        let mut radio = connected_fixture()?;
        let rx = PACKET_RAM + usize::from(RX_OFFSET);
        set_halfword(rx + 4, u16::from_le_bytes([2, 4]));
        set_halfword(rx + 0xc, u16::from(LINK_ID) << 11);
        set_halfword(rx, halfword(rx) | 0x8000);
        radio.connection_ended(0)?;
        assert!(matches!(radio.poll(), Ok(Some(LinkEvent::Connected(..)))));
        assert_eq!(radio.events.len(), 1);
        radio.expire_link(32 * 501)?;
        assert_eq!(
            radio.length_response_status()?,
            LengthResponseStatus::Disconnected(0x08)
        );
        let frame = LinkFrame::new(FrameKind::Start, &[0, 0, 4, 0]).map_err(|_| 20_u32)?;
        assert_eq!(radio.transmit(1, frame), Ok(TxAdmission::Busy));
        assert_eq!(radio.poll(), Ok(Some(LinkEvent::Disconnected(0x08))));
        assert!(radio.events.is_empty());
        let mut receiver = Receiver::start();
        assert!(receiver.take_link()?.is_none());
        Ok(())
    }

    #[cfg(all(feature = "ble-gatt-dle", not(feature = "ble-gatt-1m-only")))]
    #[test]
    fn queued_rx_uses_the_phy_of_its_receive_event() -> Result<(), u32> {
        use super::*;
        use fr8029d_bringup::advertising::RX_OFFSET;
        for received_two_m in [true, false] {
            let mut radio = connected_fixture()?;
            let link = radio.link.as_mut().ok_or(21_u32)?;
            if received_two_m {
                assert!(link.phy.schedule(0, 0, 2, 0));
                assert!(link.phy.apply(0).is_some());
            }
            radio.data_length_updated(DataLength {
                rx_time: 1100,
                ..DataLength::MAX
            });
            let rx = PACKET_RAM + usize::from(RX_OFFSET);
            let payload = core::array::from_fn::<_, 251, _>(|i| i.to_le_bytes()[0]);
            let offset = usize::from(halfword(rx + 0x14));
            for (i, byte) in payload.iter().enumerate() {
                write8(PACKET_RAM + offset + i, *byte);
            }
            set_halfword(rx + 4, u16::from_le_bytes([2, 251]));
            set_halfword(rx + 0xc, u16::from(LINK_ID) << 11);
            set_halfword(rx, halfword(rx) | 0x8000);
            radio.connection_ended(0)?;
            assert!(matches!(radio.poll(), Ok(Some(LinkEvent::Connected(..)))));

            // Apply a new PHY at the next instant while the first RX is still queued.
            let link = radio.link.as_mut().ok_or(21_u32)?;
            let next_rx = if received_two_m { 1 } else { 2 };
            assert!(link.phy.schedule(link.counter, link.counter, next_rx, 0));
            let now = link.event.start.coarse.wrapping_sub(SLOT_LEAD_TICKS + 1) & CLOCK_MASK;
            radio.schedule(now)?;
            if received_two_m {
                let Some(LinkEvent::Received(frame)) = radio.poll()? else {
                    panic!("missing queued RX")
                };
                assert_eq!(frame.payload(), payload);
            } else {
                // A faster later PHY cannot make a packet fit its original RX window.
                assert_eq!(radio.poll(), Err(20));
            }
            assert_eq!(halfword(rx) & 0x8000, 0);
        }
        Ok(())
    }

    #[cfg(feature = "ble-gatt-dle")]
    #[test]
    fn full_rx_ring_and_single_tx_descriptor_preserve_every_byte_until_consumed() -> Result<(), u32>
    {
        use super::*;
        use fr8029d_bringup::advertising::{RX_OFFSET, RX_WORDS};
        let mut radio = connected_fixture()?;
        let descriptor = |i: usize| PACKET_RAM + usize::from(RX_OFFSET) + i * RX_WORDS * 2;
        let payload = core::array::from_fn::<_, 251, _>(|i| i.to_le_bytes()[0] ^ 0xab);
        let lengths = [27, 28, 250, 251, 28, 251];
        for (i, len) in lengths.into_iter().enumerate() {
            let rx = descriptor(i);
            let offset = usize::from(halfword(rx + 0x14));
            for (j, byte) in payload[..len].iter().copied().enumerate() {
                write8(PACKET_RAM + offset + j, byte);
            }
            set_halfword(rx + 4, u16::from_le_bytes([2, len.to_le_bytes()[0]]));
            set_halfword(rx + 0xc, (u16::from(LINK_ID) << 11) | 0x0190);
            set_halfword(rx, halfword(rx) | 0x8000);
        }
        radio.connection_ended(0)?;
        assert!(matches!(radio.poll(), Ok(Some(LinkEvent::Connected(..)))));
        // Simulate another connection event before the Host consumes any ACL.
        // The six completed descriptors remain CPU-owned across that event.
        #[cfg(not(target_arch = "arm"))]
        advance_three_slots_with_full_ring(&mut radio)?;
        // The first short packet can be delivered before a length update; the
        // controller may change the limits between observations of the same event.
        for (i, len) in lengths.into_iter().enumerate() {
            if i == 1 {
                radio.data_length_updated(DataLength::MAX);
            }
            assert_ne!(halfword(descriptor(i)) & 0x8000, 0);
            let Some(LinkEvent::Received(frame)) = radio.poll()? else {
                panic!("missing RX frame")
            };
            assert_eq!(frame.payload(), &payload[..len]);
            assert_eq!(halfword(descriptor(i)) & 0x8000, 0);
        }

        let frame = LinkFrame::new(FrameKind::Start, &payload).map_err(|_| 20_u32)?;
        assert_eq!(
            radio.transmit_payload(5, frame.kind(), false, frame.payload()),
            Ok(TxAdmission::Committed)
        );
        let other = LinkFrame::new(FrameKind::Start, &[99; 28]).map_err(|_| 20_u32)?;
        assert_eq!(radio.transmit(6, other), Ok(TxAdmission::Busy));
        let tx = PACKET_RAM + usize::from(TX_OFFSET);
        assert_eq!(halfword(tx + 2), 0xfb02);
        for (i, byte) in payload.iter().copied().enumerate() {
            assert_eq!(
                super::super::read8(PACKET_RAM + usize::from(TX_PAYLOAD_OFFSET) + i),
                byte
            );
        }
        radio.connection_ended(0)?;
        assert!(radio.events.is_empty());
        radio.data_length_updated(DataLength::LEGACY);
        let link = radio.link.as_ref().ok_or(21_u32)?;
        assert_eq!(link.tx_airtime().octets(), 27);
        assert_eq!(link.slot_tx_airtime().octets(), 251);
        radio.data_length_updated(DataLength::MAX);
        set_halfword(tx, halfword(tx) | TX_DONE);
        radio.connection_ended(0)?;
        assert_eq!(radio.poll(), Ok(Some(LinkEvent::Acknowledged(5))));
        assert_eq!(FR29_DLE_OBSERVATIONS[4].load(Ordering::Relaxed), 251);

        // A repeat updates timing but returns its slot immediately. The second
        // descriptor in this event is new and remains held until polled.
        for i in 0..2 {
            let rx = descriptor(i);
            set_halfword(rx + 2, if i == 0 { RX_REPEATED } else { 0 });
            set_halfword(rx + 4, 0xfb02);
            set_halfword(rx, halfword(rx) | 0x8000);
        }
        radio.connection_ended(0)?;
        assert_eq!(halfword(descriptor(0)) & 0x8000, 0);
        assert_ne!(halfword(descriptor(1)) & 0x8000, 0);
        assert_eq!(radio.events.len(), 1);
        assert!(matches!(radio.poll(), Ok(Some(LinkEvent::Received(_)))));
        assert_eq!(halfword(descriptor(1)) & 0x8000, 0);

        radio.data_length_updated(DataLength::LEGACY);
        assert_eq!(radio.transmit(7, other), Err(20));
        radio.data_length_updated(DataLength::MAX);
        let rx = descriptor(2);
        set_halfword(rx + 4, 0xfc02);
        set_halfword(rx, halfword(rx) | 0x8000);
        radio.connection_ended(0)?;
        assert_eq!(radio.poll(), Err(20));
        assert_eq!(halfword(rx) & 0x8000, 0);
        Ok(())
    }

    #[test]
    fn notify_completion_follows_the_final_fragment() {
        let mut remaining = 0;
        let mut packet = [0u8; 27];
        packet[..7].copy_from_slice(&[23, 0, 4, 0, 0x1b, 9, 0]);
        assert!(!super::echo_fragment(
            &mut remaining,
            FrameKind::Start,
            &packet[..15]
        ));
        assert!(!super::echo_fragment(
            &mut remaining,
            FrameKind::Control,
            &[7, 0xff]
        ));
        assert!(super::echo_fragment(
            &mut remaining,
            FrameKind::Continuation,
            &packet[15..]
        ));
        assert!(!super::echo_fragment(
            &mut remaining,
            FrameKind::Continuation,
            &packet[15..]
        ));
        assert!(super::echo_fragment(
            &mut remaining,
            FrameKind::Start,
            &packet
        ));
    }

    #[test]
    fn repeated_data_can_track_the_anchor_but_is_never_delivered_twice() {
        assert_eq!(received_pdu(0x00, 0x0b02), Some((FrameKind::Start, true)));
        assert_eq!(received_pdu(0xc0, 0x0b02), Some((FrameKind::Start, false)));
        assert_eq!(received_pdu(0xc1, 0x0b02), None);
    }
}

#[derive(Debug)]
struct PendingTx {
    token: u32,
    offset: u16,
    is_echo: bool,
    #[cfg(feature = "ble-gatt-dle")]
    len: u8,
}

#[cfg(feature = "ble-gatt-dle")]
#[derive(Debug)]
enum LengthResponse {
    Idle,
    Waiting,
    Received(u32),
}

#[derive(Debug)]
struct Link {
    request: ConnectionRequest,
    request_time: PacketTime,
    event: ConnectionEvent,
    parameters: ConnectionParameters,
    counter: u16,
    anchor: Option<PacketTime>,
    missed: u16,
    tx: Option<PendingTx>,
    echo_remaining: u8,
    #[cfg(feature = "ble-gatt-dle")]
    limits: DataLength,
    #[cfg(feature = "ble-gatt-dle")]
    length_response: LengthResponse,
    #[cfg(feature = "ble-gatt-dle")]
    queued_length_responses: u8,
    #[cfg(feature = "ble-gatt-dle")]
    tx_two_m: bool,
    phy: LinkPhy,
    map_update: Option<(u16, [u8; 5])>,
    timing_update: Option<ConnectionUpdate>,
    reacquiring: bool,
    parameters_changed: bool,
}

impl Link {
    fn start(request: ConnectionRequest, request_time: PacketTime) -> Result<Self, u32> {
        let params = ConnectionParameters::new(
            request.interval,
            request.latency,
            request.supervision_timeout,
        )
        .map_err(|_| 20_u32)?;
        let event = request.first_event(request_time);
        prepare_link(request, event);
        #[cfg(feature = "ble-gatt-dle")]
        {
            record_limits(DataLength::LEGACY);
            for word in &FR29_DLE_OBSERVATIONS[2..] {
                word.store(0, Ordering::Relaxed);
            }
        }
        Ok(Self {
            request,
            request_time,
            event,
            parameters: params,
            counter: 0,
            anchor: None,
            missed: 0,
            tx: None,
            echo_remaining: 0,
            #[cfg(feature = "ble-gatt-dle")]
            limits: DataLength::LEGACY,
            #[cfg(feature = "ble-gatt-dle")]
            length_response: LengthResponse::Idle,
            #[cfg(feature = "ble-gatt-dle")]
            queued_length_responses: 0,
            #[cfg(feature = "ble-gatt-dle")]
            tx_two_m: false,
            phy: LinkPhy::default(),
            map_update: None,
            timing_update: None,
            reacquiring: false,
            parameters_changed: false,
        })
    }

    #[inline(never)]
    fn advance(&mut self, anchor: Option<PacketTime>) {
        self.counter = self.counter.wrapping_add(1);
        if let Some(anchor) = anchor {
            self.anchor = Some(anchor);
            self.missed = 0;
            self.reacquiring = false;
        } else {
            self.missed = self.missed.saturating_add(1);
        }
        self.event = if self.reacquiring {
            self.event.one_interval_later(self.request.interval)
        } else if let Some(anchor) = self.anchor {
            self.request.event_after(anchor, self.missed + 1)
        } else {
            self.request.first_event(PacketTime {
                coarse: advance_clock(
                    self.request_time.coarse,
                    4 * u32::from(self.request.interval) * u32::from(self.counter),
                ),
                fine_countdown: self.request_time.fine_countdown,
            })
        };
    }

    fn skip_event(&mut self) {
        let start = self.event.start.coarse;
        diagnostic(31, start);
        self.apply_map_at_instant();
        // No slot will execute this event's CSA1 hop. Move the previous
        // unmapped channel forward once, preserving the other link fields.
        let hop_state = halfword(CONTROL + 0x18);
        let next = ((hop_state & 0x3f) + u16::from(self.request.hop)) % 37;
        set_halfword(CONTROL + 0x18, (hop_state & !0x3f) | next);
        self.advance(None);
    }

    fn prepare_timing_update(&mut self) -> Result<(), u32> {
        if let Some(update) = self.timing_update
            && update.instant == self.counter
        {
            let anchor = self.anchor.ok_or(24_u32)?;
            self.event = update.event_at(
                anchor,
                self.request.interval,
                self.missed + 1,
                self.request.sca,
            );
            self.parameters = ConnectionParameters::new(
                update.interval,
                update.latency,
                update.supervision_timeout,
            )
            .map_err(|_| 24_u32)?;
            self.request.interval = update.interval;
            self.request.latency = update.latency;
            self.request.supervision_timeout = update.supervision_timeout;
            self.timing_update = None;
            self.reacquiring = true;
            self.parameters_changed = true;
        }
        Ok(())
    }

    #[cfg(feature = "ble-gatt-dle")]
    fn tx_airtime(&self) -> DataAirtime {
        DataAirtime::new(self.limits.tx_octets, self.limits.tx_time, self.tx_two_m)
    }

    #[cfg(feature = "ble-gatt-dle")]
    fn slot_tx_airtime(&self) -> DataAirtime {
        let limit = self.tx_airtime();
        // A changed length or PHY does not return an outstanding descriptor.
        // Reserve time for its existing payload until the peer ACKs it.
        if let Some(tx) = &self.tx
            && tx.len > limit.octets()
        {
            DataAirtime::new(u16::from(tx.len), u16::MAX, self.tx_two_m)
        } else {
            limit
        }
    }

    #[cfg(feature = "ble-gatt-dle")]
    fn rx_airtime(&self, two_m: bool) -> DataAirtime {
        DataAirtime::new(self.limits.rx_octets, self.limits.rx_time, two_m)
    }

    fn expired(&self, now: u32) -> bool {
        if let Some(anchor) = self.anchor {
            let elapsed = now.wrapping_sub(anchor.coarse) & CLOCK_MASK;
            (32 * u32::from(self.request.supervision_timeout)..HALF_CLOCK).contains(&elapsed)
        } else {
            self.counter >= 6
        }
    }

    #[inline(never)]
    fn apply_map_at_instant(&mut self) {
        if let Some((instant, map)) = self.map_update
            && instant == self.counter
        {
            for (index, bytes) in map[..4].chunks_exact(2).enumerate() {
                set_halfword(
                    CONTROL + 0x32 + index * 2,
                    u16::from_le_bytes([bytes[0], bytes[1]]),
                );
            }
            set_halfword(
                CONTROL + 0x36,
                (halfword(CONTROL + 0x36) & !0x1f) | u16::from(map[4]),
            );
            self.request.channel_map = map;
            self.map_update = None;
        }
    }
}

#[derive(Debug)]
pub(super) struct Radio {
    initialized: bool,
    receiver: Option<Receiver>,
    advertisement: Option<Advertisement>,
    link: Option<Link>,
    pending: Option<SlotWatchdog>,
    #[cfg(feature = "ble-gatt-dle")]
    last_clock: u32,
    next_advertisement: u32,
    slot: u32,
    jitter: u32,
    session: u32,
    #[cfg(feature = "ble-gatt-dle")]
    // Six held RX descriptors, plus connect, TX ACK, PHY and timing changes.
    events: OwnedQueue<BoardEvent, { RX_COUNT + 4 }>,
    #[cfg(not(feature = "ble-gatt-dle"))]
    events: OwnedQueue<BoardEvent, 8>,
}

impl Radio {
    pub(super) const fn new() -> Self {
        Self {
            initialized: false,
            receiver: None,
            advertisement: None,
            link: None,
            pending: None,
            #[cfg(feature = "ble-gatt-dle")]
            last_clock: 0,
            next_advertisement: 0,
            slot: 0,
            jitter: 1,
            session: 0,
            events: OwnedQueue::new(),
        }
    }

    #[cfg(feature = "ble-gatt-event-driven")]
    fn publish_deadline(&self) {
        let deadline = if let Some(slot) = self.pending {
            Some(slot.deadline())
        } else if let Some(link) = &self.link {
            Some(RadioDeadline::Connection {
                start: link.event.start.coarse,
                lead: SLOT_LEAD_TICKS,
            })
        } else {
            self.advertisement.map(|_| RadioDeadline::Advertising {
                at: self.next_advertisement,
            })
        };
        gatt_board_wake::set_radio_deadline(deadline);
    }

    fn start_advertising(&mut self, now: u32, advertisement: &Advertisement) {
        write_record(
            PACKET_RAM + usize::from(CONTROL_OFFSET),
            &connectable_control(GATT_ADDRESS),
        );
        for (index, &byte) in advertisement.data.as_slice().iter().enumerate() {
            write8(PACKET_RAM + usize::from(TX_PAYLOAD_OFFSET) + index, byte);
        }
        self.receiver = Some(Receiver::start());
        self.next_advertisement = now;
        #[cfg(feature = "ble-gatt-event-driven")]
        self.publish_deadline();
    }

    fn advertising_ended(&mut self) -> Result<(), u32> {
        if let Some((request, time)) = self.receiver.as_mut().ok_or(21_u32)?.drain(GATT_ADDRESS)? {
            self.link = Some(Link::start(request, time)?);
        }
        Ok(())
    }

    fn connection_ended(&mut self, now: u32) -> Result<(), u32> {
        let link = self.link.as_mut().ok_or(21_u32)?;
        if link.parameters_changed {
            self.events
                .push(BoardEvent::ParametersUpdated(link.parameters))
                .map_err(|_| 22_u32)?;
            link.parameters_changed = false;
            increment(UPDATES_APPLIED);
        }
        if let Some(tx) = &link.tx
            && halfword(PACKET_RAM + usize::from(tx.offset)) & TX_DONE != 0
        {
            self.events
                .push(BoardEvent::Acknowledged(tx.token))
                .map_err(|_| 22_u32)?;
            if tx.is_echo {
                diagnostic(NOTIFY_ACK_SESSION, self.session);
                diagnostic(NOTIFY_ACK_TOKEN, tx.token);
            }
            #[cfg(feature = "ble-gatt-dle")]
            FR29_DLE_OBSERVATIONS[4].fetch_max(u32::from(tx.len), Ordering::Relaxed);
            link.tx = None;
            increment(TX_ACKS);
        }
        let mut repeated = false;
        let receiver = self.receiver.as_mut().ok_or(21_u32)?;
        let mut anchor = None;
        for _ in 0..RX_COUNT {
            #[cfg(not(feature = "ble-gatt-dle"))]
            let packet = receiver.pop()?;
            #[cfg(feature = "ble-gatt-dle")]
            let packet = receiver.take_link()?;
            let Some(packet) = packet else { break };
            if packet.link_id != u16::from(LINK_ID) {
                #[cfg(feature = "ble-gatt-dle")]
                packet.payload.release(receiver);
                continue;
            }
            let Some((kind, new_payload)) = received_pdu(packet.status, packet.header) else {
                #[cfg(feature = "ble-gatt-dle")]
                packet.payload.release(receiver);
                continue;
            };
            if anchor.is_none() && link.anchor.is_none() {
                let peer = Peer::new(link.request.initiator, link.request.initiator_random);
                self.events
                    .push(BoardEvent::Connected(peer, link.parameters))
                    .map_err(|_| 22_u32)?;
                self.session = self.session.saturating_add(1);
            }
            repeated |= !new_payload;
            anchor.get_or_insert(
                packet
                    .time
                    .data_anchor_for_phy(ADAPTIVE_PHY && link.phy.rx_two_m()),
            );
            let length = packet.header.to_le_bytes()[1];
            if new_payload && length != 0 {
                #[cfg(not(feature = "ble-gatt-dle"))]
                let observation = {
                    let bytes = packet.payload.get(..usize::from(length)).ok_or(20_u32)?;
                    let frame = LinkFrame::new(kind, bytes).map_err(|_| 20_u32)?;
                    increment(RX_FRAMES);
                    if kind == FrameKind::Start {
                        record_att(bytes, ATT_RX, LAST_ATT_RX);
                    }
                    BoardEvent::Received(frame)
                };
                #[cfg(feature = "ble-gatt-dle")]
                let length_response = is_length_response(&packet);
                #[cfg(feature = "ble-gatt-dle")]
                let observation = BoardEvent::Received {
                    kind,
                    len: length,
                    buffer: packet.payload,
                    session: self.session,
                    rx_two_m: link.phy.rx_two_m(),
                    length_response,
                };
                self.events.push(observation).map_err(|_| 22_u32)?;
                #[cfg(feature = "ble-gatt-dle")]
                if length_response {
                    link.queued_length_responses += 1;
                    if matches!(link.length_response, LengthResponse::Waiting) {
                        link.length_response = LengthResponse::Received(packet.time.coarse);
                    }
                }
            } else {
                #[cfg(feature = "ble-gatt-dle")]
                packet.payload.release(receiver);
            }
        }
        if ADAPTIVE_PHY {
            link.phy
                .observe_event(anchor.is_none() || repeated || link.tx.is_some());
        }
        link.advance(anchor);
        self.expire_link(now)
    }

    fn commit_payload(
        &mut self,
        token: u32,
        kind: FrameKind,
        header: [u8; 2],
        payload: &[u8],
    ) -> Result<TxAdmission, u32> {
        #[cfg(feature = "ble-gatt-dle")]
        if self.link.is_none() && !self.events.is_empty() {
            // The disconnect is queued behind observations already sent to the Host.
            return Ok(TxAdmission::Busy);
        }
        let link = self.link.as_mut().ok_or(21_u32)?;
        if self.pending.is_some() || link.tx.is_some() {
            return Ok(TxAdmission::Busy);
        }
        #[cfg(feature = "ble-gatt-dle")]
        let length_request =
            kind == FrameKind::Control && matches!(payload, [0x14, ..]) && header[1] == 9;
        // Deliver all older responses before starting another local procedure.
        #[cfg(feature = "ble-gatt-dle")]
        if length_request && link.queued_length_responses != 0 {
            return Ok(TxAdmission::Busy);
        }
        #[cfg(feature = "ble-gatt-dle")]
        if payload.len() > usize::from(link.tx_airtime().octets()) {
            return Err(20);
        }
        let offset = (halfword(TX_HEAD_REGISTER) & 0x3fff) * 4;
        if !matches!(offset, TX_OFFSET | SCAN_TX_OFFSET) {
            return Err(21);
        }
        let descriptor = PACKET_RAM + usize::from(offset);
        let next = halfword(descriptor);
        if next & TX_DONE == 0 {
            return Err(21);
        }
        for (index, &byte) in payload.iter().enumerate() {
            write8(PACKET_RAM + usize::from(TX_PAYLOAD_OFFSET) + index, byte);
        }
        set_halfword(descriptor + 2, u16::from_le_bytes(header));
        set_halfword(descriptor + 4, TX_PAYLOAD_OFFSET);
        fence(Ordering::Release);
        set_halfword(descriptor, next & !TX_DONE);
        #[cfg(feature = "ble-gatt-dle")]
        if length_request {
            link.length_response = LengthResponse::Waiting;
        }
        #[cfg(feature = "ble-gatt-dle")]
        FR29_DLE_OBSERVATIONS[3].fetch_max(u32::from(header[1]), Ordering::Relaxed);
        link.tx = Some(PendingTx {
            #[cfg(feature = "ble-gatt-dle")]
            len: header[1],
            token,
            offset,
            is_echo: echo_fragment(&mut link.echo_remaining, kind, payload),
        });
        increment(TX_COMMITS);
        if kind == FrameKind::Start {
            record_att(payload, ATT_TX, LAST_ATT_TX);
        }
        Ok(TxAdmission::Committed)
    }

    fn service_radio(&mut self) -> Result<(), u32> {
        if !self.initialized {
            return Ok(());
        }
        let now = clock().ok_or(3_u32)?;
        #[cfg(feature = "ble-gatt-dle")]
        {
            self.last_clock = now;
        }
        if let Some(pending) = self.pending {
            if let Some(status) = read_end(self.slot)? {
                diagnostic(32, status);
                self.pending = None;
                self.slot = (self.slot + 1) & 15;
                if self.link.is_some() {
                    self.connection_ended(now)?;
                } else {
                    self.advertising_ended()?;
                }
            } else if pending.expired(now) {
                let now = self.restart_baseband()?;
                self.end_link(now)?;
            }
        } else {
            self.schedule(now)?;
        }
        #[cfg(feature = "ble-gatt-event-driven")]
        self.publish_deadline();
        Ok(())
    }

    #[cfg(feature = "ble-gatt-dle")]
    fn pop_into(&mut self, out: &mut LinkEvent) -> Result<bool, u32> {
        let Some(event) = self.events.pop() else {
            return Ok(false);
        };
        *out = match event {
            BoardEvent::Connected(peer, params) => LinkEvent::Connected(peer, params),
            BoardEvent::Received {
                kind,
                len,
                buffer,
                session,
                rx_two_m,
                length_response,
            } => {
                if session != self.session {
                    return Err(21);
                }
                // Length updates are consumed in FIFO order. PHY instants run
                // independently, so use the rate of the completed RX slot.
                let link = self.link.as_mut().ok_or(21_u32)?;
                link.queued_length_responses -= u8::from(length_response);
                let limit = link.rx_airtime(rx_two_m).octets();
                let receiver = self.receiver.as_mut().ok_or(21_u32)?;
                if len > limit {
                    buffer.release(receiver);
                    return Err(20);
                }
                *out = LinkEvent::Received(LinkFrame::EMPTY);
                let LinkEvent::Received(frame) = out else {
                    unreachable!()
                };
                let copied = frame.read_payload(kind, len, |i| buffer.byte(i));
                buffer.release(receiver);
                copied.map_err(|_| 20_u32)?;
                increment(RX_FRAMES);
                FR29_DLE_OBSERVATIONS[2].fetch_max(u32::from(len), Ordering::Relaxed);
                if kind == FrameKind::Start {
                    record_att(frame.payload(), ATT_RX, LAST_ATT_RX);
                }
                return Ok(true);
            }
            BoardEvent::Acknowledged(token) => LinkEvent::Acknowledged(token),
            BoardEvent::Disconnected(reason) => LinkEvent::Disconnected(reason),
            BoardEvent::ParametersUpdated(params) => LinkEvent::ParametersUpdated(params),
            BoardEvent::PhyUpdated(tx, rx) => LinkEvent::PhyUpdated(tx, rx),
        };
        Ok(true)
    }

    fn expire_link(&mut self, now: u32) -> Result<(), u32> {
        if self.link.as_ref().is_some_and(|link| link.expired(now)) {
            self.end_link(now)?;
        }
        Ok(())
    }

    #[cold]
    #[inline(never)]
    fn end_link(&mut self, now: u32) -> Result<(), u32> {
        let established = self.link.as_ref().is_some_and(|link| link.anchor.is_some());
        self.link = None;
        if established {
            #[cfg(feature = "ble-gatt-dle")]
            {
                // No slot is in flight. Retire this epoch's held RX descriptors.
                // Preserve an undelivered connect before reporting its disconnect.
                let mut connected = None;
                while let Some(event) = self.events.pop() {
                    if matches!(event, BoardEvent::Connected(..)) {
                        connected = Some(event);
                    }
                }
                self.receiver = None;
                if let Some(event) = connected {
                    self.events.push(event).map_err(|_| 22_u32)?;
                }
            }
            self.advertisement = None;
            self.events
                .push(BoardEvent::Disconnected(0x08))
                .map_err(|_| 22_u32)?;
        } else if let Some(advertisement) = self.advertisement {
            self.start_advertising(now, &advertisement);
        }
        Ok(())
    }

    #[cold]
    #[inline(never)]
    fn restart_baseband(&mut self) -> Result<u32, u32> {
        increment(RADIO_TIMEOUTS);
        // A timed-out slot may still own exchange RAM. Reset the scheduler
        // before discarding its descriptors and building another event.
        configure_baseband()?;
        let now = clock().ok_or(3_u32)?;
        super::gatt_board_time::rebase_clock(now);
        self.pending = None;
        self.slot = 0;
        Ok(now)
    }

    #[cfg_attr(feature = "ble-gatt-event-driven", inline(never))]
    fn schedule(&mut self, now: u32) -> Result<(), u32> {
        if let Some(link) = self.link.as_mut() {
            link.prepare_timing_update()?;
            if ADAPTIVE_PHY && let Some(rates) = link.phy.apply(link.counter) {
                #[cfg(feature = "ble-gatt-dle")]
                {
                    link.tx_two_m = rates & 3 != 0;
                }
                set_phy(CONNECTION_CONTROL, rates);
                self.events
                    .push(BoardEvent::PhyUpdated(1 + (rates & 3), 1 + (rates >> 2)))
                    .map_err(|_| 22_u32)?;
            }
            let start = link.event.start.coarse;
            let ahead = start.wrapping_sub(now) & CLOCK_MASK;
            if !(1..HALF_CLOCK).contains(&ahead) {
                link.skip_event();
                return self.expire_link(now);
            }
            if ahead <= SLOT_LEAD_TICKS {
                link.apply_map_at_instant();
                if link.parameters_changed {
                    let limit = 2 * link.request.interval - 3;
                    set_halfword(CONTROL + 0x2e, limit);
                    set_halfword(CONTROL + 0x30, limit);
                }
                let event = link.event;
                #[cfg(feature = "ble-gatt-dle")]
                let event = event.with_data_airtime(
                    link.rx_airtime(link.phy.rx_two_m()),
                    link.slot_tx_airtime(),
                );
                #[cfg(all(
                    feature = "mic1-acquisition",
                    feature = "ble-gatt-xip",
                    not(feature = "ble-gatt-dle")
                ))]
                let duration = {
                    // XIP instruction fetch and task preemption can consume the
                    // original scheduling lead. Sample again while owning the
                    // CPU through submission. Two coarse ticks leave at least
                    // one whole 312.5 us tick, independent of fine phase.
                    let (now, submitted) = critical_section::with(|_| {
                        let now = clock().ok_or(3_u32)?;
                        let remaining = start.wrapping_sub(now) & CLOCK_MASK;
                        let submitted = (2..=SLOT_LEAD_TICKS).contains(&remaining).then(|| {
                            let duration = submit_link(self.slot, event, link.counter);
                            gatt_board_wake::record_connection_lead(remaining);
                            duration
                        });
                        Ok::<_, u32>((now, submitted))
                    })?;
                    let Some(duration) = submitted else {
                        link.skip_event();
                        return self.expire_link(now);
                    };
                    duration
                };
                #[cfg(not(all(
                    feature = "mic1-acquisition",
                    feature = "ble-gatt-xip",
                    not(feature = "ble-gatt-dle")
                )))]
                let duration = {
                    #[cfg(feature = "ble-gatt-event-driven")]
                    gatt_board_wake::record_connection_lead(ahead);
                    submit_link(self.slot, event, link.counter)
                };
                self.pending = Some(SlotWatchdog::new(start, duration));
            }
        } else if let Some(advertisement) = self.advertisement
            && (now.wrapping_sub(self.next_advertisement) & CLOCK_MASK) < HALF_CLOCK
        {
            let start = advance_clock(now, SLOT_LEAD_TICKS);
            let words = advertising_slot(start);
            submit(self.slot, &words);
            self.pending = Some(SlotWatchdog::new(start, words[5]));
            self.jitter = self
                .jitter
                .wrapping_mul(1_664_525)
                .wrapping_add(1_013_904_223);
            let interval = u32::from(advertisement.parameters.min_interval()) * 2;
            self.next_advertisement = advance_clock(start, interval + ((self.jitter >> 24) % 33));
        }
        Ok(())
    }
}

impl LinkBackend for Radio {
    type Error = u32;

    fn supports_two_m(&self) -> bool {
        ADAPTIVE_PHY
    }

    fn peer_two_m(&mut self, supported: bool) {
        if ADAPTIVE_PHY && let Some(link) = self.link.as_mut() {
            link.phy.peer_features(supported);
        }
    }

    fn phy_response_mask(&mut self) -> u8 {
        self.link.as_mut().map_or(1, |link| link.phy.respond())
    }

    fn take_feature_query(&mut self) -> bool {
        self.supports_two_m()
            && self
                .link
                .as_mut()
                .is_some_and(|link| link.phy.take_feature_query())
    }

    fn take_phy_request(&mut self) -> Option<u8> {
        if !self.supports_two_m() {
            return None;
        }
        self.link.as_mut()?.phy.take_request()
    }

    fn phy_rejected(&mut self) {
        if let Some(link) = self.link.as_mut() {
            link.phy.rejected();
        }
    }

    fn schedule_phy(&mut self, instant: u16, central_tx: u8, peripheral_tx: u8) -> bool {
        self.link.as_mut().is_some_and(|link| {
            !(self.pending.is_some()
                && instant == link.counter
                && (central_tx | peripheral_tx) != 0)
                && link
                    .phy
                    .schedule(link.counter, instant, central_tx, peripheral_tx)
        })
    }

    #[cfg(feature = "ble-gatt-dle")]
    fn supports_data_length(&self) -> bool {
        true
    }

    #[cfg(feature = "ble-gatt-dle")]
    fn data_length_clock(&self) -> Option<u32> {
        Some(self.last_clock)
    }

    #[cfg(feature = "ble-gatt-dle")]
    fn length_response_status(&self) -> Result<LengthResponseStatus, u32> {
        let Some(link) = self.link.as_ref() else {
            return Ok(match self.events.front() {
                Some(BoardEvent::Disconnected(reason)) => {
                    LengthResponseStatus::Disconnected(*reason)
                }
                _ => LengthResponseStatus::Pending,
            });
        };
        let time = match link.length_response {
            LengthResponse::Received(time) => Some(time),
            LengthResponse::Waiting if self.pending.is_some() => self
                .receiver
                .as_ref()
                .ok_or(21_u32)?
                .ready_time(is_length_response)?
                .map(|time| time.coarse),
            _ => None,
        };
        Ok(time.map_or(
            LengthResponseStatus::Pending,
            LengthResponseStatus::Received,
        ))
    }

    #[cfg(feature = "ble-gatt-dle")]
    fn data_length_updated(&mut self, limits: DataLength) {
        if let Some(link) = self.link.as_mut() {
            link.limits = limits;
            record_limits(limits);
        }
    }

    #[cfg(feature = "ble-gatt-dle")]
    fn max_acl_fragment_len(&self) -> u8 {
        self.link
            .as_ref()
            .map_or(27, |link| link.tx_airtime().octets())
    }

    #[cfg(not(feature = "ble-gatt-dle"))]
    fn max_acl_fragment_len(&self) -> u8 {
        if cfg!(feature = "ble-gatt-freertos") && fr8029d_bringup::connection::WIDE_RECEIVE_WINDOW {
            15
        } else {
            27
        }
    }

    fn reset(&mut self) -> Result<(), u32> {
        self.stop()?;
        if !self.initialized {
            initialize()?;
            self.initialized = true;
        }
        Ok(())
    }

    fn advertise(&mut self, advertisement: &Advertisement) -> Result<(), u32> {
        if !self.initialized
            || self.link.is_some()
            || self.pending.is_some()
            || advertisement.address.bytes() != GATT_ADDRESS
            || advertisement.parameters.channels() != 7
            || advertisement.data.as_slice().len() != LEGACY_ADV_LEN
            || !advertisement.scan_response.as_slice().is_empty()
        {
            return Err(20);
        }
        self.advertisement = Some(*advertisement);
        self.start_advertising(clock().ok_or(3_u32)?, advertisement);
        Ok(())
    }

    fn stop(&mut self) -> Result<(), u32> {
        if self.pending.is_some() {
            match wait_connection_event(self.slot) {
                Ok(_) => {
                    self.slot = (self.slot + 1) & 15;
                    self.pending = None;
                }
                Err(EVENT_COMPLETION_TIMEOUT) => {
                    self.restart_baseband()?;
                }
                Err(error) => return Err(error),
            }
        }
        self.receiver = None;
        self.advertisement = None;
        self.link = None;
        self.events.clear();
        #[cfg(feature = "ble-gatt-event-driven")]
        self.publish_deadline();
        Ok(())
    }

    #[cfg(not(feature = "ble-gatt-dle"))]
    fn poll(&mut self) -> Result<Option<LinkEvent>, u32> {
        if self.events.is_empty() {
            self.service_radio()?;
        }
        Ok(self.events.pop())
    }

    #[cfg(feature = "ble-gatt-dle")]
    fn poll(&mut self) -> Result<Option<LinkEvent>, u32> {
        let mut event = LinkEvent::Disconnected(0);
        Ok(self.poll_into(&mut event)?.then_some(event))
    }

    #[cfg(feature = "ble-gatt-dle")]
    fn service_backpressure(&mut self) -> Result<(), u32> {
        self.service_radio()
    }

    #[cfg(feature = "ble-gatt-dle")]
    fn poll_into(&mut self, out: &mut LinkEvent) -> Result<bool, u32> {
        if self.events.is_empty() {
            self.service_radio()?;
        }
        self.pop_into(out)
    }

    fn transmit(&mut self, token: u32, frame: LinkFrame) -> Result<TxAdmission, u32> {
        let pdu = frame.as_pdu();
        self.commit_payload(token, frame.kind(), [pdu[0], pdu[1]], frame.payload())
    }

    #[cfg(feature = "ble-gatt-dle")]
    fn transmit_payload(
        &mut self,
        token: u32,
        kind: FrameKind,
        more_data: bool,
        payload: &[u8],
    ) -> Result<TxAdmission, u32> {
        let len = u8::try_from(payload.len()).map_err(|_| 20_u32)?;
        if (kind == FrameKind::Control && !(1..=27).contains(&len))
            || (kind == FrameKind::Start && len < 4)
        {
            return Err(20);
        }
        self.commit_payload(
            token,
            kind,
            [kind as u8 | u8::from(more_data) << 4, len],
            payload,
        )
    }

    fn connection_update(&mut self, indication: &[u8]) -> Result<(), u32> {
        let link = self.link.as_mut().ok_or(21_u32)?;
        if link.timing_update.is_some() || link.anchor.is_none() {
            return Err(24);
        }
        let update = ConnectionUpdate::decode(indication, link.counter.wrapping_sub(1))
            .map_err(|_| 24_u32)?;
        if update.instant == link.counter && self.pending.is_some() {
            return Err(24);
        }
        link.timing_update = Some(update);
        increment(UPDATE_REQUESTS);
        Ok(())
    }

    fn channel_map(&mut self, indication: &[u8]) -> Result<(), u32> {
        let link = self.link.as_mut().ok_or(21_u32)?;
        let [1, byte0, byte1, byte2, byte3, byte4, lo, hi] = *indication else {
            return Err(20);
        };
        let instant = u16::from_le_bytes([lo, hi]);
        let delta = instant.wrapping_sub(link.counter);
        let map = [byte0, byte1, byte2, byte3, byte4];
        if byte4 & !0x1f != 0
            || map.iter().map(|x| x.count_ones()).sum::<u32>() < 2
            || delta >= 0x8000
            || link.map_update.is_some()
        {
            return Err(20);
        }
        link.map_update = Some((instant, map));
        Ok(())
    }
}

fn record_att(payload: &[u8], counter: usize, last_opcode: usize) {
    if let [_, _, 4, 0, opcode, ..] = payload {
        increment(counter);
        diagnostic(last_opcode, u32::from(*opcode));
    }
}

// The Mac compares the entire value. SWD publishes its first eight bytes and
// session only after the final LL fragment is acknowledged, including the 15-byte bench.
fn echo_fragment(remaining: &mut u8, kind: FrameKind, payload: &[u8]) -> bool {
    match kind {
        FrameKind::Start => {
            *remaining = 0;
            let [att_len, 0, 4, 0, 0x1b, 9, 0, data @ ..] = payload else {
                return false;
            };
            let Some(value_len @ 1..=20) = att_len.checked_sub(3) else {
                return false;
            };
            let Ok(part_len) = u8::try_from(data.len()) else {
                return false;
            };
            let Some(missing) = value_len.checked_sub(part_len) else {
                return false;
            };
            let mut prefix = [0u8; 8];
            let count = prefix.len().min(data.len());
            prefix[..count].copy_from_slice(&data[..count]);
            diagnostic(NOTIFY_ACK_TOKEN, 0);
            let (words, _) = prefix.as_chunks::<4>();
            diagnostic(NOTIFY_ACK_LOW, u32::from_le_bytes(words[0]));
            diagnostic(NOTIFY_ACK_HIGH, u32::from_le_bytes(words[1]));
            *remaining = missing;
        }
        FrameKind::Continuation if *remaining > 0 => {
            let Ok(len) = u8::try_from(payload.len()) else {
                return false;
            };
            let Some(missing) = remaining.checked_sub(len) else {
                *remaining = 0;
                return false;
            };
            *remaining = missing;
        }
        _ => return false,
    }
    *remaining == 0
}
