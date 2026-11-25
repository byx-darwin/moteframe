//! Existing history wire service over actual durable records and cumulative ACKs.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "support/media.rs"]
mod media;
use core::{cell::Cell, num::NonZeroU64};
use device_history_service::{
    HistoryAccess, HistoryFailure, HistoryHandler, HistoryOperation, RecordStore,
};
use device_history_store::{AppendRecord, DomainConfig, HistoryClock, HistorySchema, HistoryStore};
use device_protocol::{
    DataKind, Frame, Header, HistoryFilter, HistoryRequest, Kind, ModuleEntry, SharedDataLane,
};
use device_services::ServiceHandler;
use device_storage::StoredRecord;
use device_storage::persistence::{AtomicSnapshot, Layout};
use media::TestMedium;
use std::rc::Rc;

#[derive(Clone, Debug)]
struct Clock(Rc<Cell<u64>>);
impl HistoryClock for Clock {
    fn now_ms(&self) -> Result<u64, HistoryFailure> {
        Ok(self.0.get())
    }
}
#[derive(Clone, Copy, Debug)]
struct Schema;
impl HistorySchema for Schema {
    fn validate(&self, record: StoredRecord<'_>, parent: u64) -> Result<(), HistoryFailure> {
        if record.domain == 0x20
            && record.record_type == 0x20
            && record.schema == 1
            && record.payload.len() == 4
            && record.payload[1..] == [0, 0, 1]
            && parent == 0
        {
            Ok(())
        } else {
            Err(HistoryFailure::StoredIntegrity)
        }
    }
}
#[derive(Debug)]
struct Access;
impl HistoryAccess for Access {
    fn module_allowed(&self) -> bool {
        true
    }
    fn allowed(&self, _: [u8; 16], _: Option<u8>, _: HistoryOperation) -> bool {
        true
    }
}
type Store = HistoryStore<AtomicSnapshot<TestMedium, 2048>, Schema, Clock, 1, 4, 16, 2048>;
type Handler<'a> = HistoryHandler<'a, Store, 1, 16>;
fn layout() -> Layout {
    Layout {
        start: 0,
        bank_bytes: 4096,
    }
}
fn filter() -> HistoryFilter {
    HistoryFilter {
        domain: 0x20,
        filter_flags: 0,
        start_utc_ms: 0,
        end_utc_ms: 0,
        record_type: 0,
        parent_session: 0,
    }
}
fn input(body: &[u8]) -> AppendRecord<'_> {
    AppendRecord {
        domain: 0x20,
        record_type: 0x20,
        schema: 1,
        utc_ms: 1000,
        flags: 1,
        parent_session: 0,
        payload: body,
    }
}
fn initialized(medium: TestMedium, clock: Clock) -> Store {
    HistoryStore::initialize(
        AtomicSnapshot::<_, 2048>::open(medium, layout()).unwrap(),
        Schema,
        clock,
        [7; 16],
        &[DomainConfig {
            domain: 0x20,
            epoch: 3,
        }],
        60,
    )
    .unwrap()
}
fn frame(opcode: u8, bytes: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module: 0x10,
            opcode,
            message_id: 1,
            offset: 0,
            total_length: u16::try_from(bytes.len()).unwrap(),
        },
        payload: bytes,
    }
}
fn encoded(request: HistoryRequest) -> Vec<u8> {
    let mut input = [0; 128];
    let n = request.encode(&mut input).unwrap();
    input[..n].to_vec()
}
fn run(handler: &mut Handler<'_>, request: HistoryRequest) -> Vec<u8> {
    let input = encoded(request);
    let mut out = [0; 512];
    let n = handler
        .dispatch(frame(request.opcode(), &input), &mut out)
        .unwrap();
    out[..n].to_vec()
}
fn handler<'a>(store: &'a mut Store, lane: &'a SharedDataLane) -> Handler<'a> {
    HistoryHandler::new(
        ModuleEntry {
            module: 0x10,
            major: 1,
            minor: 2,
            availability: 1,
            features: 0,
            input_max: 512,
            output_max: 512,
        },
        store,
        &Access,
        lane,
        128,
        true,
    )
    .unwrap()
}

#[test]
fn test_should_persist_only_phone_requested_ack_after_actual_record_admission_and_reopen() {
    let clock = Clock(Rc::new(Cell::new(0)));
    let mut store = initialized(TestMedium::blank(), clock.clone());
    assert_eq!(store.append(input(&[60, 0, 0, 1])).unwrap(), 1);
    assert_eq!(store.append(input(&[61, 0, 0, 1])).unwrap(), 2);
    let lane = SharedDataLane::new(NonZeroU64::new(1).unwrap());
    {
        let mut service = handler(&mut store, &lane);
        assert_eq!(
            &run(&mut service, HistoryRequest::Open(filter()))[..4],
            &[0; 4]
        );
        assert_eq!(
            run(
                &mut service,
                HistoryRequest::Ack {
                    snapshot: 1,
                    epoch: 3,
                    upto_id: 2
                }
            ),
            [4, 0, 0, 0]
        );
        let request = HistoryRequest::Read {
            snapshot: 1,
            after_id: 0,
            max_records: 2,
        };
        let response = run(&mut service, request);
        assert_eq!(&response[..4], &[0; 4]);
        let stream = u16::from_le_bytes(response[4..6].try_into().unwrap());
        service
            .response_admitted(frame(request.opcode(), &encoded(request)), &response)
            .unwrap();
        service.grant(DataKind::History, stream, 2).unwrap();
        let mut out = [0; 128];
        let first = service.next_data(&mut out).unwrap().unwrap();
        // A prepared/sent event is not a phone ACK, nor even complete queue proof.
        assert_eq!(
            run(
                &mut service,
                HistoryRequest::Ack {
                    snapshot: 1,
                    epoch: 3,
                    upto_id: 2
                }
            ),
            [4, 0, 0, 0]
        );
        service.finish_data(first.message_id).unwrap();
        assert_eq!(
            run(
                &mut service,
                HistoryRequest::Ack {
                    snapshot: 1,
                    epoch: 3,
                    upto_id: 2
                }
            ),
            [4, 0, 0, 0]
        );
        let second = service.next_data(&mut out).unwrap().unwrap();
        service.finish_data(second.message_id).unwrap();
        assert_eq!(
            run(
                &mut service,
                HistoryRequest::Ack {
                    snapshot: 1,
                    epoch: 3,
                    upto_id: 2
                }
            ),
            [0, 0, 0, 0, 2, 0, 0, 0, 0, 0, 0, 0]
        );
    }
    let medium = store.into_snapshot().into_medium();
    let mut store = HistoryStore::<_, _, _, 1, 4, 16, 2048>::open(
        AtomicSnapshot::<_, 2048>::open(medium, layout()).unwrap(),
        Schema,
        clock,
        60,
    )
    .unwrap();
    assert_eq!(store.directory().unwrap().dataset, [7; 16]);
    assert_eq!(store.count(filter(), &Access).unwrap(), (3, 2));
    let lease = RecordStore::open(&mut store, 42, filter(), 0, &Access).unwrap();
    assert_eq!(lease.committed_id, 2);
    assert_eq!(
        store.ack_atomic(lease, filter(), 3, 2, 0, &Access).unwrap(),
        2
    );
    store.close(lease).unwrap();
    assert_eq!(store.append(input(&[62, 0, 0, 1])).unwrap(), 3);
}

#[test]
fn test_should_block_uncertain_ack_reads_then_recover_watermark_without_resurrecting_proof() {
    for barrier in 1..=3 {
        let clock = Clock(Rc::new(Cell::new(0)));
        let medium = TestMedium::blank();
        let control = medium.clone();
        let mut store = initialized(medium, clock);
        store.append(input(&[60, 0, 0, 1])).unwrap();
        let lease = RecordStore::open(&mut store, 1, filter(), 0, &Access).unwrap();
        store.record_admitted(lease, 1, &Access).unwrap();
        control.fail_sync.set(Some(control.syncs.get() + barrier));
        assert_eq!(
            store.ack_atomic(lease, filter(), 3, 1, 0, &Access),
            Err(HistoryFailure::Storage)
        );
        assert!(store.directory().is_err());
        store.recover().unwrap();
        assert!(store.record_admitted(lease, 1, &Access).is_err());
        let fresh = RecordStore::open(&mut store, 2, filter(), 0, &Access).unwrap();
        assert!(fresh.committed_id <= 1);
        if fresh.committed_id == 0 {
            assert!(store.ack_atomic(fresh, filter(), 3, 1, 0, &Access).is_err());
            store.record_admitted(fresh, 1, &Access).unwrap();
        }
        assert_eq!(
            store.ack_atomic(fresh, filter(), 3, 1, 0, &Access).unwrap(),
            1
        );
    }
}

#[test]
fn test_should_refuse_expired_direct_read_without_a_fresh_plan_timestamp() {
    let clock = Clock(Rc::new(Cell::new(0)));
    let mut store = initialized(TestMedium::blank(), clock.clone());
    store.append(input(&[60, 0, 0, 1])).unwrap();
    let lease = RecordStore::open(&mut store, 1, filter(), 0, &Access).unwrap();
    clock.0.set(60_000);
    assert_eq!(
        store.read(lease, 1, &mut [0; 16], &Access),
        Err(HistoryFailure::CursorExpired)
    );
    assert!(store.record_admitted(lease, 1, &Access).is_err());
}

#[test]
fn test_should_resume_wire_stream_above_durable_ack_after_restart_without_resending_old_records() {
    let clock = Clock(Rc::new(Cell::new(0)));
    let mut store = initialized(TestMedium::blank(), clock.clone());
    store.append(input(&[60, 0, 0, 1])).unwrap();
    store.append(input(&[61, 0, 0, 1])).unwrap();
    {
        let lane = SharedDataLane::new(NonZeroU64::new(1).unwrap());
        let mut service = handler(&mut store, &lane);
        assert_eq!(
            &run(&mut service, HistoryRequest::Open(filter()))[..4],
            &[0; 4]
        );
        read_one_and_ack(&mut service, 0, 1);
    }
    // Reconstruct both the persistent store and connection actor after losing
    // volatile proof; the next READ deliberately starts after durable ACK1.
    let medium = store.into_snapshot().into_medium();
    let mut store = HistoryStore::<_, _, _, 1, 4, 16, 2048>::open(
        AtomicSnapshot::<_, 2048>::open(medium, layout()).unwrap(),
        Schema,
        clock,
        60,
    )
    .unwrap();
    {
        let lane = SharedDataLane::new(NonZeroU64::new(2).unwrap());
        let mut service = handler(&mut store, &lane);
        assert_eq!(
            &run(&mut service, HistoryRequest::Open(filter()))[..4],
            &[0; 4]
        );
        read_one_and_ack(&mut service, 1, 2);
    }
    store.recover().unwrap();
    let lease = RecordStore::open(&mut store, 17, filter(), 0, &Access).unwrap();
    assert_eq!(lease.committed_id, 2);
}

fn read_one_and_ack(service: &mut Handler<'_>, after: u64, upto: u64) {
    let request = HistoryRequest::Read {
        snapshot: 1,
        after_id: after,
        max_records: 1,
    };
    let response = run(service, request);
    assert_eq!(&response[..4], &[0; 4]);
    let stream = u16::from_le_bytes(response[4..6].try_into().unwrap());
    service
        .response_admitted(frame(request.opcode(), &encoded(request)), &response)
        .unwrap();
    service.grant(DataKind::History, stream, 1).unwrap();
    let event = service.next_data(&mut [0; 128]).unwrap().unwrap();
    service.finish_data(event.message_id).unwrap();
    let response = run(
        service,
        HistoryRequest::Ack {
            snapshot: 1,
            epoch: 3,
            upto_id: upto,
        },
    );
    assert_eq!(&response[..4], &[0; 4]);
    assert_eq!(
        u64::from_le_bytes(response[4..12].try_into().unwrap()),
        upto
    );
}

// Catches treating a valid READ cursor as ACK coverage or invalidating a stream
// merely because an earlier unacknowledged record was deliberately not selected.
#[test]
fn test_should_read_after_unacknowledged_records_and_only_ack_after_filling_prefix() {
    let clock = Clock(Rc::new(Cell::new(0)));
    let mut store = initialized(TestMedium::blank(), clock);
    for value in [60, 61, 62] {
        store.append(input(&[value, 0, 0, 1])).unwrap();
    }
    {
        let lane = SharedDataLane::new(NonZeroU64::new(1).unwrap());
        let mut service = handler(&mut store, &lane);
        assert_eq!(
            &run(&mut service, HistoryRequest::Open(filter()))[..4],
            &[0; 4]
        );
        admit_one(&mut service, 2);
        assert_eq!(
            run(
                &mut service,
                HistoryRequest::Ack {
                    snapshot: 1,
                    epoch: 3,
                    upto_id: 3
                }
            ),
            [4, 0, 0, 0]
        );
        admit_one(&mut service, 0);
        assert_eq!(
            run(
                &mut service,
                HistoryRequest::Ack {
                    snapshot: 1,
                    epoch: 3,
                    upto_id: 3
                }
            ),
            [4, 0, 0, 0]
        );
        // Filling ID2 completes the actual prefix, retaining the prior ID3 proof.
        read_one_and_ack(&mut service, 1, 3);
    }
    store.recover().unwrap();
    let lease = RecordStore::open(&mut store, 8, filter(), 0, &Access).unwrap();
    assert_eq!(lease.committed_id, 3);
}

// Catches imposing cumulative-ACK ordering on a filtered read-only stream.
#[test]
fn test_should_deliver_filtered_wire_query_after_cursor_without_advancing_ack() {
    let clock = Clock(Rc::new(Cell::new(0)));
    let mut store = initialized(TestMedium::blank(), clock);
    for value in [60, 61, 62] {
        store.append(input(&[value, 0, 0, 1])).unwrap();
    }
    {
        let lane = SharedDataLane::new(NonZeroU64::new(1).unwrap());
        let mut service = handler(&mut store, &lane);
        let filtered = HistoryFilter {
            filter_flags: 2,
            record_type: 0x20,
            ..filter()
        };
        assert_eq!(
            &run(&mut service, HistoryRequest::OpenV2(filtered))[..4],
            &[0; 4]
        );
        admit_one(&mut service, 2);
        assert_ne!(
            &run(
                &mut service,
                HistoryRequest::Ack {
                    snapshot: 1,
                    epoch: 3,
                    upto_id: 3
                }
            )[..4],
            &[0; 4]
        );
    }
    store.recover().unwrap();
    let lease = RecordStore::open(&mut store, 8, filter(), 0, &Access).unwrap();
    assert_eq!(lease.committed_id, 0);
}

fn admit_one(service: &mut Handler<'_>, after: u64) {
    let request = HistoryRequest::Read {
        snapshot: 1,
        after_id: after,
        max_records: 1,
    };
    let response = run(service, request);
    assert_eq!(&response[..4], &[0; 4]);
    let stream = u16::from_le_bytes(response[4..6].try_into().unwrap());
    service
        .response_admitted(frame(request.opcode(), &encoded(request)), &response)
        .unwrap();
    service.grant(DataKind::History, stream, 1).unwrap();
    let event = service.next_data(&mut [0; 128]).unwrap().unwrap();
    service.finish_data(event.message_id).unwrap();
    assert!(service.next_data(&mut [0; 128]).unwrap().is_none());
    assert!(service.done(&mut [0; 64]).unwrap().is_some());
    service.done_admitted(stream).unwrap();
}
