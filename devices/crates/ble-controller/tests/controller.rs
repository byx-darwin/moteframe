//! Ownership, backpressure and acknowledgements across the backend boundary.
#![allow(clippy::unwrap_used, reason = "Unexpected test setup failure")]
use ble_controller::*;
use std::collections::VecDeque;

#[derive(Debug, Default)]
struct Backend {
    events: VecDeque<LinkEvent>,
    sent: Vec<(u32, LinkFrame)>,
    busy: bool,
    fail: bool,
}
impl LinkBackend for Backend {
    type Error = ();
    fn reset(&mut self) -> Result<(), Self::Error> {
        self.events.clear();
        Ok(())
    }
    fn advertise(&mut self, _: &Advertisement) -> Result<(), Self::Error> {
        Ok(())
    }
    fn stop(&mut self) -> Result<(), Self::Error> {
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
fn connected() -> Controller<Backend> {
    let mut core = Controller::new(Backend::default(), Version::new(0xff, 0xffff, 1));
    core.reset().unwrap();
    let advertisement = Advertisement::new(
        RandomAddress::new([1, 2, 3, 4, 5, 0xc0]).unwrap(),
        AdvertisingParameters::new(160, 160, 7).unwrap(),
        AdvertisingData::new(&[]).unwrap(),
        AdvertisingData::new(&[]).unwrap(),
    );
    core.advertise(advertisement).unwrap();
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
fn test_should_fault_on_control_overflow_instead_of_blocking_ack_processing_forever() {
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
    assert!(core.service().is_err());
}
#[test]
fn test_should_reject_truncated_known_control_procedures() {
    let mut core = connected();
    core.backend_mut().events.push_back(LinkEvent::Received(
        LinkFrame::new(FrameKind::Control, &[0]).unwrap(),
    ));
    assert!(core.service().is_err());
}
