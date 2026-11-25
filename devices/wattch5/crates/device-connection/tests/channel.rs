//! Paired real logical control-channel regressions; queues/backends are offline fixtures only.
#![allow(clippy::unwrap_used)]
use core::{cell::Cell, num::NonZeroU16};
use device_connection::{
    ConnectionError, ControlConnection, EventChannel, EventOffer, EventPlane, EventPump,
    EventRoute, EventSource, FirstFragmentQueue, FirstSlotError, FirstSlotOutcome, PumpFailure,
    Receive, SharedControlChannel, SourceFailure,
};
use device_protocol::{Frame, Header, Kind, ModuleEntry, Progress, ProtocolLimits, Reassembler};
use device_services::{BuildProfile, Config, DeviceInfo, ServiceHandler, SessionError};
const ENTRIES: [ModuleEntry; 3] = [
    ModuleEntry {
        module: 0,
        major: 1,
        minor: 2,
        availability: 1,
        features: 0,
        input_max: 512,
        output_max: 512,
    },
    ModuleEntry {
        module: 0x10,
        major: 1,
        minor: 2,
        availability: 1,
        features: 0,
        input_max: 512,
        output_max: 512,
    },
    ModuleEntry {
        module: 0x20,
        major: 1,
        minor: 0,
        availability: 1,
        features: 0,
        input_max: 512,
        output_max: 512,
    },
];
fn config() -> Config<'static> {
    Config {
        session: 1,
        capability_rev: 1,
        profile: BuildProfile::Commercial,
        info: DeviceInfo {
            product_id: 0,
            fw_major: 1,
            fw_minor: 0,
            fw_patch: 0,
            build_id: 0,
        },
        modules: &ENTRIES,
        limits: ProtocolLimits {
            rx_frame_max: 20,
            tx_frame_max: 20,
            control_max: 512,
            data_max: 512,
            idle_ms: 2000,
            fragment_ms: 100,
            response_ms: 10000,
        },
    }
}
#[derive(Default)]
struct Handler {
    calls: u32,
    closed: u32,
}
impl ServiceHandler for Handler {
    fn required_features(&self, e: &ModuleEntry, op: u8) -> Option<u32> {
        (e.module == 0x20 && matches!(op, 0x21 | 0x32)).then_some(0)
    }
    fn authorized(&self, _: Frame<'_>) -> bool {
        true
    }
    fn dispatch(&mut self, request: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        self.calls += 1;
        if request.header.opcode == 0x21 {
            assert!(device_protocol::ObjectRequest::decode(0x21, request.payload).is_ok());
            out[..4].fill(0);
            Ok(4)
        } else {
            out[..7].copy_from_slice(&[0, 0, 0, 0, 1, 0, 0]);
            Ok(7)
        }
    }
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        self.closed += 1;
        Ok(())
    }
}
struct Source {
    plane: EventPlane,
    present: Cell<bool>,
    large: bool,
    not_ready: Cell<bool>,
    prepares: u32,
    admitted: u32,
    closed: u32,
}
impl Source {
    fn new(plane: EventPlane) -> Self {
        Self {
            plane,
            present: Cell::new(true),
            large: false,
            not_ready: Cell::new(false),
            prepares: 0,
            admitted: 0,
            closed: 0,
        }
    }
}
impl EventSource for Source {
    type Token = u8;
    fn route(&self) -> Result<Option<EventRoute>, SourceFailure> {
        Ok(self.present.get().then_some(EventRoute {
            module: if self.plane == EventPlane::Control {
                0x20
            } else {
                0x10
            },
            opcode: 0x80,
            plane: self.plane,
        }))
    }
    fn prepare(&mut self, out: &mut [u8]) -> Result<EventOffer<u8>, SourceFailure> {
        if self.not_ready.get() {
            return Err(SourceFailure::NotReady);
        }
        if self.large {
            if out.len() < 200 {
                return Err(SourceFailure::Capacity);
            }
            out[..200].fill(0x42);
            self.prepares += 1;
            return Ok(EventOffer {
                token: 1,
                length: 200,
                data_message: None,
            });
        }
        let body = [0x20, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 4, 0, 60, 0, 0, 1];
        if out.len() < body.len() {
            return Err(SourceFailure::Capacity);
        }
        out[..body.len()].copy_from_slice(&body);
        self.prepares += 1;
        Ok(EventOffer {
            token: 1,
            length: body.len(),
            data_message: if self.plane == EventPlane::Data {
                NonZeroU16::new(1)
            } else {
                None
            },
        })
    }
    fn authorized(&self, token: u8) -> bool {
        token == 1
    }
    fn must_end_session(&self) -> bool {
        false
    }
    fn admitted(&mut self, _: u8) -> Result<(), SourceFailure> {
        self.admitted += 1;
        self.present.set(false);
        Ok(())
    }
    fn connection_closed(&mut self) -> Result<(), SourceFailure> {
        self.closed += 1;
        Ok(())
    }
}
#[derive(Default)]
struct Queue {
    ready: bool,
    frames: Vec<Vec<u8>>,
}
impl FirstFragmentQueue for Queue {
    fn with_reserved_slot<T, E>(
        &mut self,
        fill: impl FnOnce(&mut [u8]) -> Result<(usize, T), E>,
    ) -> Result<FirstSlotOutcome<T>, FirstSlotError<E>> {
        if !self.ready {
            return Ok(FirstSlotOutcome::NoSlot);
        }
        let mut slot = [0; 20];
        let (n, value) = fill(&mut slot).map_err(FirstSlotError::Fill)?;
        if n == 0 || n > slot.len() {
            return Err(FirstSlotError::Contract);
        }
        self.frames.push(slot[..n].to_vec());
        Ok(FirstSlotOutcome::Admitted(value))
    }
}
fn send(
    control: &mut ControlConnection<'_>,
    handler: &mut Handler,
    header: Header,
    body: &[u8],
    now: u64,
) -> Result<Receive, ConnectionError> {
    let mut bytes = [0; 20];
    let n = Frame {
        header,
        payload: body,
    }
    .encode(&mut bytes)
    .unwrap();
    control.receive(&bytes[..n], now, handler)
}
fn header(module: u8, opcode: u8, id: u16, total: u16, offset: u16) -> Header {
    Header {
        kind: Kind::Request,
        module,
        opcode,
        message_id: id,
        offset,
        total_length: total,
    }
}
fn drain(control: &mut ControlConnection<'_>, handler: &mut Handler, queue: &mut Queue) {
    while control.response_pending() {
        queue
            .with_reserved_slot(|slot| control.next_frame(handler, slot).map(|n| (n, ())))
            .unwrap();
        control.admit_frame(handler).unwrap();
    }
}
fn active<'a>(
    owner: &'a SharedControlChannel,
    handler: &mut Handler,
) -> (ControlConnection<'a>, EventChannel<'a>) {
    let (mut control, channel) = ControlConnection::with_events(config(), owner).unwrap();
    control.notifications_enabled().unwrap();
    assert_eq!(
        send(
            &mut control,
            handler,
            header(0, 1, 1, 8, 0),
            &[1, 2, 128, 0, 128, 0, 3, 0],
            0
        ),
        Ok(Receive::ResponseReady)
    );
    drain(
        &mut control,
        handler,
        &mut Queue {
            ready: true,
            ..Queue::default()
        },
    );
    (control, channel)
}
#[test]
fn started_control_event_retains_one_request_without_executing_until_event_tail() {
    let owner = SharedControlChannel::new();
    let mut handler = Handler::default();
    let (mut control, channel) = active(&owner, &mut handler);
    let mut pump = EventPump::new(channel, Source::new(EventPlane::Control)).unwrap();
    let mut queue = Queue {
        ready: true,
        ..Queue::default()
    };
    assert_eq!(pump.flush(1, EventPlane::Control, &mut queue), Ok(true));
    assert!(pump.pending());
    assert_eq!(
        send(
            &mut control,
            &mut handler,
            header(0x20, 0x32, 2, 4, 0),
            &[1, 0, 0, 0],
            2
        ),
        Ok(Receive::Deferred)
    );
    assert_eq!(handler.calls, 0);
    assert!(control.waiting_for_handler());
    assert_eq!(control.poll_pending(2, &mut handler), Ok(Receive::Deferred));
    assert_eq!(pump.flush(2, EventPlane::Control, &mut queue), Ok(true));
    assert!(!pump.pending());
    assert_eq!(
        control.poll_pending(3, &mut handler),
        Ok(Receive::ResponseReady)
    );
    assert_eq!(handler.calls, 1);
    drain(&mut control, &mut handler, &mut queue);
    let mut peer = Reassembler::<128>::new(20, 128).unwrap();
    assert_eq!(
        peer.push(Frame::decode(&queue.frames[0], 20, 128).unwrap(), 1),
        Ok(Progress::Pending)
    );
    assert_eq!(
        peer.push(Frame::decode(&queue.frames[1], 20, 128).unwrap(), 2),
        Ok(Progress::Complete)
    );
    assert_eq!(peer.message().unwrap().payload.len(), 19);
    peer.reset();
    assert_eq!(
        peer.push(Frame::decode(&queue.frames[2], 20, 128).unwrap(), 3),
        Ok(Progress::Complete)
    );
    assert_eq!(peer.message().unwrap().payload, &[0, 0, 0, 0, 1, 0, 0]);
}
#[test]
fn paired_event_source_never_prepares_before_actual_hello_or_its_response_drain() {
    let owner = SharedControlChannel::new();
    let (mut control, channel) = ControlConnection::with_events(config(), &owner).unwrap();
    let mut handler = Handler::default();
    let mut pump = EventPump::new(channel, Source::new(EventPlane::Control)).unwrap();
    let mut queue = Queue {
        ready: true,
        ..Queue::default()
    };
    assert_eq!(pump.flush(0, EventPlane::Control, &mut queue), Ok(false));
    assert_eq!(pump.source().prepares, 0);
    control.notifications_enabled().unwrap();
    send(
        &mut control,
        &mut handler,
        header(0, 1, 1, 8, 0),
        &[1, 2, 128, 0, 128, 0, 3, 0],
        0,
    )
    .unwrap();
    assert_eq!(pump.flush(0, EventPlane::Control, &mut queue), Ok(false));
    drain(&mut control, &mut handler, &mut queue);
    assert_eq!(pump.flush(0, EventPlane::Control, &mut queue), Ok(true));
}

#[test]
fn partial_request_and_queued_response_prevent_new_event_start() {
    let owner = SharedControlChannel::new();
    let mut handler = Handler::default();
    let (mut control, channel) = active(&owner, &mut handler);
    let mut pump = EventPump::new(channel, Source::new(EventPlane::Control)).unwrap();
    let mut queue = Queue {
        ready: true,
        ..Queue::default()
    };
    let body = [1, 0, 0, 0, 0, 0, 0, 0, 10, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10];
    assert_eq!(
        send(
            &mut control,
            &mut handler,
            header(0x20, 0x21, 2, 20, 0),
            &body[..10],
            1
        ),
        Ok(Receive::Pending)
    );
    assert_eq!(pump.flush(1, EventPlane::Control, &mut queue), Ok(false));
    assert_eq!(pump.source().prepares, 0);
    assert_eq!(
        send(
            &mut control,
            &mut handler,
            header(0x20, 0x21, 2, 20, 10),
            &body[10..],
            2
        ),
        Ok(Receive::ResponseReady)
    );
    assert_eq!(handler.calls, 1);
    assert_eq!(pump.flush(2, EventPlane::Control, &mut queue), Ok(false));
    drain(&mut control, &mut handler, &mut queue);
    assert_eq!(pump.flush(3, EventPlane::Control, &mut queue), Ok(true));
}
#[test]
fn only_one_pair_can_ever_be_issued_by_an_external_owner() {
    let owner = SharedControlChannel::new();
    let (_control, _token) = ControlConnection::with_events(config(), &owner).unwrap();
    assert!(ControlConnection::with_events(config(), &owner).is_err());
}
#[test]
fn control_close_invalidates_and_cleans_the_event_half_before_any_more_frames() {
    let owner = SharedControlChannel::new();
    let mut handler = Handler::default();
    let (mut control, channel) = active(&owner, &mut handler);
    let mut pump = EventPump::new(channel, Source::new(EventPlane::Control)).unwrap();
    control.close(&mut handler).unwrap();
    let mut queue = Queue {
        ready: true,
        ..Queue::default()
    };
    assert_eq!(
        pump.flush(1, EventPlane::Control, &mut queue)
            .unwrap_err()
            .failure,
        PumpFailure::Closed
    );
    assert_eq!(pump.source().prepares, 0);
    assert_eq!(pump.source().closed, 1);
    assert!(queue.frames.is_empty());
}
#[test]
fn event_close_invalidates_and_cleans_control_half_and_cannot_reopen() {
    let owner = SharedControlChannel::new();
    let mut handler = Handler::default();
    let (mut control, channel) = active(&owner, &mut handler);
    let mut pump = EventPump::new(channel, Source::new(EventPlane::Control)).unwrap();
    pump.close().unwrap();
    assert!(control.active_limits().is_none());
    assert_eq!(control.tick(1, &mut handler), Err(ConnectionError::Closed));
    assert_eq!(handler.closed, 1);
    assert_eq!(
        control.notifications_enabled(),
        Err(ConnectionError::Closed)
    );
}
#[test]
fn drop_mutes_the_counterpart_without_claiming_backend_cleanup_succeeded() {
    let owner = SharedControlChannel::new();
    let mut handler = Handler::default();
    let (control, channel) = active(&owner, &mut handler);
    let mut pump = EventPump::new(channel, Source::new(EventPlane::Control)).unwrap();
    drop(control);
    let mut queue = Queue {
        ready: true,
        ..Queue::default()
    };
    assert_eq!(
        pump.flush(1, EventPlane::Control, &mut queue)
            .unwrap_err()
            .failure,
        PumpFailure::Closed
    );
    assert_eq!(pump.source().closed, 1);
    assert_eq!(handler.closed, 0);
}
#[test]
fn source_preparation_uses_actual_lower_hello_capacity_from_the_same_connection() {
    let owner = SharedControlChannel::new();
    let mut handler = Handler::default();
    let (mut control, channel) = active(&owner, &mut handler);
    assert_eq!(control.active_limits().unwrap().control_max, 128);
    let mut source = Source::new(EventPlane::Control);
    source.large = true;
    let mut pump = EventPump::new(channel, source).unwrap();
    let mut queue = Queue {
        ready: true,
        ..Queue::default()
    };
    assert_eq!(
        pump.flush(1, EventPlane::Control, &mut queue)
            .unwrap_err()
            .failure,
        PumpFailure::Source(SourceFailure::Capacity)
    );
    assert_eq!(pump.source().prepares, 0);
    assert!(queue.frames.is_empty());
    assert_eq!(control.tick(1, &mut handler), Err(ConnectionError::Closed));
}
#[test]
fn no_slot_and_not_ready_leave_logical_control_channel_available_for_a_real_request() {
    let owner = SharedControlChannel::new();
    let mut handler = Handler::default();
    let (mut control, channel) = active(&owner, &mut handler);
    let source = Source::new(EventPlane::Control);
    source.not_ready.set(true);
    let mut pump = EventPump::new(channel, source).unwrap();
    let mut queue = Queue::default();
    assert_eq!(pump.flush(1, EventPlane::Control, &mut queue), Ok(false));
    queue.ready = true;
    assert_eq!(pump.flush(1, EventPlane::Control, &mut queue), Ok(false));
    assert_eq!(pump.source().prepares, 0);
    assert_eq!(
        send(
            &mut control,
            &mut handler,
            header(0x20, 0x32, 2, 4, 0),
            &[1, 0, 0, 0],
            1
        ),
        Ok(Receive::ResponseReady)
    );
    assert_eq!(handler.calls, 1);
    drain(&mut control, &mut handler, &mut queue);
}
