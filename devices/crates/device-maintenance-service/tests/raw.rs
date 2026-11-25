#![cfg(feature = "raw-data")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use device_connection::{FirstFragmentQueue, FirstSlotError, FirstSlotOutcome};
use device_maintenance::{Authority, Capability, Profile, raw::*};
use device_maintenance_service::*;
use device_protocol::{
    ConnectionDataLane, DataKind, DataLease, Frame, Header, Kind, ModuleEntry, RawBatch,
};
use device_services::ServiceHandler;
use std::cell::{Cell, RefCell};
use std::num::NonZeroU64;
struct Auth;
impl Authority for Auth {
    fn allows(&self, _: Capability) -> bool {
        true
    }
    fn station_id(&self) -> Option<u32> {
        Some(42)
    }
}
struct Time;
impl Clock for Time {
    fn now_ms(&self) -> Result<u64, Error> {
        Ok(1)
    }
}
struct Port<'a> {
    stops: &'a Cell<u32>,
    sample: bool,
    revoke_on_poll: Option<&'a Cell<bool>>,
    stop_fail: bool,
}
impl RawPort for Port<'_> {
    fn source_count(&self) -> usize {
        1
    }
    fn source(&self, _: usize) -> Option<Source> {
        Some(Source {
            id: 1,
            kind: 3,
            channels: 1,
            formats: 1,
            flags: 1,
        })
    }
    fn channel(&self, _: u16, i: u8) -> Result<Channel, device_maintenance::Error> {
        Ok(Channel {
            index: i,
            format: Format::I16,
            unit: 0,
            scale_num: 1,
            scale_den: 1,
            axis: 0,
        })
    }
    fn configure(&mut self, _: Config) -> Result<Accepted, device_maintenance::Error> {
        Ok(Accepted {
            rate_mhz: 100_000,
            period_ns: 10_000_000,
            stride: 2,
        })
    }
    fn start(&mut self, _: u16) -> Result<u64, device_maintenance::Error> {
        Ok(100)
    }
    fn poll(
        &mut self,
        out: &mut [u8],
        _: u16,
        _: u32,
    ) -> Result<Option<Polled>, device_maintenance::Error> {
        if let Some(scope) = self.revoke_on_poll {
            scope.set(false);
        }
        if !self.sample {
            return Ok(None);
        }
        self.sample = false;
        out[..2].copy_from_slice(&[0x34, 0x12]);
        Ok(Some(Polled {
            samples: 1,
            bytes: 2,
            lost: 0,
            t0_mono_us: 100,
            quality_flags: 0,
            valid: true,
        }))
    }
    fn stop(&mut self) -> Result<(), device_maintenance::Error> {
        self.stops.set(self.stops.get() + 1);
        if self.stop_fail {
            return Err(device_maintenance::Error::Backend);
        }
        Ok(())
    }
}
struct Lane<'a> {
    owner: &'a RefCell<ConnectionDataLane>,
    lease: Option<DataLease>,
}
impl DataLane for Lane<'_> {
    fn available(&self) -> bool {
        self.owner.borrow().available()
    }
    fn acquire(&mut self) -> Result<u16, device_maintenance::Error> {
        let lease = self
            .owner
            .borrow_mut()
            .acquire(DataKind::Raw)
            .map_err(|_| device_maintenance::Error::Busy)?;
        self.lease = Some(lease);
        Ok(lease.stream_id())
    }
    fn owns(&self, s: u16) -> bool {
        self.lease
            .is_some_and(|l| l.stream_id() == s && self.owner.borrow().owns(l))
    }
    fn reserve_message(&mut self, _: u16) -> Result<u16, device_maintenance::Error> {
        self.owner
            .borrow_mut()
            .begin_message(self.lease.ok_or(device_maintenance::Error::Identity)?)
            .map_err(|_| device_maintenance::Error::Busy)
    }
    fn finish_message(&mut self, _: u16, id: u16) -> Result<(), device_maintenance::Error> {
        self.owner
            .borrow_mut()
            .finish_message(self.lease.ok_or(device_maintenance::Error::Identity)?, id)
            .map_err(|_| device_maintenance::Error::Identity)
    }
    fn release(&mut self, _: u16) -> Result<(), device_maintenance::Error> {
        self.owner
            .borrow_mut()
            .release(self.lease.ok_or(device_maintenance::Error::Identity)?)
            .map_err(|_| device_maintenance::Error::Busy)
    }
}
struct Close<'a>(&'a RefCell<ConnectionDataLane>);
impl DataSessionCloser for Close<'_> {
    fn end_data_session(&self) -> Result<(), Error> {
        self.0.borrow_mut().disconnect();
        Ok(())
    }
}
struct Queue {
    full: bool,
    frames: Vec<Vec<u8>>,
}
impl FirstFragmentQueue for Queue {
    fn with_reserved_slot<T, E>(
        &mut self,
        fill: impl FnOnce(&mut [u8]) -> Result<(usize, T), E>,
    ) -> Result<FirstSlotOutcome<T>, FirstSlotError<E>> {
        if self.full {
            return Ok(FirstSlotOutcome::NoSlot);
        }
        let mut slot = [0; 512];
        let (n, value) = fill(&mut slot).map_err(FirstSlotError::Fill)?;
        self.frames.push(slot[..n].to_vec());
        Ok(FirstSlotOutcome::Admitted(value))
    }
}
const ENTRY: ModuleEntry = ModuleEntry {
    module: 0x70,
    major: 1,
    minor: 2,
    availability: 1,
    features: 0,
    input_max: 128,
    output_max: 128,
};
fn req(op: u8, b: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module: 0x70,
            opcode: op,
            message_id: 2,
            offset: 0,
            total_length: u16::try_from(b.len()).unwrap(),
        },
        payload: b,
    }
}
fn configure(h: &mut impl ServiceHandler) {
    let mut out = [0; 128];
    assert_eq!(
        h.dispatch(
            req(3, &[1, 0, 1, 0, 0, 0, 0xa0, 0x86, 1, 0, 0, 0, 0, 0]),
            &mut out
        ),
        Ok(18)
    );
    assert_eq!(
        &out[..18],
        &[
            0, 0, 0, 0, 1, 0, 0, 0, 0xa0, 0x86, 1, 0, 0x80, 0x96, 0x98, 0, 2, 0
        ]
    );
}
#[test]
fn test_should_encode_exact_catalog_and_real_config_before_start() {
    let owner = RefCell::new(ConnectionDataLane::new(NonZeroU64::new(1).unwrap()));
    let mut lane = Lane {
        owner: &owner,
        lease: None,
    };
    let stops = Cell::new(0);
    let mut port = Port {
        stops: &stops,
        sample: true,
        revoke_on_poll: None,
        stop_fail: false,
    };
    let closer = Close(&owner);
    let trace = CleanupTrace::new();
    let mut observed = RawPortObserver::new(&mut port, &trace).unwrap();
    let actor = RawActor::<8>::new(
        Profile::Development,
        &mut observed,
        &mut lane,
        &Auth,
        &Time,
        &closer,
        128,
    )
    .unwrap();
    let mut h = RawHandler::new(&actor, ENTRY).unwrap();
    let mut out = [0; 128];
    assert_eq!(h.dispatch(req(1, &[0, 4]), &mut out), Ok(12));
    assert_eq!(&out[..12], &[0, 0, 0, 0, 255, 1, 1, 0, 3, 1, 1, 1]);
    configure(&mut h);
}
#[test]
fn test_should_require_real_first_slot_before_begin_and_count_only_final_fragment() {
    let owner = RefCell::new(ConnectionDataLane::new(NonZeroU64::new(1).unwrap()));
    let mut lane = Lane {
        owner: &owner,
        lease: None,
    };
    let stops = Cell::new(0);
    let mut port = Port {
        stops: &stops,
        sample: true,
        revoke_on_poll: None,
        stop_fail: false,
    };
    let closer = Close(&owner);
    let trace = CleanupTrace::new();
    let mut observed = RawPortObserver::new(&mut port, &trace).unwrap();
    let actor = RawActor::<8>::new(
        Profile::Development,
        &mut observed,
        &mut lane,
        &Auth,
        &Time,
        &closer,
        128,
    )
    .unwrap();
    let mut h = RawHandler::new(&actor, ENTRY).unwrap();
    configure(&mut h);
    let mut out = [0; 128];
    let start = req(4, &[1, 0, 1, 0, 0, 0]);
    assert_eq!(h.dispatch(start, &mut out), Ok(24));
    h.response_admitted(start, &out[..24]).unwrap();
    actor.poll().unwrap();
    actor.grant(1, 1).unwrap();
    let mut queue = Queue {
        full: true,
        frames: Vec::new(),
    };
    assert_eq!(actor.start_batch(&mut queue, 20), Ok(BatchStart::NoSlot));
    assert!(queue.frames.is_empty());
    queue.full = false;
    assert_eq!(
        actor.start_batch(&mut queue, 20),
        Ok(BatchStart::Admitted {
            message: 1,
            complete: false
        })
    );
    let mut body = Vec::new();
    body.extend_from_slice(Frame::decode(&queue.frames[0], 20, 128).unwrap().payload);
    loop {
        let (token, n) = actor.next_frame(&mut out).unwrap();
        let same = out;
        let (again, n2) = actor.next_frame(&mut out).unwrap();
        assert_eq!((again, n2), (token, n));
        assert_eq!(out, same);
        body.extend_from_slice(Frame::decode(&out[..n], 20, 128).unwrap().payload);
        actor.confirm_frame(token).unwrap();
        if body.len() == 48 {
            break;
        }
    }
    let batch = RawBatch::decode(&body, 128, 2).unwrap();
    assert_eq!(batch.samples, &[0x34, 0x12]);
    assert_eq!(h.dispatch(req(6, &[1, 0]), &mut out), Ok(19));
    assert_eq!(&out[7..19], &[1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]);
}
#[test]
fn test_should_drop_unstarted_sample_when_fifo_has_no_reserved_first_slot() {
    let owner = RefCell::new(ConnectionDataLane::new(NonZeroU64::new(1).unwrap()));
    let mut lane = Lane {
        owner: &owner,
        lease: None,
    };
    let stops = Cell::new(0);
    let mut port = Port {
        stops: &stops,
        sample: true,
        revoke_on_poll: None,
        stop_fail: false,
    };
    let closer = Close(&owner);
    let trace = CleanupTrace::new();
    let mut observed = RawPortObserver::new(&mut port, &trace).unwrap();
    let actor = RawActor::<8>::new(
        Profile::Factory,
        &mut observed,
        &mut lane,
        &Auth,
        &Time,
        &closer,
        128,
    )
    .unwrap();
    let mut h = RawHandler::new(&actor, ENTRY).unwrap();
    configure(&mut h);
    let mut out = [0; 128];
    let start = req(4, &[1, 0, 1, 0, 0, 0]);
    h.dispatch(start, &mut out).unwrap();
    h.response_admitted(start, &out[..24]).unwrap();
    actor.poll().unwrap();
    actor.grant(1, 1).unwrap();
    let mut queue = Queue {
        full: true,
        frames: Vec::new(),
    };
    assert_eq!(actor.start_batch(&mut queue, 20), Ok(BatchStart::NoSlot));
    let stop = req(5, &[1, 0]);
    assert_eq!(h.response_ready(stop), Ok(true));
    assert_eq!(h.dispatch(stop, &mut out), Ok(18));
    assert_eq!(&out[4..18], &[1, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0]);
    assert!(!actor.grant_replay_valid(1, 1));
    assert!(actor.grant_authorized());
}
struct Lost;
impl FirstFragmentQueue for Lost {
    fn with_reserved_slot<T, E>(
        &mut self,
        fill: impl FnOnce(&mut [u8]) -> Result<(usize, T), E>,
    ) -> Result<FirstSlotOutcome<T>, FirstSlotError<E>> {
        let mut slot = [0; 512];
        let _ = fill(&mut slot).map_err(FirstSlotError::Fill)?;
        Err(FirstSlotError::Transport)
    }
}
#[test]
fn test_should_end_shared_owner_on_unknown_queue_commit_after_begin() {
    let owner = RefCell::new(ConnectionDataLane::new(NonZeroU64::new(1).unwrap()));
    let mut lane = Lane {
        owner: &owner,
        lease: None,
    };
    let stops = Cell::new(0);
    let mut port = Port {
        stops: &stops,
        sample: true,
        revoke_on_poll: None,
        stop_fail: false,
    };
    let closer = Close(&owner);
    let trace = CleanupTrace::new();
    let mut observed = RawPortObserver::new(&mut port, &trace).unwrap();
    let actor = RawActor::<8>::new(
        Profile::Factory,
        &mut observed,
        &mut lane,
        &Auth,
        &Time,
        &closer,
        128,
    )
    .unwrap();
    let mut h = RawHandler::new(&actor, ENTRY).unwrap();
    configure(&mut h);
    let mut out = [0; 128];
    let start = req(4, &[1, 0, 1, 0, 0, 0]);
    h.dispatch(start, &mut out).unwrap();
    h.response_admitted(start, &out[..24]).unwrap();
    actor.poll().unwrap();
    actor.grant(1, 1).unwrap();
    assert!(actor.start_batch(&mut Lost, 20).is_err());
    assert!(h.must_end_session());
    assert!(!owner.borrow().available());
    assert_eq!(stops.get(), 1);
    out.fill(0xa5);
    assert!(actor.next_frame(&mut out).is_err());
    assert!(out.iter().all(|v| *v == 0));
}
#[test]
fn test_should_defer_stop_until_final_fragment_and_invalidate_released_grant_context() {
    let owner = RefCell::new(ConnectionDataLane::new(NonZeroU64::new(1).unwrap()));
    let mut lane = Lane {
        owner: &owner,
        lease: None,
    };
    let stops = Cell::new(0);
    let mut port = Port {
        stops: &stops,
        sample: true,
        revoke_on_poll: None,
        stop_fail: false,
    };
    let closer = Close(&owner);
    let trace = CleanupTrace::new();
    let mut observed = RawPortObserver::new(&mut port, &trace).unwrap();
    let actor = RawActor::<8>::new(
        Profile::Factory,
        &mut observed,
        &mut lane,
        &Auth,
        &Time,
        &closer,
        128,
    )
    .unwrap();
    let mut h = RawHandler::new(&actor, ENTRY).unwrap();
    assert!(actor.grant_authorized());
    assert!(actor.validate_grant(1, 1).is_err());
    configure(&mut h);
    let mut out = [0; 128];
    let start = req(4, &[1, 0, 1, 0, 0, 0]);
    h.dispatch(start, &mut out).unwrap();
    h.response_admitted(start, &out[..24]).unwrap();
    actor.poll().unwrap();
    actor.grant(1, 1).unwrap();
    actor
        .start_batch(
            &mut Queue {
                full: false,
                frames: Vec::new(),
            },
            20,
        )
        .unwrap();
    let stop = req(5, &[1, 0]);
    assert_eq!(h.response_ready(stop), Ok(false));
    assert!(actor.grant_replay_valid(1, 1));
    assert!(actor.validate_grant(1, 2).is_err());
    for _ in 0..4 {
        let (token, _) = actor.next_frame(&mut out).unwrap();
        actor.confirm_frame(token).unwrap();
    }
    assert!(!actor.grant_replay_valid(1, 1));
    assert!(actor.grant_authorized());
    assert_eq!(h.response_ready(stop), Ok(true));
    assert_eq!(h.dispatch(stop, &mut out), Ok(18));
    assert_eq!(&out[4..18], &[1, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0, 1, 0]);
}

#[test]
fn test_should_leave_unknown_stop_to_wire_not_found_dispatch() {
    let owner = RefCell::new(ConnectionDataLane::new(NonZeroU64::new(1).unwrap()));
    let mut lane = Lane {
        owner: &owner,
        lease: None,
    };
    let stops = Cell::new(0);
    let mut port = Port {
        stops: &stops,
        sample: true,
        revoke_on_poll: None,
        stop_fail: false,
    };
    let closer = Close(&owner);
    let trace = CleanupTrace::new();
    let mut observed = RawPortObserver::new(&mut port, &trace).unwrap();
    let actor = RawActor::<8>::new(
        Profile::Factory,
        &mut observed,
        &mut lane,
        &Auth,
        &Time,
        &closer,
        128,
    )
    .unwrap();
    let mut h = RawHandler::new(&actor, ENTRY).unwrap();
    let result = h.response_ready(req(5, &[77, 0]));
    assert_eq!(result, Ok(true));
    let mut out = [0; 128];
    assert_eq!(h.dispatch(req(5, &[77, 0]), &mut out), Ok(4));
    assert_eq!(&out[..4], &[8, 0, 0, 0]);
}
struct ReviewScope(Cell<bool>);
impl Authority for ReviewScope {
    fn station_id(&self) -> Option<u32> {
        Some(42)
    }
    fn allows(&self, _: Capability) -> bool {
        self.0.get()
    }
}
#[test]
fn test_should_preserve_raw_cleanup_failure_when_capture_revokes_during_poll() {
    let owner = RefCell::new(ConnectionDataLane::new(NonZeroU64::new(1).unwrap()));
    let mut lane = Lane {
        owner: &owner,
        lease: None,
    };
    let stops = Cell::new(0);
    let auth = ReviewScope(Cell::new(true));
    let mut port = Port {
        stops: &stops,
        sample: true,
        revoke_on_poll: Some(&auth.0),
        stop_fail: true,
    };
    let closer = Close(&owner);
    let trace = CleanupTrace::new();
    let mut observed = RawPortObserver::new(&mut port, &trace).unwrap();
    let actor = RawActor::<8>::new(
        Profile::Factory,
        &mut observed,
        &mut lane,
        &auth,
        &Time,
        &closer,
        128,
    )
    .unwrap();
    let mut h = RawHandler::new(&actor, ENTRY).unwrap();
    configure(&mut h);
    let mut out = [0; 128];
    let start = req(4, &[1, 0, 1, 0, 0, 0]);
    h.dispatch(start, &mut out).unwrap();
    h.response_admitted(start, &out[..24]).unwrap();
    assert!(actor.poll().is_err());
    assert_eq!(stops.get(), 1);
    assert!(h.must_end_session());
    let cleanup = actor.disconnect();
    assert!(cleanup.is_err());
    assert!(actor.disconnect().is_err());
}

#[test]
fn test_should_close_on_failed_stop_from_control_dispatch() {
    let owner = RefCell::new(ConnectionDataLane::new(NonZeroU64::new(1).unwrap()));
    let mut lane = Lane {
        owner: &owner,
        lease: None,
    };
    let stops = Cell::new(0);
    let mut port = Port {
        stops: &stops,
        sample: true,
        revoke_on_poll: None,
        stop_fail: true,
    };
    let closer = Close(&owner);
    let trace = CleanupTrace::new();
    let mut observed = RawPortObserver::new(&mut port, &trace).unwrap();
    let actor = RawActor::<8>::new(
        Profile::Factory,
        &mut observed,
        &mut lane,
        &Auth,
        &Time,
        &closer,
        128,
    )
    .unwrap();
    let mut h = RawHandler::new(&actor, ENTRY).unwrap();
    configure(&mut h);
    let mut out = [0; 128];
    let start = req(4, &[1, 0, 1, 0, 0, 0]);
    h.dispatch(start, &mut out).unwrap();
    h.response_admitted(start, &out[..24]).unwrap();
    assert_eq!(
        h.dispatch(req(5, &[1, 0]), &mut out),
        Err(device_services::SessionError::AuthorizationRevoked)
    );
    assert!(h.must_end_session());
    assert!(h.connection_closed().is_err());
    assert!(h.connection_closed().is_err());
}
