//! Ownership, backpressure and acknowledgements across the backend boundary.
#![allow(clippy::unwrap_used, reason = "Unexpected test setup failure")]
use ble_controller::*;
use std::collections::VecDeque;

#[derive(Debug, Default)]
struct Backend {
    events: VecDeque<LinkEvent>,
    sent: Vec<(u32, LinkFrame)>,
    #[cfg(feature = "data-length")]
    maintenance: usize,
    #[cfg(feature = "data-length")]
    clock: u32,
    #[cfg(feature = "data-length")]
    receipt: Option<u32>,
    stops: usize,
    busy: bool,
    fail: bool,
    short_fragments: bool,
    lengths: Option<DataLength>,
    peer_two_m: Option<bool>,
}
impl LinkBackend for Backend {
    type Error = ();
    #[cfg(feature = "data-length")]
    fn service_backpressure(&mut self) -> Result<(), ()> {
        self.maintenance += 1;
        Ok(())
    }
    fn supports_data_length(&self) -> bool {
        self.lengths.is_some()
    }
    #[cfg(feature = "data-length")]
    fn data_length_clock(&self) -> Option<u32> {
        Some(self.clock)
    }
    #[cfg(feature = "data-length")]
    fn data_length_response_clock(&self) -> Option<u32> {
        self.receipt
    }
    #[cfg(feature = "data-length")]
    fn length_response_status(&self) -> Result<LengthResponseStatus, ()> {
        if self.fail {
            return Err(());
        }
        if let Some(LinkEvent::Disconnected(reason)) = self.events.front() {
            return Ok(LengthResponseStatus::Disconnected(*reason));
        }
        Ok(self.receipt.map_or(
            LengthResponseStatus::Pending,
            LengthResponseStatus::Received,
        ))
    }
    fn data_length_updated(&mut self, limits: DataLength) {
        self.lengths = Some(limits);
    }
    fn supports_two_m(&self) -> bool {
        self.peer_two_m.is_some()
    }
    fn peer_two_m(&mut self, supported: bool) {
        if let Some(peer) = self.peer_two_m.as_mut() {
            *peer = supported;
        }
    }
    fn phy_response_mask(&mut self) -> u8 {
        3
    }
    fn max_acl_fragment_len(&self) -> u8 {
        if self.short_fragments {
            15
        } else if self.lengths.is_some() {
            251
        } else {
            27
        }
    }
    fn reset(&mut self) -> Result<(), Self::Error> {
        self.events.clear();
        Ok(())
    }
    fn advertise(&mut self, _: &Advertisement) -> Result<(), Self::Error> {
        Ok(())
    }
    fn stop(&mut self) -> Result<(), Self::Error> {
        self.stops += 1;
        Ok(())
    }
    fn poll(&mut self) -> Result<Option<LinkEvent>, Self::Error> {
        Ok(self.events.pop_front())
    }
    fn transmit(&mut self, token: u32, frame: LinkFrame) -> Result<TxAdmission, Self::Error> {
        if self.fail {
            return Err(());
        }
        if self.busy {
            return Ok(TxAdmission::Busy);
        }
        self.sent.push((token, frame));
        Ok(TxAdmission::Committed)
    }
    fn connection_update(&mut self, _: &[u8]) -> Result<(), Self::Error> {
        Ok(())
    }
    fn channel_map(&mut self, _: &[u8]) -> Result<(), Self::Error> {
        Ok(())
    }
}
fn advertisement() -> Advertisement {
    Advertisement::new(
        RandomAddress::new([1, 2, 3, 4, 5, 0xc0]).unwrap(),
        AdvertisingParameters::new(160, 160, 7).unwrap(),
        AdvertisingData::new(&[]).unwrap(),
        AdvertisingData::new(&[]).unwrap(),
    )
}
fn connected() -> Controller<Backend> {
    let mut core = Controller::new(Backend::default(), Version::new(0xff, 0xffff, 1));
    core.reset().unwrap();
    core.advertise(advertisement()).unwrap();
    core.backend_mut().events.push_back(LinkEvent::Connected(
        Peer::new([1; 6], false),
        ConnectionParameters::new(24, 0, 72).unwrap(),
    ));
    core.service().unwrap();
    assert!(matches!(
        core.next_event(),
        Some(ControllerEvent::Connected { .. })
    ));
    core
}
fn frame(n: u8) -> LinkFrame {
    LinkFrame::new(FrameKind::Start, &[0, 0, 4, n]).unwrap()
}

#[test]
fn completes_one_host_packet_after_both_radio_fragments_are_acknowledged() {
    let mut core = connected();
    let id = core.connection().unwrap();
    core.backend_mut().short_fragments = true;
    let mut payload = [0x29; 27];
    payload[..4].copy_from_slice(&[23, 0, 4, 0]);
    core.send_acl(id, LinkFrame::new(FrameKind::Start, &payload).unwrap())
        .unwrap();
    core.service().unwrap();
    let (first_token, first) = core.backend_mut().sent[0];
    assert_eq!(first.as_pdu()[0], 0x12);
    assert_eq!(first.payload(), &payload[..15]);
    core.backend_mut()
        .events
        .push_back(LinkEvent::Acknowledged(first_token));
    core.service().unwrap();
    assert!(core.next_event().is_none());
    assert_eq!(core.pending_acl(), 1);

    let (last_token, last) = core.backend_mut().sent[1];
    assert_eq!(last.kind(), FrameKind::Continuation);
    assert_eq!(last.payload(), &payload[15..]);
    core.backend_mut()
        .events
        .push_back(LinkEvent::Acknowledged(last_token));
    core.service().unwrap();
    assert_eq!(
        core.next_event(),
        Some(ControllerEvent::Completed { id, packets: 1 })
    );
    assert_eq!(core.pending_acl(), 0);
}

#[test]
fn test_should_retry_full_fifo_and_credit_only_matching_air_ack() {
    let mut core = connected();
    let id = core.connection().unwrap();
    core.backend_mut().busy = true;
    core.send_acl(id, frame(1)).unwrap();
    core.send_acl(id, frame(2)).unwrap();
    core.service().unwrap();
    assert!(core.backend_mut().sent.is_empty());
    assert_eq!(core.pending_acl(), 2);
    core.backend_mut().busy = false;
    core.service().unwrap();
    let (token, first) = core.backend_mut().sent[0];
    assert_eq!(first.as_pdu()[0], 0x12);
    assert!(core.next_event().is_none());
    core.backend_mut()
        .events
        .push_back(LinkEvent::Acknowledged(token + 1));
    core.service().unwrap();
    assert_eq!(core.backend_mut().sent.len(), 1);
    core.backend_mut()
        .events
        .push_back(LinkEvent::Acknowledged(token));
    core.service().unwrap();
    assert!(matches!(
        core.next_event(),
        Some(ControllerEvent::Completed { packets: 1, .. })
    ));
    assert_eq!(core.backend_mut().sent[1].1.as_pdu()[0], 2);
    assert_eq!(core.pending_acl(), 1);
}
#[test]
fn test_should_reject_stale_work_and_invalidate_inflight_on_disconnect() {
    let mut core = connected();
    let old = core.connection().unwrap();
    core.send_acl(old, frame(1)).unwrap();
    core.service().unwrap();
    let token = core.backend_mut().sent[0].0;
    core.backend_mut()
        .events
        .push_back(LinkEvent::Disconnected(8));
    core.service().unwrap();
    assert_eq!(core.pending_acl(), 0);
    assert!(core.send_acl(old, frame(2)).is_err());
    core.backend_mut()
        .events
        .push_back(LinkEvent::Acknowledged(token));
    core.service().unwrap();
    assert!(matches!(
        core.next_event(),
        Some(ControllerEvent::Disconnected { reason: 8, .. })
    ));
    assert!(core.next_event().is_none());
    core.reset().unwrap();
    assert!(core.send_acl(old, frame(3)).is_err());
}
#[test]
fn test_should_latch_uncertain_hardware_failure_until_successful_reset() {
    let mut core = connected();
    let id = core.connection().unwrap();
    core.send_acl(id, frame(1)).unwrap();
    core.backend_mut().fail = true;
    assert!(core.service().is_err());
    core.backend_mut().fail = false;
    assert!(core.service().is_err());
    assert!(core.backend_mut().sent.is_empty());
    core.reset().unwrap();
    assert!(core.service().is_ok());
    assert!(core.connection().is_none());
}
#[test]
fn test_should_bound_host_credits_and_process_control_without_host_acl_credit() {
    let mut core = connected();
    let id = core.connection().unwrap();
    for n in 0..4 {
        core.send_acl(id, frame(n)).unwrap();
    }
    assert!(core.send_acl(id, frame(4)).is_err());
    core.backend_mut().events.push_back(LinkEvent::Received(
        LinkFrame::new(FrameKind::Control, &[0x08, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap(),
    ));
    core.service().unwrap();
    let (token, _) = core.backend_mut().sent[0];
    core.backend_mut()
        .events
        .push_back(LinkEvent::Acknowledged(token));
    core.service().unwrap();
    assert_eq!(core.pending_acl(), 3);
    assert_eq!(core.backend_mut().sent.len(), 2);
}

#[test]
fn control_queue_exhaustion_ends_only_the_connection() {
    let mut core = connected();
    let id = core.connection().unwrap();
    for n in 0..4 {
        core.send_acl(id, frame(n)).unwrap();
    }
    for _ in 0..8 {
        core.backend_mut().events.push_back(LinkEvent::Received(
            LinkFrame::new(FrameKind::Control, &[0xff]).unwrap(),
        ));
    }
    core.service().unwrap();
    assert_eq!(
        core.next_event(),
        Some(ControllerEvent::Disconnected { id, reason: 0x16 })
    );
    assert!(core.connection().is_none());
    assert_eq!(core.pending_acl(), 0);
    core.advertise(advertisement()).unwrap();
    core.backend_mut().events.push_back(LinkEvent::Connected(
        Peer::new([2; 6], false),
        ConnectionParameters::new(24, 0, 72).unwrap(),
    ));
    core.service().unwrap();
    let next = core.connection().unwrap();
    assert_ne!(id, next);
    core.send_acl(next, frame(1)).unwrap();
    assert_eq!(core.send_acl(id, frame(1)), Err(ControllerError::State));
}
#[test]
fn local_disconnect_with_full_control_fifo_preserves_inflight_ack() {
    let mut core = connected();
    let id = core.connection().unwrap();
    core.send_acl(id, frame(9)).unwrap();
    core.service().unwrap();
    let flight = core.backend_mut().sent[0].0;
    let features = LinkFrame::new(FrameKind::Control, &[8, 0, 0, 0, 0, 0, 0, 0, 0]).unwrap();
    for _ in 0..8 {
        core.backend_mut()
            .events
            .push_back(LinkEvent::Received(features));
    }
    core.service().unwrap();
    core.disconnect(id, 0x13).unwrap();
    core.service().unwrap();
    assert_eq!(core.backend_mut().sent.len(), 1);
    core.backend_mut()
        .events
        .push_back(LinkEvent::Acknowledged(flight));
    core.service().unwrap();
    assert_eq!(
        core.next_event(),
        Some(ControllerEvent::Completed { id, packets: 1 })
    );
    let (token, term) = core.backend_mut().sent[1];
    assert_eq!(term.payload(), [2, 0x13]);
    core.backend_mut()
        .events
        .push_back(LinkEvent::Acknowledged(token));
    core.service().unwrap();
    assert_eq!(
        core.next_event(),
        Some(ControllerEvent::Disconnected { id, reason: 0x16 })
    );
    core.advertise(advertisement()).unwrap();
}

#[test]
fn test_should_reject_truncated_known_control_procedures() {
    let mut core = connected();
    core.backend_mut().events.push_back(LinkEvent::Received(
        LinkFrame::new(FrameKind::Control, &[0]).unwrap(),
    ));
    assert!(core.service().is_err());
}

#[test]
fn feature_and_phy_responses_follow_radio_capabilities() {
    for capable in [false, true] {
        let mut core = connected();
        core.backend_mut().peer_two_m = capable.then_some(false);
        let feature_req = LinkFrame::new(FrameKind::Control, &[8, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
        core.backend_mut()
            .events
            .push_back(LinkEvent::Received(feature_req));
        core.service().unwrap();
        let (token, features) = core.backend_mut().sent[0];
        assert_eq!(
            features.payload(),
            &[9, 0, u8::from(capable), 0, 0, 0, 0, 0, 0]
        );
        assert_eq!(core.backend_mut().peer_two_m, capable.then_some(true));
        let phy_req = LinkFrame::new(FrameKind::Control, &[0x16, 2, 2]).unwrap();
        core.backend_mut()
            .events
            .extend([LinkEvent::Acknowledged(token), LinkEvent::Received(phy_req)]);
        core.service().unwrap();
        let expected: &[u8] = if capable { &[0x17, 3, 3] } else { &[7, 0x16] };
        assert_eq!(core.backend_mut().sent[1].1.payload(), expected);
    }
}

#[cfg(feature = "data-length")]
#[test]
fn exchanges_directional_lengths_once_and_limits_fragmentation_before_peer_response() {
    let mut core = connected();
    let id = core.connection().unwrap();
    core.backend_mut().lengths = Some(DataLength::LEGACY);
    let request = [0x14, 100, 0, 0xd0, 7, 251, 0, 0x48, 8];
    let rx = |p: &[u8]| LinkEvent::Received(LinkFrame::new(FrameKind::Control, p).unwrap());
    core.backend_mut().events.push_back(rx(&request));
    core.service().unwrap();
    let (token, response) = core.backend_mut().sent[0];
    assert_eq!(
        response.payload(),
        &[0x15, 251, 0, 0x48, 8, 251, 0, 0x48, 8]
    );
    let limits = DataLength {
        tx_octets: 100,
        tx_time: 2000,
        rx_octets: 251,
        rx_time: 2120,
    };
    assert_eq!(core.backend_mut().lengths, Some(limits));
    assert_eq!(
        core.next_event(),
        Some(ControllerEvent::DataLengthChanged { id, limits })
    );
    core.backend_mut()
        .events
        .extend([LinkEvent::Acknowledged(token), rx(&request)]);
    core.service().unwrap();
    assert!(core.next_event().is_none());
    let token = core.backend_mut().sent[1].0;
    let bytes: Vec<u8> = (0..251).collect();
    core.send_acl(id, LinkFrame::new(FrameKind::Start, &bytes).unwrap())
        .unwrap();
    let mut last = token;
    for (i, chunk) in bytes.chunks(100).enumerate() {
        core.backend_mut()
            .events
            .push_back(LinkEvent::Acknowledged(last));
        core.service().unwrap();
        let (token, packet) = core.backend_mut().sent[2 + i];
        assert_eq!(packet.payload(), chunk);
        assert_eq!(
            packet.kind(),
            if i == 0 {
                FrameKind::Start
            } else {
                FrameKind::Continuation
            }
        );
        assert_eq!(core.pending_acl(), 1);
        last = token;
    }
    core.backend_mut()
        .events
        .push_back(LinkEvent::Acknowledged(last));
    core.service().unwrap();
    assert_eq!(
        core.next_event(),
        Some(ControllerEvent::Completed { id, packets: 1 })
    );
}

#[cfg(feature = "data-length")]
#[test]
fn crossed_requests_keep_the_local_procedure_open_until_its_response() {
    let mut core = connected();
    let id = core.connection().unwrap();
    core.backend_mut().lengths = Some(DataLength::LEGACY);
    core.service().unwrap();
    let (token, request) = core.backend_mut().sent[0];
    assert_eq!(request.payload()[0], 0x14);
    let control = |p: &[u8]| LinkEvent::Received(LinkFrame::new(FrameKind::Control, p).unwrap());
    core.backend_mut().events.extend([
        LinkEvent::Acknowledged(token),
        control(&[0x14, 28, 0, 72, 1, 250, 0, 72, 8]),
        control(&[0x15, 251, 0, 72, 8, 251, 0, 72, 8]),
    ]);
    core.service().unwrap();
    let asymmetric = DataLength {
        tx_octets: 28,
        tx_time: 328,
        rx_octets: 250,
        rx_time: 2120,
    };
    assert_eq!(
        core.next_event(),
        Some(ControllerEvent::DataLengthChanged {
            id,
            limits: asymmetric
        })
    );
    assert_eq!(
        core.next_event(),
        Some(ControllerEvent::DataLengthChanged {
            id,
            limits: DataLength::MAX
        })
    );
    assert_eq!(core.backend_mut().sent.last().unwrap().1.payload()[0], 0x15);
    core.backend_mut()
        .events
        .push_back(control(&[0x15, 27, 0, 72, 1, 27, 0, 72, 1]));
    core.service().unwrap();
    assert!(core.next_event().is_none());
    assert_eq!(core.backend_mut().lengths, Some(DataLength::MAX));
}

#[cfg(feature = "data-length")]
#[test]
fn full_receive_pool_leaves_next_packet_with_backend_then_drains_in_order() {
    let mut core = connected();
    let id = core.connection().unwrap();
    core.backend_mut().lengths = Some(DataLength::LEGACY);
    let control = |p: &[u8]| LinkEvent::Received(LinkFrame::new(FrameKind::Control, p).unwrap());
    core.backend_mut()
        .events
        .push_back(control(&[0x14, 251, 0, 72, 8, 251, 0, 72, 8]));
    for index in 0..4 {
        core.backend_mut().events.push_back(LinkEvent::Received(
            LinkFrame::new(FrameKind::Continuation, &[index; 251]).unwrap(),
        ));
    }
    core.service().unwrap();
    assert_eq!(core.backend_mut().events.len(), 2);
    for _ in 0..3 {
        let previous = core.backend_mut().maintenance;
        core.service().unwrap();
        assert_eq!(core.backend_mut().maintenance, previous + 1);
        assert_eq!(core.backend_mut().events.len(), 2);
    }
    assert_eq!(
        core.next_event(),
        Some(ControllerEvent::DataLengthChanged {
            id,
            limits: DataLength::MAX
        })
    );
    for index in 0..4 {
        let Some(ControllerEvent::Acl { id: owner, frame }) = core.next_event() else {
            panic!("missing received ACL");
        };
        assert_eq!(owner, id);
        assert_eq!(frame.payload(), [index; 251]);
        core.service().unwrap();
    }
    assert!(core.next_event().is_none());
}

#[cfg(feature = "data-length")]
#[test]
fn refused_length_procedure_keeps_four_long_host_credits_and_retries_at_27_octets() {
    let mut core = connected();
    let id = core.connection().unwrap();
    core.backend_mut().lengths = Some(DataLength::LEGACY);
    core.service().unwrap();
    let first_token = core.backend_mut().sent[0].0;
    let control = LinkFrame::new(FrameKind::Control, &[0x11, 0x14, 0x1a]).unwrap();
    core.backend_mut().events.extend([
        LinkEvent::Acknowledged(first_token),
        LinkEvent::Received(control),
    ]);
    core.backend_mut().busy = true;
    let bytes: Vec<u8> = (0..251).collect();
    let frame = LinkFrame::new(FrameKind::Start, &bytes).unwrap();
    for _ in 0..4 {
        core.send_acl(id, frame).unwrap();
    }
    assert_eq!(core.send_acl(id, frame), Err(ControllerError::Capacity));
    core.service().unwrap();
    assert_eq!(core.backend_mut().sent.len(), 1);
    core.backend_mut().busy = false;
    for (index, chunk) in bytes.chunks(27).enumerate() {
        core.service().unwrap();
        let (token, fragment) = core.backend_mut().sent[index + 1];
        assert_eq!(fragment.payload(), chunk);
        assert_eq!(core.pending_acl(), 4);
        core.backend_mut()
            .events
            .push_back(LinkEvent::Acknowledged(token));
    }
    core.service().unwrap();
    assert_eq!(
        core.next_event(),
        Some(ControllerEvent::Completed { id, packets: 1 })
    );
    assert_eq!(core.pending_acl(), 3);
    core.backend_mut()
        .events
        .push_back(LinkEvent::Disconnected(8));
    core.service().unwrap();
    assert_eq!(core.pending_acl(), 0);
    assert!(core.send_acl(id, frame).is_err());
}

#[cfg(feature = "data-length")]
#[test]
fn ignores_unsolicited_length_response_while_hardware_has_not_committed_request() {
    let mut core = connected();
    core.backend_mut().lengths = Some(DataLength::LEGACY);
    core.backend_mut().busy = true;
    core.service().unwrap();
    let response =
        LinkFrame::new(FrameKind::Control, &[0x15, 251, 0, 72, 8, 251, 0, 72, 8]).unwrap();
    core.backend_mut()
        .events
        .push_back(LinkEvent::Received(response));
    core.service().unwrap();
    assert_eq!(core.backend_mut().lengths, Some(DataLength::LEGACY));
    assert!(core.next_event().is_none());
    core.backend_mut().busy = false;
    core.service().unwrap();
    assert_eq!(core.backend_mut().sent[0].1.payload()[0], 0x14);
}

#[cfg(feature = "data-length")]
#[test]
fn unanswered_length_request_expires_only_after_admission_across_clock_wrap() {
    const START: u32 = 0x0fff_ff00;
    let mut core = connected();
    let id = core.connection().unwrap();
    core.backend_mut().lengths = Some(DataLength::LEGACY);
    core.backend_mut().busy = true;
    core.service().unwrap();
    core.backend_mut().clock = START;
    core.backend_mut().busy = false;
    core.service().unwrap();
    let (token, request) = core.backend_mut().sent[0];
    assert_eq!(request.payload()[0], 0x14);
    core.backend_mut()
        .events
        .push_back(LinkEvent::Acknowledged(token));
    core.backend_mut().clock = (START + 127_999) & 0x0fff_ffff;
    core.service().unwrap();
    assert_eq!(core.connection(), Some(id));
    core.backend_mut().clock += 1;
    core.service().unwrap();
    assert_eq!(core.connection(), Some(id));
    core.backend_mut().clock += 1;
    core.service().unwrap();
    assert_eq!(
        core.next_event(),
        Some(ControllerEvent::Disconnected { id, reason: 0x22 })
    );
    core.advertise(advertisement()).unwrap();
    core.backend_mut().events.push_back(LinkEvent::Connected(
        Peer::new([1; 6], false),
        ConnectionParameters::new(24, 0, 72).unwrap(),
    ));
    core.service().unwrap();
    let next_id = core.connection().unwrap();
    assert_ne!(next_id, id);
    let (next_token, request) = core.backend_mut().sent[1];
    assert_eq!(request.payload()[0], 0x14);
    let response =
        LinkFrame::new(FrameKind::Control, &[0x15, 251, 0, 72, 8, 251, 0, 72, 8]).unwrap();
    core.backend_mut().events.extend([
        LinkEvent::Acknowledged(next_token),
        LinkEvent::Received(response),
    ]);
    core.service().unwrap();
    assert_eq!(core.backend_mut().lengths, Some(DataLength::MAX));
}

#[cfg(feature = "data-length")]
#[test]
fn length_response_in_the_receive_queue_cancels_timeout() {
    let mut core = connected();
    let id = core.connection().unwrap();
    core.backend_mut().lengths = Some(DataLength::LEGACY);
    core.service().unwrap();
    let token = core.backend_mut().sent[0].0;
    let rsp = LinkFrame::new(FrameKind::Control, &[0x15, 251, 0, 72, 8, 251, 0, 72, 8]).unwrap();
    core.backend_mut().clock = 128_000;
    core.service().unwrap();
    assert_eq!(core.connection(), Some(id));
    core.backend_mut()
        .events
        .extend([LinkEvent::Acknowledged(token), LinkEvent::Received(rsp)]);
    core.backend_mut().clock = 128_000;
    core.service().unwrap();
    core.backend_mut().clock += 128_000;
    core.service().unwrap();
    assert_eq!(core.connection(), Some(id));
    assert_eq!(core.backend_mut().lengths, Some(DataLength::MAX));
}

#[cfg(feature = "data-length")]
#[test]
fn length_deadline_disconnects_with_full_host_queues_in_fifo_order() {
    for acl in [true, false] {
        let mut core = connected();
        let id = core.connection().unwrap();
        core.backend_mut().lengths = Some(DataLength::LEGACY);
        core.service().unwrap();
        let count = if acl { 2 } else { 16 };
        for n in 0..count {
            let event = if acl {
                LinkEvent::Received(frame(n))
            } else {
                LinkEvent::PhyUpdated(1, 1)
            };
            core.backend_mut().events.push_back(event);
        }
        core.service().unwrap();
        core.service().unwrap();
        core.backend_mut().clock = 128_001;
        core.service().unwrap();
        assert_eq!(core.connection(), None);
        assert_eq!(core.backend_mut().stops, 1);
        for n in 0..count {
            let expected = if acl {
                ControllerEvent::Acl {
                    id,
                    frame: frame(n),
                }
            } else {
                ControllerEvent::PhyUpdated { id, tx: 1, rx: 1 }
            };
            assert_eq!(core.next_event(), Some(expected));
        }
        assert_eq!(
            core.next_event(),
            Some(ControllerEvent::Disconnected { id, reason: 0x22 })
        );
        assert!(core.next_event().is_none());
        core.advertise(advertisement()).unwrap();
    }
}

#[cfg(feature = "data-length")]
#[test]
fn length_deadline_keeps_timely_ninth_response_even_after_clock_wrap() {
    let mut core = connected();
    let id = core.connection().unwrap();
    let start = 0x0fff_fe00;
    core.backend_mut().clock = start;
    core.backend_mut().lengths = Some(DataLength::LEGACY);
    core.service().unwrap();
    let ignored = LinkEvent::Received(LinkFrame::new(FrameKind::Control, &[0x07, 0x70]).unwrap());
    core.backend_mut().events.extend([ignored; 8]);
    let response =
        LinkFrame::new(FrameKind::Control, &[0x15, 251, 0, 72, 8, 251, 0, 72, 8]).unwrap();
    core.backend_mut()
        .events
        .push_back(LinkEvent::Received(response));
    core.backend_mut().receipt = Some((start + 127_999) & 0x0fff_ffff);
    core.backend_mut().clock = (start + 128_010) & 0x0fff_ffff;
    core.service().unwrap();
    assert_eq!(core.connection(), Some(id));
    assert_eq!(core.backend_mut().events.len(), 1);
    core.service().unwrap();
    assert_eq!(
        core.next_event(),
        Some(ControllerEvent::DataLengthChanged {
            id,
            limits: DataLength::MAX
        })
    );
}

#[cfg(feature = "data-length")]
#[test]
fn length_deadline_does_not_accept_a_late_response_at_the_front() {
    let mut core = connected();
    let id = core.connection().unwrap();
    core.backend_mut().lengths = Some(DataLength::LEGACY);
    core.service().unwrap();
    let response =
        LinkFrame::new(FrameKind::Control, &[0x15, 251, 0, 72, 8, 251, 0, 72, 8]).unwrap();
    core.backend_mut()
        .events
        .push_back(LinkEvent::Received(response));
    core.backend_mut().receipt = Some(128_001);
    core.backend_mut().clock = 128_001;
    core.service().unwrap();
    assert_eq!(
        core.next_event(),
        Some(ControllerEvent::Disconnected { id, reason: 0x22 })
    );
}

#[cfg(feature = "data-length")]
#[test]
fn length_deadline_preserves_timely_reply_behind_full_acl_pool() {
    let mut core = connected();
    let id = core.connection().unwrap();
    core.backend_mut().lengths = Some(DataLength::LEGACY);
    core.service().unwrap();
    let reject = LinkFrame::new(FrameKind::Control, &[0x11, 0x14, 0x1a]).unwrap();
    core.backend_mut().events.extend([
        LinkEvent::Received(frame(1)),
        LinkEvent::Received(frame(2)),
        LinkEvent::Received(reject),
    ]);
    core.backend_mut().receipt = Some(128_000);
    core.backend_mut().clock = 128_001;
    core.service().unwrap();
    core.service().unwrap();
    assert_eq!(core.connection(), Some(id));
    assert_eq!(core.backend_mut().events.len(), 1);
    for n in [1, 2] {
        assert_eq!(
            core.next_event(),
            Some(ControllerEvent::Acl {
                id,
                frame: frame(n)
            })
        );
    }
    core.service().unwrap();
    assert!(core.backend_mut().events.is_empty());
    core.backend_mut().receipt = None;
    core.backend_mut().clock += 128_000;
    core.service().unwrap();
    assert_eq!(core.connection(), Some(id));
}

#[cfg(feature = "data-length")]
#[test]
fn length_deadline_ignores_truncated_response() {
    let mut core = connected();
    let id = core.connection().unwrap();
    core.backend_mut().lengths = Some(DataLength::LEGACY);
    core.service().unwrap();
    let truncated = LinkFrame::new(FrameKind::Control, &[0x15, 0]).unwrap();
    core.backend_mut()
        .events
        .push_back(LinkEvent::Received(truncated));
    core.backend_mut().clock = 128_001;
    core.service().unwrap();
    assert_eq!(
        core.next_event(),
        Some(ControllerEvent::Disconnected { id, reason: 0x22 })
    );
}

#[cfg(feature = "data-length")]
#[test]
fn queued_radio_disconnect_wins_over_length_timeout_under_backpressure() {
    let mut core = connected();
    let id = core.connection().unwrap();
    core.backend_mut().lengths = Some(DataLength::LEGACY);
    core.service().unwrap();
    core.backend_mut().events.extend([
        LinkEvent::Received(frame(1)),
        LinkEvent::Received(frame(2)),
        LinkEvent::Disconnected(0x08),
    ]);
    core.backend_mut().clock = 124_800;
    core.service().unwrap();
    core.backend_mut().clock = 128_001;
    core.service().unwrap();
    for n in [1, 2] {
        assert_eq!(
            core.next_event(),
            Some(ControllerEvent::Acl {
                id,
                frame: frame(n)
            })
        );
    }
    assert_eq!(
        core.next_event(),
        Some(ControllerEvent::Disconnected { id, reason: 0x08 })
    );
    core.service().unwrap();
    assert_eq!(core.next_event(), None);
    assert_eq!(core.backend_mut().stops, 1);
}

#[cfg(feature = "data-length")]
#[test]
fn length_deadline_propagates_backend_rx_fault() {
    let mut core = connected();
    core.backend_mut().lengths = Some(DataLength::LEGACY);
    core.service().unwrap();
    core.backend_mut().clock = 128_001;
    core.backend_mut().fail = true;
    assert!(matches!(core.service(), Err(ControllerError::Backend(()))));
    assert_eq!(core.next_event(), None);
}
