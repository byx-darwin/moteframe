//! End-to-end software model: actual core/history handlers, shared DATA owner and reserved event FIFO.
#![cfg(feature = "history")]
#![allow(clippy::unwrap_used)]
use core::{cell::Cell, num::NonZeroU64};
use device_application::{
    HistoryEventClock, HistoryEvents, HistoryPort, ResourceRouter, ServiceCell,
};
use device_connection::{
    ControlConnection, EventPlane, EventPump, FirstFragmentQueue, FirstSlotError, FirstSlotOutcome,
    SharedControlChannel,
};
use device_history_service::*;
use device_protocol::*;
use device_services::*;
use device_storage::{CommitState, StoredRecord};
const DATASET: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
const DOMAINS: [HistoryDomain; 1] = [HistoryDomain {
    domain: 32,
    epoch: 3,
    oldest_id: 4,
    newest_id: 4,
}];
const ENTRIES: [ModuleEntry; 2] = [
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
        module: 16,
        major: 1,
        minor: 2,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    },
];
struct Access(Cell<bool>);
impl HistoryAccess for Access {
    fn module_allowed(&self) -> bool {
        self.0.get()
    }
    fn allowed(&self, dataset: [u8; 16], domain: Option<u8>, _: HistoryOperation) -> bool {
        self.0.get() && dataset == DATASET && domain.is_none_or(|d| d == 32)
    }
}
struct Clock(Cell<u64>);
impl HistoryEventClock for Clock {
    fn now_ms(&self) -> Result<u64, HistoryFailure> {
        Ok(self.0.get())
    }
}
struct Info;
impl TrustedAccessProvider for Info {
    fn access_snapshot(&self) -> AccessSnapshot {
        AccessSnapshot {
            state: AccessState::Authorized,
            reason: 0,
        }
    }
}
struct Store<'a> {
    lease: Option<StoreLease>,
    reads: &'a Cell<u32>,
    proof: &'a Cell<bool>,
    acks: &'a Cell<u32>,
    closes: &'a Cell<u32>,
}
impl Store<'_> {
    fn live(&self, lease: StoreLease, access: &dyn HistoryAccess) -> Result<(), HistoryFailure> {
        if !access.module_allowed() || !access.allowed(DATASET, Some(32), HistoryOperation::Read) {
            Err(HistoryFailure::Denied)
        } else if self.lease != Some(lease) {
            Err(HistoryFailure::CursorExpired)
        } else {
            Ok(())
        }
    }
}
impl RecordStore for Store<'_> {
    fn directory(&self) -> Result<Directory<'_>, HistoryFailure> {
        Ok(Directory {
            dataset: DATASET,
            domains: &DOMAINS,
        })
    }
    fn open(
        &mut self,
        snapshot: u32,
        filter: HistoryFilter,
        _: u64,
        access: &dyn HistoryAccess,
    ) -> Result<StoreLease, HistoryFailure> {
        if !access.module_allowed() || filter.domain != 32 {
            return Err(HistoryFailure::Denied);
        }
        let lease = StoreLease {
            handle: u64::from(snapshot),
            snapshot: HistorySnapshot {
                snapshot,
                epoch: 3,
                first_id: 4,
                last_id: 4,
                count: 1,
                lease_s: 60,
            },
            committed_id: 0,
        };
        self.lease = Some(lease);
        Ok(lease)
    }
    fn plan(
        &mut self,
        lease: StoreLease,
        after: u64,
        max: u16,
        _: u64,
        access: &dyn HistoryAccess,
    ) -> Result<ReadPlan, HistoryFailure> {
        self.live(lease, access)?;
        let mut ids = [0; 16];
        let count = u16::from(after < 4 && max > 0);
        if count == 1 {
            ids[0] = 4;
        }
        Ok(ReadPlan {
            ids,
            count,
            end_after_id: if count == 1 { 4 } else { after },
            eof: true,
        })
    }
    fn read(
        &self,
        lease: StoreLease,
        id: u64,
        out: &mut [u8],
        access: &dyn HistoryAccess,
    ) -> Result<RecordContent, HistoryFailure> {
        self.live(lease, access)?;
        if id != 4 || out.len() < 4 {
            return Err(HistoryFailure::NotFound);
        }
        out[..4].copy_from_slice(&[60, 0, 0, 1]);
        self.reads.set(self.reads.get() + 1);
        let stored = StoredRecord {
            domain: 32,
            epoch: 3,
            record_id: 4,
            record_type: 32,
            schema: 1,
            utc_ms: 0,
            flags: 0,
            payload: &out[..4],
        };
        Ok(RecordContent {
            domain: 32,
            epoch: 3,
            record_id: 4,
            record_type: 32,
            schema: 1,
            utc_ms: 0,
            flags: 0,
            length: 4,
            commit: CommitState::Committed,
            stored_crc: stored.crc().map_err(|_| HistoryFailure::StoredIntegrity)?,
        })
    }
    fn validate_record(
        &self,
        _: StoreLease,
        record: StoredRecord<'_>,
    ) -> Result<(), HistoryFailure> {
        if record.domain == 32
            && record.record_type == 32
            && record.schema == 1
            && record.payload == [60, 0, 0, 1]
        {
            Ok(())
        } else {
            Err(HistoryFailure::Invalid)
        }
    }
    fn record_admitted(
        &mut self,
        lease: StoreLease,
        id: u64,
        access: &dyn HistoryAccess,
    ) -> Result<(), HistoryFailure> {
        self.live(lease, access)?;
        if id != 4 {
            return Err(HistoryFailure::NotFound);
        }
        self.proof.set(true);
        Ok(())
    }
    fn ack_atomic(
        &mut self,
        _: StoreLease,
        _: HistoryFilter,
        _: u32,
        _: u64,
        _: u64,
        _: &dyn HistoryAccess,
    ) -> Result<u64, HistoryFailure> {
        self.acks.set(self.acks.get() + 1);
        Err(HistoryFailure::Storage)
    }
    fn count(&self, _: HistoryFilter, _: &dyn HistoryAccess) -> Result<(u32, u32), HistoryFailure> {
        Ok((3, 1))
    }
    fn renew(
        &mut self,
        lease: StoreLease,
        _: u64,
        access: &dyn HistoryAccess,
    ) -> Result<(), HistoryFailure> {
        self.live(lease, access)
    }
    fn close(&mut self, lease: StoreLease) -> Result<(), HistoryFailure> {
        if self.lease == Some(lease) {
            self.lease = None;
            self.closes.set(self.closes.get() + 1);
            Ok(())
        } else {
            Err(HistoryFailure::CursorExpired)
        }
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
        let (n, result) = fill(&mut slot).map_err(FirstSlotError::Fill)?;
        if n > 20 || n == 0 {
            return Err(FirstSlotError::Contract);
        }
        self.frames.push(slot[..n].to_vec());
        Ok(FirstSlotOutcome::Admitted(result))
    }
}
fn request(
    connection: &mut ControlConnection<'_>,
    handler: &mut impl ServiceHandler,
    module: u8,
    opcode: u8,
    id: u16,
    body: &[u8],
) {
    let header = Header {
        kind: Kind::Request,
        module,
        opcode,
        message_id: id,
        offset: 0,
        total_length: u16::try_from(body.len()).unwrap(),
    };
    let mut fragments = Fragmenter::new(header, body, 20).unwrap();
    let mut bytes = [0; 20];
    while !fragments.is_complete() {
        let n = fragments.write_next(&mut bytes).unwrap();
        connection.receive(&bytes[..n], 0, handler).unwrap();
        fragments.advance().unwrap();
    }
}
fn drain(connection: &mut ControlConnection<'_>, handler: &mut impl ServiceHandler) {
    let mut queue = Queue {
        ready: true,
        ..Queue::default()
    };
    while connection.response_pending() {
        let outcome = queue
            .with_reserved_slot(|slot| {
                connection
                    .next_frame(handler, slot)
                    .map(|length| (length, ()))
            })
            .unwrap();
        assert!(matches!(outcome, FirstSlotOutcome::Admitted(())));
        connection.admit_frame(handler).unwrap();
    }
}
#[test]
fn test_should_stream_frozen_record_only_after_real_read_and_grant_replies_and_release_after_done()
{
    let (reads, proof, acks, closes) = (Cell::new(0), Cell::new(false), Cell::new(0), Cell::new(0));
    let access = Access(Cell::new(true));
    let lane = SharedDataLane::new(NonZeroU64::new(1).unwrap());
    lane.with_mut(|owner| {
        let lease = owner.acquire(DataKind::Raw)?;
        owner.release(lease)
    })
    .unwrap();
    let mut store = Store {
        lease: None,
        reads: &reads,
        proof: &proof,
        acks: &acks,
        closes: &closes,
    };
    let history =
        HistoryHandler::<_, 1, 90>::new(ENTRIES[1], &mut store, &access, &lane, 128, true).unwrap();
    let owner = ServiceCell::new(history);
    let (mut credit, mut identity) = (HistoryPort::new(&owner), HistoryPort::new(&owner));
    let mut resources = ResourceRouter::new(&Info, &mut credit, &mut identity);
    let mut core = CoreResourceHandler::new(ENTRIES[0], &mut resources).unwrap();
    let mut history_route = owner.handle();
    let mut handlers: [&mut dyn ServiceHandler; 2] = [&mut core, &mut history_route];
    let mut table = HandlerTable::new(&ENTRIES, &mut handlers).unwrap();
    let channel_owner = SharedControlChannel::new();
    let (mut control, event_channel) =
        ControlConnection::with_events(connection_config(), &channel_owner).unwrap();
    control.notifications_enabled().unwrap();
    request(
        &mut control,
        &mut table,
        0,
        1,
        1,
        &[1, 1, 128, 0, 128, 0, 3, 0],
    );
    drain(&mut control, &mut table);
    let mut input = [0; 128];
    let n = HistoryRequest::Open(HistoryFilter {
        domain: 32,
        filter_flags: 0,
        start_utc_ms: 0,
        end_utc_ms: 0,
        record_type: 0,
        parent_session: 0,
    })
    .encode(&mut input)
    .unwrap();
    request(&mut control, &mut table, 16, 2, 2, &input[..n]);
    drain(&mut control, &mut table);
    let n = HistoryRequest::Read {
        snapshot: 1,
        after_id: 0,
        max_records: 1,
    }
    .encode(&mut input)
    .unwrap();
    request(&mut control, &mut table, 16, 3, 3, &input[..n]);
    drain(&mut control, &mut table);
    request(&mut control, &mut table, 0, 7, 4, &[1, 2, 0, 1, 0, 0, 0]);
    let clock = Clock(Cell::new(0));
    let source = HistoryEvents::new(&owner, &clock);
    let mut pump = EventPump::new(event_channel, source).unwrap();
    let mut queue = Queue {
        ready: true,
        ..Queue::default()
    };
    assert_eq!(pump.flush(0, EventPlane::Data, &mut queue), Ok(false));
    assert_eq!(reads.get(), 0);
    drain(&mut control, &mut table);
    queue.ready = false;
    assert_eq!(pump.flush(0, EventPlane::Data, &mut queue), Ok(false));
    assert_eq!(reads.get(), 0);
    queue.ready = true;
    for index in 0..5 {
        assert_eq!(pump.flush(0, EventPlane::Data, &mut queue), Ok(true));
        assert_eq!(proof.get(), index == 4);
    }
    assert_eq!(reads.get(), 1);
    assert_eq!(acks.get(), 0);
    assert_frozen_record(&queue.frames);
    assert!(!lane.available().unwrap());
    assert_eq!(pump.flush(0, EventPlane::Control, &mut queue), Ok(true));
    assert!(!lane.available().unwrap());
    assert_eq!(pump.flush(0, EventPlane::Control, &mut queue), Ok(true));
    assert!(lane.available().unwrap());
    assert_eq!(acks.get(), 0);
    pump.close().unwrap();
    assert_eq!(closes.get(), 1);
}

fn connection_config() -> Config<'static> {
    Config {
        session: 1,
        capability_rev: 1,
        profile: BuildProfile::Commercial,
        info: DeviceInfo {
            product_id: 0,
            fw_major: 0,
            fw_minor: 1,
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
    }
}

fn assert_frozen_record(frames: &[Vec<u8>]) {
    let mut receiver = Reassembler::<128>::new(20, 128).unwrap();
    for bytes in frames {
        let frame = Frame::decode(bytes, 20, 128).unwrap();
        assert_eq!(frame.header.message_id, 1);
        receiver.push(frame, 0).unwrap();
    }
    assert_eq!(
        receiver.message().unwrap().payload,
        &[
            1, 0, 0, 0, 2, 0, 3, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 32, 0, 1, 0, 0, 0, 0, 0, 0, 0, 0,
            0, 0, 0, 4, 0, 60, 0, 0, 1, 0xbd, 0x1a, 0x16, 0x3a
        ]
    );
}
#[test]
fn test_should_recheck_actual_lease_clock_before_each_already_started_fragment() {
    let (reads, proof, acks, closes) = (Cell::new(0), Cell::new(false), Cell::new(0), Cell::new(0));
    let access = Access(Cell::new(true));
    let lane = SharedDataLane::new(NonZeroU64::new(2).unwrap());
    let mut store = Store {
        lease: None,
        reads: &reads,
        proof: &proof,
        acks: &acks,
        closes: &closes,
    };
    let mut history =
        HistoryHandler::<_, 1, 90>::new(ENTRIES[1], &mut store, &access, &lane, 128, true).unwrap();
    let mut input = [0; 128];
    let mut out = [0; 128];
    let open = HistoryRequest::Open(HistoryFilter {
        domain: 32,
        filter_flags: 0,
        start_utc_ms: 0,
        end_utc_ms: 0,
        record_type: 0,
        parent_session: 0,
    });
    let n = open.encode(&mut input).unwrap();
    let frame = Frame {
        header: Header {
            kind: Kind::Request,
            module: 16,
            opcode: 2,
            message_id: 1,
            offset: 0,
            total_length: u16::try_from(n).unwrap(),
        },
        payload: &input[..n],
    };
    history.dispatch(frame, &mut out).unwrap();
    let read = HistoryRequest::Read {
        snapshot: 1,
        after_id: 0,
        max_records: 1,
    };
    let n = read.encode(&mut input).unwrap();
    let frame = Frame {
        header: Header {
            kind: Kind::Request,
            module: 16,
            opcode: 3,
            message_id: 2,
            offset: 0,
            total_length: u16::try_from(n).unwrap(),
        },
        payload: &input[..n],
    };
    let length = history.dispatch(frame, &mut out).unwrap();
    history.response_admitted(frame, &out[..length]).unwrap();
    history.grant(DataKind::History, 1, 1).unwrap();
    let owner = ServiceCell::new(history);
    let clock = Clock(Cell::new(59000));
    let source = HistoryEvents::new(&owner, &clock);
    let channel_owner = SharedControlChannel::new();
    let (mut control, event_channel) =
        ControlConnection::with_events(connection_config(), &channel_owner).unwrap();
    let mut route = owner.handle();
    control.notifications_enabled().unwrap();
    request(
        &mut control,
        &mut route,
        0,
        1,
        1,
        &[1, 2, 128, 0, 128, 0, 3, 0],
    );
    drain(&mut control, &mut route);
    let mut pump = EventPump::new(event_channel, source).unwrap();
    let mut queue = Queue {
        ready: true,
        ..Queue::default()
    };
    assert_eq!(pump.flush(59000, EventPlane::Data, &mut queue), Ok(true));
    assert!(!proof.get());
    clock.0.set(60000);
    assert_eq!(
        pump.flush(60000, EventPlane::Data, &mut queue)
            .unwrap_err()
            .failure,
        device_connection::PumpFailure::AuthorizationRevoked
    );
    assert_eq!(queue.frames.len(), 1);
    assert_eq!(closes.get(), 1);
    assert_eq!(acks.get(), 0);
}

#[test]
fn core_grant_uses_fresh_history_lease_time_from_control_tick() {
    let (reads, proof, acks, closes) = (Cell::new(0), Cell::new(false), Cell::new(0), Cell::new(0));
    let access = Access(Cell::new(true));
    let lane = SharedDataLane::new(NonZeroU64::new(503).unwrap());
    let mut store = Store {
        lease: None,
        reads: &reads,
        proof: &proof,
        acks: &acks,
        closes: &closes,
    };
    let history =
        HistoryHandler::<_, 1, 90>::new(ENTRIES[1], &mut store, &access, &lane, 128, true).unwrap();
    let owner = ServiceCell::new(history);
    let (mut credits, mut identities) = (HistoryPort::new(&owner), HistoryPort::new(&owner));
    let mut resources = ResourceRouter::new(&Info, &mut credits, &mut identities);
    let mut core = CoreResourceHandler::new(ENTRIES[0], &mut resources).unwrap();
    let mut route = owner.handle();
    let mut handlers: [&mut dyn ServiceHandler; 2] = [&mut core, &mut route];
    let mut table = HandlerTable::new(&ENTRIES, &mut handlers).unwrap();
    let mut control = ControlConnection::new(connection_config()).unwrap();
    control.notifications_enabled().unwrap();
    send_at(
        &mut control,
        &mut table,
        0,
        1,
        1,
        &[1, 2, 128, 0, 128, 0, 3, 0],
        0,
    );
    drain(&mut control, &mut table);
    let mut bytes = [0; 128];
    let n = HistoryRequest::Open(HistoryFilter {
        domain: 32,
        filter_flags: 0,
        start_utc_ms: 0,
        end_utc_ms: 0,
        record_type: 0,
        parent_session: 0,
    })
    .encode(&mut bytes)
    .unwrap();
    send_at(&mut control, &mut table, 16, 2, 2, &bytes[..n], 0);
    drain(&mut control, &mut table);
    let n = HistoryRequest::Read {
        snapshot: 1,
        after_id: 0,
        max_records: 1,
    }
    .encode(&mut bytes)
    .unwrap();
    send_at(&mut control, &mut table, 16, 3, 3, &bytes[..n], 0);
    drain(&mut control, &mut table);
    control.tick(60000, &mut table).unwrap();
    send_at(
        &mut control,
        &mut table,
        0,
        7,
        4,
        &[1, 1, 0, 1, 0, 0, 0],
        60000,
    );
    let mut response = Reassembler::<128>::new(20, 128).unwrap();
    while control.response_pending() {
        let n = control.next_frame(&mut table, &mut bytes).unwrap();
        response
            .push(Frame::decode(&bytes[..n], 20, 128).unwrap(), 60000)
            .unwrap();
        control.admit_frame(&mut table).unwrap();
    }
    assert_eq!(&response.message().unwrap().payload[..4], &[4, 0, 0, 0]);
    owner.with_mut(|h| h.advance(60000)).unwrap().unwrap();
    let check = HistoryPort::new(&owner);
    assert_eq!(
        device_application::CreditPort::validate(&check, 1, 1),
        Err(ResourceFailure::InvalidState)
    );
}

fn send_at(
    control: &mut ControlConnection<'_>,
    handler: &mut impl ServiceHandler,
    module: u8,
    opcode: u8,
    id: u16,
    body: &[u8],
    now: u64,
) {
    let header = Header {
        kind: Kind::Request,
        module,
        opcode,
        message_id: id,
        offset: 0,
        total_length: u16::try_from(body.len()).unwrap(),
    };
    let mut fragments = Fragmenter::new(header, body, 20).unwrap();
    let mut bytes = [0; 20];
    while !fragments.is_complete() {
        let n = fragments.write_next(&mut bytes).unwrap();
        control.receive(&bytes[..n], now, handler).unwrap();
        fragments.advance().unwrap();
    }
}

#[test]
fn test_should_drain_started_data_during_actual_cancel_and_report_its_final_message_barrier() {
    let (reads, proof, acks, closes) = (Cell::new(0), Cell::new(false), Cell::new(0), Cell::new(0));
    let access = Access(Cell::new(true));
    let lane = SharedDataLane::new(NonZeroU64::new(1).unwrap());
    lane.with_mut(|owner| {
        let lease = owner.acquire(DataKind::Raw)?;
        owner.release(lease)
    })
    .unwrap();
    let mut store = Store {
        lease: None,
        reads: &reads,
        proof: &proof,
        acks: &acks,
        closes: &closes,
    };
    let history =
        HistoryHandler::<_, 1, 90>::new(ENTRIES[1], &mut store, &access, &lane, 128, true).unwrap();
    let owner = ServiceCell::new(history);
    let (mut credit, mut identity) = (HistoryPort::new(&owner), HistoryPort::new(&owner));
    let mut resources = ResourceRouter::new(&Info, &mut credit, &mut identity);
    let mut core = CoreResourceHandler::new(ENTRIES[0], &mut resources).unwrap();
    let mut history_route = owner.handle();
    let mut handlers: [&mut dyn ServiceHandler; 2] = [&mut core, &mut history_route];
    let mut table = HandlerTable::new(&ENTRIES, &mut handlers).unwrap();
    let channel_owner = SharedControlChannel::new();
    let (mut control, event_channel) =
        ControlConnection::with_events(connection_config(), &channel_owner).unwrap();
    control.notifications_enabled().unwrap();
    request(
        &mut control,
        &mut table,
        0,
        1,
        1,
        &[1, 1, 128, 0, 128, 0, 3, 0],
    );
    drain(&mut control, &mut table);
    let mut input = [0; 128];
    let n = HistoryRequest::Open(HistoryFilter {
        domain: 32,
        filter_flags: 0,
        start_utc_ms: 0,
        end_utc_ms: 0,
        record_type: 0,
        parent_session: 0,
    })
    .encode(&mut input)
    .unwrap();
    request(&mut control, &mut table, 16, 2, 2, &input[..n]);
    drain(&mut control, &mut table);
    let n = HistoryRequest::Read {
        snapshot: 1,
        after_id: 0,
        max_records: 1,
    }
    .encode(&mut input)
    .unwrap();
    request(&mut control, &mut table, 16, 3, 3, &input[..n]);
    drain(&mut control, &mut table);
    request(&mut control, &mut table, 0, 7, 4, &[1, 2, 0, 1, 0, 0, 0]);
    let clock = Clock(Cell::new(0));
    let source = HistoryEvents::new(&owner, &clock);
    let mut pump = EventPump::new(event_channel, source).unwrap();
    let mut queue = Queue {
        ready: true,
        ..Queue::default()
    };
    assert_eq!(pump.flush(0, EventPlane::Data, &mut queue), Ok(false));
    assert_eq!(reads.get(), 0);
    drain(&mut control, &mut table);
    queue.ready = false;
    assert_eq!(pump.flush(0, EventPlane::Data, &mut queue), Ok(false));
    assert_eq!(reads.get(), 0);
    queue.ready = true;
    assert_eq!(pump.flush(0, EventPlane::Data, &mut queue), Ok(true));
    request(&mut control, &mut table, 16, 8, 5, &[1, 0, 0, 0, 2, 0]);
    assert!(control.waiting_for_handler());
    assert_eq!(
        control.poll_pending(0, &mut table),
        Ok(device_connection::Receive::Deferred)
    );
    for _ in 0..4 {
        assert_eq!(pump.flush(0, EventPlane::Data, &mut queue), Ok(true));
    }
    assert_eq!(
        control.poll_pending(0, &mut table),
        Ok(device_connection::Receive::ResponseReady)
    );
    let response = collect_response(&mut control, &mut table, 0);
    assert_eq!(response.message().unwrap().payload, &[0, 0, 0, 0, 1, 0]);
    assert_frozen_record(&queue.frames);
    assert!(lane.available().unwrap());
    assert_eq!(pump.flush(0, EventPlane::Control, &mut queue), Ok(false));
    assert_eq!(acks.get(), 0);
    pump.close().unwrap();
    assert_eq!(closes.get(), 1);
}

fn collect_response(
    control: &mut ControlConnection<'_>,
    handler: &mut impl ServiceHandler,
    now: u64,
) -> Reassembler<128> {
    let mut response = Reassembler::<128>::new(20, 128).unwrap();
    let mut bytes = [0; 128];
    while control.response_pending() {
        let n = control.next_frame(handler, &mut bytes).unwrap();
        response
            .push(Frame::decode(&bytes[..n], 20, 128).unwrap(), now)
            .unwrap();
        control.admit_frame(handler).unwrap();
    }
    response
}
