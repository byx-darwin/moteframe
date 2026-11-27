//! Schedules the peripheral's 1M receive windows using the FR8029D baseband.

#[cfg(not(feature = "ble-gatt-board"))]
use super::{CLOCK_MASK, connect_probe::Receiver};
use super::{
    PACKET_RAM, diagnostic,
    radio_hardware::{scheduler_fault, submit_slot, take_event_end},
    status_slot, write_record,
};
#[cfg(not(feature = "ble-gatt-board"))]
use fr8029d_bringup::connection::PacketTime;
use fr8029d_bringup::{
    advertising::{SCAN_TX_OFFSET, TX_OFFSET},
    connection::{ConnectionEvent, ConnectionRequest},
};

pub(super) use fr8029d_chip::radio::records::CONNECTION_CONTROL;
use fr8029d_chip::radio::records::{LINK_EVENT_COUNTER, LINK_RX_WINDOW, returned_tx_descriptor};
pub(super) const LINK_ID: u8 = 1;
#[cfg(not(feature = "ble-gatt-board"))]
const MAX_PROBE_EVENTS: u16 = 20;
pub(super) const CONNECTED_RX_ERRORS: u16 = 0x007f;
pub(super) const EVENT_COMPLETION_TIMEOUT: u32 = 18;

pub(super) fn prepare_link(request: ConnectionRequest, event: ConnectionEvent) {
    // Advertising has completed. Empty, returned TX descriptors allow the
    // baseband to exchange empty PDUs until the Host queues a packet.
    write_record(
        PACKET_RAM + usize::from(TX_OFFSET),
        &returned_tx_descriptor(SCAN_TX_OFFSET),
    );
    write_record(
        PACKET_RAM + usize::from(SCAN_TX_OFFSET),
        &returned_tx_descriptor(TX_OFFSET),
    );
    let control = PACKET_RAM + usize::from(CONNECTION_CONTROL);
    write_record(control, &request.control_words(LINK_ID, TX_OFFSET, event));
}

pub(super) fn submit_link(slot: u32, event: ConnectionEvent, counter: u16) -> u16 {
    let control = PACKET_RAM + usize::from(CONNECTION_CONTROL);
    // The previous event has completed. Preserve the rest of the live link record.
    write_record(control + LINK_RX_WINDOW, &[event.receive_window]);
    write_record(control + LINK_EVENT_COUNTER, &[counter]);
    let words = event.slot_words(CONNECTION_CONTROL);
    submit(slot, &words);
    words[5]
}

pub(super) fn submit(slot: u32, words: &[u16; 8]) {
    submit_slot(slot, words);
}

#[cfg(not(feature = "ble-gatt-board"))]
pub(super) fn probe_link(
    receiver: &mut Receiver,
    request: ConnectionRequest,
    request_time: PacketTime,
    mut slot: u32,
) -> Result<(), u32> {
    let mut event = request.first_event(request_time);
    prepare_link(request, event);
    let mut received_events = 0_u32;
    let mut anchor = None;
    let mut intervals = 1_u16;
    for counter in 0..MAX_PROBE_EVENTS {
        let words = event.slot_words(CONNECTION_CONTROL);
        diagnostic(30, u32::from(counter) + 1);
        diagnostic(31, u32::from(words[1]) | (u32::from(words[2]) << 16));
        submit_link(slot, event, counter);
        diagnostic(32, wait_connection_event(slot)?);
        let mut current_anchor = None;
        for _ in 0..fr8029d_bringup::advertising::RX_COUNT {
            let Some(packet) = receiver.pop()? else { break };
            if packet.link_id != u16::from(LINK_ID) {
                continue;
            }
            diagnostic(33, u32::from(packet.status));
            diagnostic(34, u32::from(packet.header));
            if packet.status & CONNECTED_RX_ERRORS == 0 && packet.header & 3 != 0 {
                current_anchor.get_or_insert(packet.time.data_anchor());
                received_events += 1;
                diagnostic(35, received_events);
            }
        }
        if let Some(now) = current_anchor {
            anchor = Some(now);
            intervals = 1;
        }
        if let Some(anchor) = anchor {
            event = request.event_after(anchor, intervals);
            intervals += 1;
        } else {
            // Until an anchor is measured, repeat the initial receive window
            // one connection interval later. The first attempt is left intact.
            event = request.first_event(PacketTime {
                coarse: request_time
                    .coarse
                    .wrapping_add(4 * u32::from(request.interval) * (u32::from(counter) + 1))
                    & CLOCK_MASK,
                fine_countdown: request_time.fine_countdown,
            });
        }
        slot = (slot + 1) & 15;
    }
    if received_events == 0 {
        Err(19)
    } else {
        Ok(())
    }
}

pub(super) fn read_end(slot: u32) -> Result<Option<u32>, u32> {
    if let Some(error) = scheduler_fault() {
        diagnostic(15, error);
        return Err(15);
    }
    if let Some(status) = take_event_end()
        && status & 2 != 0
        && status_slot(status) == slot
    {
        return Ok(Some(status));
    }
    Ok(None)
}

pub(super) fn wait_connection_event(slot: u32) -> Result<u32, u32> {
    for _ in 0..1_200_000 {
        if let Some(status) = read_end(slot)? {
            return Ok(status);
        }
    }
    Err(EVENT_COMPLETION_TIMEOUT)
}
