//! Event transport models test admission semantics without claiming real device FIFO behavior.
#![allow(clippy::unwrap_used)]
use core::{cell::Cell, num::NonZeroU16};
use device_connection::{
    EventOffer, EventPlane, EventPump, EventRoute, EventSource, PumpFailure, SourceFailure,
};
use device_connection::{FirstFragmentQueue, FirstSlotError, FirstSlotOutcome};
use device_protocol::{Frame, ModuleEntry, ProtocolLimits};
#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)] // Independent injected backend/permission faults in this test model.
struct Source {
    offered: Cell<bool>,
    scope: Cell<bool>,
    generation: Cell<u32>,
    prepares: u32,
    confirmed: u32,
    closed: u32,
    plane: EventPlane,
    cleanup_failure: Cell<bool>,
    confirmation_failure: Cell<bool>,
}
impl Source {
    fn new(plane: EventPlane) -> Self {
        Self {
            offered: Cell::new(true),
            scope: Cell::new(true),
            generation: Cell::new(1),
            prepares: 0,
            confirmed: 0,
            closed: 0,
            plane,
            cleanup_failure: Cell::new(false),
            confirmation_failure: Cell::new(false),
        }
    }
}
impl EventSource for Source {
    type Token = u32;
    fn route(&self) -> Result<Option<EventRoute>, SourceFailure> {
        Ok(self.offered.get().then_some(EventRoute {
            module: if self.plane == EventPlane::Data {
                0x10
            } else {
                2
            },
            opcode: 0x80,
            plane: self.plane,
        }))
    }
    fn prepare(&mut self, out: &mut [u8]) -> Result<EventOffer<u32>, SourceFailure> {
        if out.len() < 21 {
            return Err(SourceFailure::Capacity);
        }
        out[..21].fill(0x42);
        self.prepares += 1;
        Ok(EventOffer {
            token: self.generation.get(),
            length: 21,
            data_message: if self.plane == EventPlane::Data {
                NonZeroU16::new(7)
            } else {
                None
            },
        })
    }
    fn authorized(&self, token: u32) -> bool {
        self.scope.get() && token == self.generation.get()
    }
    fn must_end_session(&self) -> bool {
        false
    }
    fn admitted(&mut self, token: u32) -> Result<(), SourceFailure> {
        if !self.authorized(token) {
            return Err(SourceFailure::Denied);
        }
        self.confirmed += 1;
        self.offered.set(false);
        if self.confirmation_failure.get() {
            Err(SourceFailure::Backend)
        } else {
            Ok(())
        }
    }
    fn connection_closed(&mut self) -> Result<(), SourceFailure> {
        self.closed += 1;
        self.offered.set(false);
        if self.cleanup_failure.get() {
            Err(SourceFailure::Backend)
        } else {
            Ok(())
        }
    }
}
#[derive(Debug, Default)]
struct Queue {
    ready: bool,
    fail_after_fill: bool,
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
        let (length, result) = fill(&mut slot).map_err(FirstSlotError::Fill)?;
        if self.fail_after_fill {
            return Err(FirstSlotError::Transport);
        }
        if length == 0 || length > slot.len() {
            return Err(FirstSlotError::Contract);
        }
        self.frames.push(slot[..length].to_vec());
        Ok(FirstSlotOutcome::Admitted(result))
    }
}
const ENTRIES: [ModuleEntry; 3] = [
    ModuleEntry {
        module: 0,
        major: 1,
        minor: 2,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    },
    ModuleEntry {
        module: 2,
        major: 1,
        minor: 0,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    },
    ModuleEntry {
        module: 0x10,
        major: 1,
        minor: 2,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    },
];
struct NoHandler;
impl device_services::ServiceHandler for NoHandler {
    fn required_features(&self, _: &ModuleEntry, _: u8) -> Option<u32> {
        None
    }
    fn authorized(&self, _: Frame<'_>) -> bool {
        false
    }
    fn dispatch(
        &mut self,
        _: Frame<'_>,
        _: &mut [u8],
    ) -> Result<usize, device_services::SessionError> {
        Err(device_services::SessionError::InvalidHandlerResponse)
    }
}
fn new_pump(
    owner: &device_connection::SharedControlChannel,
    source: Source,
) -> (
    device_connection::ControlConnection<'_>,
    EventPump<'_, Source>,
) {
    use device_services::{BuildProfile, Config, DeviceInfo};
    let config = Config {
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
            control_max: 128,
            data_max: 128,
            idle_ms: 2000,
            fragment_ms: 100,
            response_ms: 10000,
        },
    };
    let (mut control, channel) =
        device_connection::ControlConnection::with_events(config, owner).unwrap();
    control.notifications_enabled().unwrap();
    core_request(&mut control, 1, 1, &[1, 2, 128, 0, 128, 0, 3, 0]);
    drain_control(&mut control);
    (control, EventPump::new(channel, source).unwrap())
}
fn core_request(
    control: &mut device_connection::ControlConnection<'_>,
    op: u8,
    id: u16,
    body: &[u8],
) {
    let mut bytes = [0; 20];
    let n = Frame {
        header: device_protocol::Header {
            kind: device_protocol::Kind::Request,
            module: 0,
            opcode: op,
            message_id: id,
            offset: 0,
            total_length: u16::try_from(body.len()).unwrap(),
        },
        payload: body,
    }
    .encode(&mut bytes)
    .unwrap();
    control.receive(&bytes[..n], 0, &mut NoHandler).unwrap();
}
fn drain_control(control: &mut device_connection::ControlConnection<'_>) {
    while control.response_pending() {
        control.next_frame(&mut NoHandler, &mut [0; 20]).unwrap();
        control.admit_frame(&mut NoHandler).unwrap();
    }
}
#[test]
fn test_should_not_prepare_or_reserve_data_identity_without_real_first_frame_capacity() {
    let owner = device_connection::SharedControlChannel::new();
    let (_control, mut pump) = new_pump(&owner, Source::new(EventPlane::Data));
    let mut queue = Queue::default();
    assert_eq!(pump.flush(0, EventPlane::Data, &mut queue), Ok(false));
    assert_eq!(pump.source().prepares, 0);
    queue.ready = true;
    assert_eq!(pump.flush(0, EventPlane::Data, &mut queue), Ok(true));
    assert_eq!((pump.source().prepares, pump.source().confirmed), (1, 0));
    queue.ready = false;
    assert_eq!(pump.flush(0, EventPlane::Data, &mut queue), Ok(false));
    assert_eq!(pump.source().prepares, 1);
    queue.ready = true;
    assert_eq!(pump.flush(0, EventPlane::Data, &mut queue), Ok(true));
    assert_eq!(pump.flush(0, EventPlane::Data, &mut queue), Ok(true));
    assert_eq!((pump.source().prepares, pump.source().confirmed), (1, 1));
    assert_eq!(queue.frames.len(), 3);
    for (frame, offset) in queue.frames.iter().zip([0, 10, 20]) {
        let frame = Frame::decode(frame, 20, 128).unwrap();
        assert_eq!(frame.header.message_id, 7);
        assert_eq!(frame.header.offset, offset);
    }
}
#[test]
fn test_should_prioritize_control_replies_before_start_and_drain_existing_data_during_cancel() {
    let owner = device_connection::SharedControlChannel::new();
    let (mut control, mut pump) = new_pump(&owner, Source::new(EventPlane::Data));
    let mut queue = Queue {
        ready: true,
        ..Queue::default()
    };
    core_request(&mut control, 5, 2, &[0, 0, 0, 0]);
    assert_eq!(pump.flush(0, EventPlane::Data, &mut queue), Ok(false));
    assert_eq!(pump.source().prepares, 0);
    drain_control(&mut control);
    assert_eq!(pump.flush(0, EventPlane::Data, &mut queue), Ok(true));
    core_request(&mut control, 5, 3, &[0, 0, 0, 0]);
    assert!(control.response_pending());
    assert_eq!(pump.flush(0, EventPlane::Data, &mut queue), Ok(true));
    assert_eq!(pump.flush(0, EventPlane::Data, &mut queue), Ok(true));
    assert_eq!(pump.source().confirmed, 1);
}

#[test]
fn test_should_revoke_pending_scope_even_while_fifo_is_full_and_preserve_cleanup_failure() {
    let owner = device_connection::SharedControlChannel::new();
    let (_control, mut pump) = new_pump(&owner, Source::new(EventPlane::Control));
    let mut queue = Queue {
        ready: true,
        ..Queue::default()
    };
    assert_eq!(pump.flush(0, EventPlane::Control, &mut queue), Ok(true));
    pump.source().scope.set(false);
    pump.source().cleanup_failure.set(true);
    queue.ready = false;
    let error = pump.flush(0, EventPlane::Control, &mut queue).unwrap_err();
    assert_eq!(error.failure, PumpFailure::AuthorizationRevoked);
    assert_eq!(error.cleanup, Some(SourceFailure::Backend));
    assert_eq!(pump.source().closed, 1);
    assert_eq!(queue.frames.len(), 1);
    assert_eq!(
        pump.flush(0, EventPlane::Control, &mut queue)
            .unwrap_err()
            .cleanup,
        Some(SourceFailure::Backend)
    );
    pump.source().cleanup_failure.set(false);
    assert_eq!(pump.close(), Ok(()));
    assert_eq!(pump.source().closed, 2);
}
#[test]
fn test_should_close_on_uncertain_reserved_commit_instead_of_retrying_as_fifo_full() {
    let owner = device_connection::SharedControlChannel::new();
    let (_control, mut pump) = new_pump(&owner, Source::new(EventPlane::Data));
    let mut queue = Queue {
        ready: true,
        fail_after_fill: true,
        ..Queue::default()
    };
    assert_eq!(
        pump.flush(0, EventPlane::Data, &mut queue)
            .unwrap_err()
            .failure,
        PumpFailure::Transport
    );
    assert_eq!(
        (
            pump.source().prepares,
            pump.source().confirmed,
            pump.source().closed
        ),
        (1, 0, 1)
    );
    assert!(queue.frames.is_empty());
}
#[test]
fn test_should_confirm_whole_event_once_and_close_on_actual_admission_callback_failure() {
    let source = Source::new(EventPlane::Control);
    source.confirmation_failure.set(true);
    let owner = device_connection::SharedControlChannel::new();
    let (_control, mut pump) = new_pump(&owner, source);
    let mut queue = Queue {
        ready: true,
        ..Queue::default()
    };
    for _ in 0..2 {
        assert_eq!(pump.flush(0, EventPlane::Control, &mut queue), Ok(true));
    }
    assert_eq!(
        pump.flush(0, EventPlane::Control, &mut queue)
            .unwrap_err()
            .failure,
        PumpFailure::Source(SourceFailure::Backend)
    );
    assert_eq!(pump.source().confirmed, 1);
    assert_eq!(queue.frames.len(), 3);
}
#[test]
fn test_should_reject_reused_data_mid_even_if_a_faulty_source_offers_a_new_token() {
    let owner = device_connection::SharedControlChannel::new();
    let (_control, mut pump) = new_pump(&owner, Source::new(EventPlane::Data));
    let mut queue = Queue {
        ready: true,
        ..Queue::default()
    };
    for _ in 0..3 {
        assert_eq!(pump.flush(0, EventPlane::Data, &mut queue), Ok(true));
    }
    pump.source().offered.set(true);
    pump.source().generation.set(2);
    assert_eq!(
        pump.flush(1, EventPlane::Data, &mut queue)
            .unwrap_err()
            .failure,
        PumpFailure::InvalidOffer
    );
    assert_eq!(queue.frames.len(), 3);
}
#[test]
fn test_should_honor_idle_and_absolute_partial_event_deadlines_and_clock_order() {
    let owner = device_connection::SharedControlChannel::new();
    let (_control, mut pump) = new_pump(&owner, Source::new(EventPlane::Data));
    let mut queue = Queue {
        ready: true,
        ..Queue::default()
    };
    assert_eq!(pump.flush(0, EventPlane::Data, &mut queue), Ok(true));
    assert_eq!(pump.flush(2000, EventPlane::Data, &mut queue), Ok(true));
    assert_eq!(
        pump.flush(2201, EventPlane::Data, &mut queue)
            .unwrap_err()
            .failure,
        PumpFailure::Timeout
    );
    assert_eq!(queue.frames.len(), 2);
    let owner = device_connection::SharedControlChannel::new();
    let (_control, mut pump) = new_pump(&owner, Source::new(EventPlane::Control));
    let mut queue = Queue::default();
    assert_eq!(pump.flush(1000, EventPlane::Control, &mut queue), Ok(false));
    assert_eq!(
        pump.flush(999, EventPlane::Control, &mut queue)
            .unwrap_err()
            .failure,
        PumpFailure::Clock
    );
}
#[test]
fn test_should_bind_pending_bytes_to_the_original_source_authority() {
    let owner = device_connection::SharedControlChannel::new();
    let (_control, mut pump) = new_pump(&owner, Source::new(EventPlane::Control));
    let unrelated = Source::new(EventPlane::Control);
    let mut queue = Queue {
        ready: true,
        ..Queue::default()
    };
    assert_eq!(pump.flush(0, EventPlane::Control, &mut queue), Ok(true));
    pump.source().scope.set(false);
    assert!(unrelated.scope.get());
    // The sender owns its source. A different source cannot be passed to a retry to authorize old bytes.
    assert_eq!(
        pump.flush(1, EventPlane::Control, &mut queue)
            .unwrap_err()
            .failure,
        PumpFailure::AuthorizationRevoked
    );
    assert_eq!(queue.frames.len(), 1);
}

#[derive(Debug)]
struct RetainedSource {
    inner: Source,
    bytes: [u8; 512],
    views: Cell<usize>,
}
impl EventSource for RetainedSource {
    type Token = u32;
    fn route(&self) -> Result<Option<EventRoute>, SourceFailure> {
        self.inner.route()
    }
    fn prepare(&mut self, _: &mut [u8]) -> Result<EventOffer<u32>, SourceFailure> {
        panic!("retained sender must not copy a complete logical body")
    }
    fn authorized(&self, token: u32) -> bool {
        self.inner.authorized(token)
    }
    fn must_end_session(&self) -> bool {
        self.inner.must_end_session()
    }
    fn admitted(&mut self, token: u32) -> Result<(), SourceFailure> {
        self.inner.admitted(token)
    }
    fn connection_closed(&mut self) -> Result<(), SourceFailure> {
        self.bytes.fill(0);
        self.inner.connection_closed()
    }
}
impl device_connection::RetainedEventSource for RetainedSource {
    fn prepare_retained(&mut self, limit: usize) -> Result<EventOffer<u32>, SourceFailure> {
        if limit < self.bytes.len() {
            return Err(SourceFailure::Capacity);
        }
        self.inner.prepares += 1;
        Ok(EventOffer {
            token: self.inner.generation.get(),
            length: self.bytes.len(),
            data_message: NonZeroU16::new(7),
        })
    }
    fn with_retained<T>(
        &self,
        token: u32,
        inspect: impl for<'body> FnOnce(&'body [u8]) -> T,
    ) -> Result<T, SourceFailure> {
        if !self.authorized(token) {
            return Err(SourceFailure::Denied);
        }
        self.views.set(self.views.get() + 1);
        Ok(inspect(&self.bytes))
    }
}
fn retained_pump(
    owner: &device_connection::SharedControlChannel,
) -> (
    device_connection::ControlConnection<'_>,
    EventPump<'_, RetainedSource, device_connection::RetainedBody>,
) {
    use device_services::{BuildProfile, Config, DeviceInfo};
    const LARGE: [ModuleEntry; 3] = {
        let mut entries = ENTRIES;
        entries[2].output_max = 512;
        entries
    };
    let config = Config {
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
        modules: &LARGE,
        limits: ProtocolLimits {
            rx_frame_max: 20,
            tx_frame_max: 20,
            control_max: 128,
            data_max: 512,
            idle_ms: 2000,
            fragment_ms: 100,
            response_ms: 10000,
        },
    };
    let (mut control, channel) =
        device_connection::ControlConnection::with_events(config, owner).unwrap();
    control.notifications_enabled().unwrap();
    core_request(&mut control, 1, 1, &[1, 2, 128, 0, 0, 2, 3, 0]);
    drain_control(&mut control);
    let source = RetainedSource {
        inner: Source::new(EventPlane::Data),
        bytes: [0x42; 512],
        views: Cell::new(0),
    };
    (control, EventPump::new_retained(channel, source).unwrap())
}
#[test]
fn test_should_fragment_full_retained_body_without_preparing_on_full_fifo() {
    let owner = device_connection::SharedControlChannel::new();
    let (_control, mut pump) = retained_pump(&owner);
    let mut queue = Queue::default();
    assert!(!pump.flush(0, EventPlane::Data, &mut queue).unwrap());
    assert_eq!(pump.source().inner.prepares, 0);
    let mut assembled = Vec::new();
    for index in 0..52 {
        queue.ready = false;
        assert!(!pump.flush(index, EventPlane::Data, &mut queue).unwrap());
        queue.ready = true;
        assert!(pump.flush(index, EventPlane::Data, &mut queue).unwrap());
        let frame = Frame::decode(queue.frames.last().unwrap(), 20, 512).unwrap();
        assert_eq!(frame.header.message_id, 7);
        assert_eq!(frame.header.total_length, 512);
        assert_eq!(usize::from(frame.header.offset), assembled.len());
        assembled.extend_from_slice(frame.payload);
        assert_eq!(pump.source().inner.confirmed, u32::from(index == 51));
    }
    assert_eq!(assembled, vec![0x42; 512]);
    assert_eq!(pump.source().inner.prepares, 1);
    assert_eq!(pump.source().views.get(), 52);
    assert!(!pump.pending());
    assert_eq!(
        core::mem::size_of::<EventPump<'_, RetainedSource>>()
            - core::mem::size_of::<EventPump<'_, RetainedSource, device_connection::RetainedBody>>(
            ),
        512
    );
}
#[test]
fn test_should_close_retained_source_on_revocation_or_uncertain_publication() {
    for failed_queue in [false, true] {
        let owner = device_connection::SharedControlChannel::new();
        let (_control, mut pump) = retained_pump(&owner);
        let mut queue = Queue {
            ready: true,
            ..Queue::default()
        };
        assert!(pump.flush(0, EventPlane::Data, &mut queue).unwrap());
        if failed_queue {
            queue.fail_after_fill = true;
        } else {
            pump.source().inner.scope.set(false);
        }
        assert!(pump.flush(1, EventPlane::Data, &mut queue).is_err());
        assert_eq!(pump.source().inner.confirmed, 0);
        assert_eq!(pump.source().inner.closed, 1);
        assert_eq!(pump.source().bytes, [0; 512]);
        assert!(owner.is_closed());
    }
}
