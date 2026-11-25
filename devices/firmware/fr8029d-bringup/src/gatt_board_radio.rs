//! FR8029D legacy 1M transport for the existing Rust Host.

use super::{
    CLOCK_MASK, PACKET_RAM, SLOT_LEAD_TICKS, clock,
    connect_probe::Receiver,
    connect_radio::{
        CONNECTED_RX_ERRORS, CONNECTION_CONTROL, LINK_ID, TX_DONE, prepare_link, read_end, submit,
        submit_link, wait_connection_event,
    },
    diagnostic, increment, write_record, write8,
};
use ble_controller::{
    Advertisement, ConnectionParameters, FrameKind, LinkBackend, LinkEvent, LinkFrame, OwnedQueue,
    Peer, TxAdmission,
};
use core::{
    ptr::{read_volatile, write_volatile},
    sync::atomic::{Ordering, fence},
};
use fr8029d_bringup::{
    advertising::{
        CONTROL_OFFSET, RX_COUNT, SCAN_TX_OFFSET, TX_OFFSET, TX_PAYLOAD_OFFSET, advance_clock,
        advertising_slot, connectable_control,
    },
    connection::{ConnectionEvent, ConnectionRequest, ConnectionUpdate, PacketTime},
    wake::SlotWatchdog,
};
#[cfg(feature = "ble-gatt-event-driven")]
use {super::gatt_board_wake, fr8029d_bringup::wake::RadioDeadline};

const HALF_CLOCK: u32 = CLOCK_MASK.div_ceil(2);
const CONTROL: usize = PACKET_RAM + CONNECTION_CONTROL as usize;
const TX_HEAD_REGISTER: usize = CONTROL + 0x24;
const LEGACY_ADV_LEN: usize = 14;
// The mobile scanner must discover the new GATT table and its added CCCD.
const ADDRESS_SUFFIX: u8 = if cfg!(feature = "ble-gatt-freertos") {
    0xa2
} else if cfg!(feature = "ble-gatt-xip") {
    0xa1
} else {
    0xa0
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

#[derive(Debug)]
struct Link {
    request: ConnectionRequest,
    request_time: PacketTime,
    event: ConnectionEvent,
    parameters: ConnectionParameters,
    counter: u16,
    anchor: Option<PacketTime>,
    missed: u16,
    tx: Option<(u32, usize)>,
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
        Ok(Self {
            request,
            request_time,
            event,
            parameters: params,
            counter: 0,
            anchor: None,
            missed: 0,
            tx: None,
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
    next_advertisement: u32,
    slot: u32,
    jitter: u32,
    events: OwnedQueue<LinkEvent, 8>,
}

impl Radio {
    pub(super) const fn new() -> Self {
        Self {
            initialized: false,
            receiver: None,
            advertisement: None,
            link: None,
            pending: None,
            next_advertisement: 0,
            slot: 0,
            jitter: 1,
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
                .push(LinkEvent::ParametersUpdated(link.parameters))
                .map_err(|_| 22_u32)?;
            link.parameters_changed = false;
            increment(UPDATES_APPLIED);
        }
        if let Some((token, descriptor)) = link.tx
            && halfword(descriptor) & TX_DONE != 0
        {
            self.events
                .push(LinkEvent::Acknowledged(token))
                .map_err(|_| 22_u32)?;
            link.tx = None;
            increment(TX_ACKS);
        }
        let receiver = self.receiver.as_mut().ok_or(21_u32)?;
        let mut anchor = None;
        for _ in 0..RX_COUNT {
            let Some(packet) = receiver.pop()? else { break };
            if packet.link_id != u16::from(LINK_ID) || packet.status & CONNECTED_RX_ERRORS != 0 {
                continue;
            }
            let kind = match packet.header & 3 {
                1 => FrameKind::Continuation,
                2 => FrameKind::Start,
                3 => FrameKind::Control,
                _ => continue,
            };
            if anchor.is_none() && link.anchor.is_none() {
                let peer = Peer::new(link.request.initiator, link.request.initiator_random);
                self.events
                    .push(LinkEvent::Connected(peer, link.parameters))
                    .map_err(|_| 22_u32)?;
            }
            anchor.get_or_insert(packet.time.data_anchor());
            let length = usize::from(packet.header >> 8);
            if length != 0 {
                let bytes = packet.payload.get(..length).ok_or(20_u32)?;
                let frame = LinkFrame::new(kind, bytes).map_err(|_| 20_u32)?;
                increment(RX_FRAMES);
                if kind == FrameKind::Start {
                    record_att(bytes, ATT_RX, LAST_ATT_RX);
                }
                self.events
                    .push(LinkEvent::Received(frame))
                    .map_err(|_| 22_u32)?;
            }
        }
        link.advance(anchor);
        self.expire_link(now)
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
            self.advertisement = None;
            self.events
                .push(LinkEvent::Disconnected(0x08))
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
        super::cold_radio::configure_baseband()?;
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
            let start = link.event.start.coarse;
            let ahead = start.wrapping_sub(now) & CLOCK_MASK;
            if !(1..HALF_CLOCK).contains(&ahead) {
                diagnostic(31, start);
                link.apply_map_at_instant();
                link.advance(None);
                return self.expire_link(now);
            }
            if ahead <= SLOT_LEAD_TICKS {
                #[cfg(feature = "ble-gatt-event-driven")]
                gatt_board_wake::record_connection_lead(ahead);
                link.apply_map_at_instant();
                if link.parameters_changed {
                    let limit = 2 * link.request.interval - 3;
                    set_halfword(CONTROL + 0x2e, limit);
                    set_halfword(CONTROL + 0x30, limit);
                }
                let duration = submit_link(self.slot, link.event, link.counter);
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

    fn reset(&mut self) -> Result<(), u32> {
        self.stop()?;
        if !self.initialized {
            super::cold_radio::init()?;
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
            wait_connection_event(self.slot)?;
            self.slot = (self.slot + 1) & 15;
            self.pending = None;
        }
        self.receiver = None;
        self.advertisement = None;
        self.link = None;
        self.events.clear();
        #[cfg(feature = "ble-gatt-event-driven")]
        self.publish_deadline();
        Ok(())
    }

    fn poll(&mut self) -> Result<Option<LinkEvent>, u32> {
        if let Some(event) = self.events.pop() {
            return Ok(Some(event));
        }
        if !self.initialized {
            return Ok(None);
        }
        let now = clock().ok_or(3_u32)?;
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
        Ok(self.events.pop())
    }

    fn transmit(&mut self, token: u32, frame: LinkFrame) -> Result<TxAdmission, u32> {
        let link = self.link.as_mut().ok_or(21_u32)?;
        if self.pending.is_some() || link.tx.is_some() {
            return Ok(TxAdmission::Busy);
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
        for (index, &byte) in frame.payload().iter().enumerate() {
            write8(PACKET_RAM + usize::from(TX_PAYLOAD_OFFSET) + index, byte);
        }
        let pdu = frame.as_pdu();
        set_halfword(descriptor + 2, u16::from_le_bytes([pdu[0], pdu[1]]));
        set_halfword(descriptor + 4, TX_PAYLOAD_OFFSET);
        fence(Ordering::Release);
        set_halfword(descriptor, next & !TX_DONE);
        link.tx = Some((token, descriptor));
        increment(TX_COMMITS);
        if frame.kind() == FrameKind::Start {
            record_att(frame.payload(), ATT_TX, LAST_ATT_TX);
        }
        Ok(TxAdmission::Committed)
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

fn halfword(address: usize) -> u16 {
    // SAFETY: This module reads only fixed, aligned control and TX descriptor fields.
    unsafe { read_volatile(address as *const u16) }
}

fn set_halfword(address: usize, value: u16) {
    // SAFETY: These are aligned fields in the installed control/descriptor
    // records, written between completed events while the scheduler is idle.
    unsafe { write_volatile(address as *mut u16, value) };
}
