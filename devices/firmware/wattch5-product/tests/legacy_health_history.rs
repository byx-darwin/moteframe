#![cfg(feature = "legacy-health")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "../../../crates/device-history-store/tests/async_paged_support/mod.rs"]
mod medium;
use core::task::Poll;
use device_health::{JobManager, Latest};
use device_health::{ResultInfo, ResultSource, Timestamp};
use device_health_service::{Error, HealthImport, HealthImportPort, ImportKey, LatestStore};
use device_health_service::{HealthHandler, ImportHealthHandler, SharedHealthOwner};
use device_history_service::{HistoryAccess, Preparation, RecordStore};
use device_history_store::{
    DomainConfig, HistoryClock,
    async_paged::{JobResult, JobToken},
    paged::LogConfig,
};
use device_protocol::HistoryFilter;
use device_services::ServiceHandler;
use device_storage::{incremental::IncrementalSnapshot, persistence::Layout};
use medium::{Clock, Control};
use wattch5_product::runtime::legacy_health::{
    HealthHistory, HealthLog, HealthSchema, measurement,
};
use wattch5_product::runtime::legacy_health::{LegacyBackend, LegacyProduct};
use wattch5_product::runtime::{ObservationSource, legacy_health::LegacyObservations};
type History = HealthHistory<medium::Mem, medium::Mem, Clock>;
fn owner(meta: &Control, log: &Control, clock: Clock) -> History {
    HealthHistory::new_unrecovered(
        IncrementalSnapshot::new_unrecovered(
            meta.medium(),
            Layout {
                start: 0,
                bank_bytes: 4096,
            },
        )
        .unwrap(),
        log.medium(),
        HealthSchema,
        clock,
        LogConfig {
            namespace: [9; 16],
            log_start: 0x0039_d000,
            slots_per_domain: 259,
        },
        300,
    )
    .unwrap()
}
fn finish(
    h: &History,
    t: JobToken<'_>,
) -> Result<JobResult, device_history_service::HistoryFailure> {
    for _ in 0..100_000 {
        h.step()?;
        if let Poll::Ready(r) = h.result(t)? {
            return r;
        }
    }
    panic!("bounded fixture stalled")
}
fn initialize(h: &History) {
    assert_eq!(
        finish(h, h.begin_recover().unwrap()).unwrap(),
        JobResult::Recovered { initialized: false }
    );
    assert_eq!(
        finish(
            h,
            h.begin_initialize(
                [7; 16],
                &[
                    DomainConfig {
                        domain: 3,
                        epoch: 1
                    },
                    DomainConfig {
                        domain: 4,
                        epoch: 1
                    },
                    DomainConfig {
                        domain: 8,
                        epoch: 1
                    }
                ]
            )
            .unwrap()
        )
        .unwrap(),
        JobResult::Initialized
    );
}
fn imports() -> [(u8, Vec<u8>); 3] {
    [
        (0x25, vec![118, 0, 78, 0, 60, 0, 7, 0]),
        (0x26, vec![0x48, 0x26, 0, 0, 1, 0]),
        (0x27, vec![0x58, 2, 1]),
    ]
}
fn import_wire(body: &[u8]) -> Vec<u8> {
    let mut b = vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    b.extend(body);
    b
}
#[test]
fn test_should_commit_source_once_then_restore_latest_ui_and_history() {
    let meta = Control::new(0x2000, 256, 4096);
    let log = Control::new(0x62000, 256, 4096);
    let clock = Clock::new();
    let h = owner(&meta, &log, clock.clone());
    initialize(&h);
    let producer = HealthLog::restore(&h, || clock.now_ms(), || {}).unwrap();
    for (n, (module, body)) in imports().into_iter().enumerate() {
        let bytes = import_wire(&body);
        let request = HealthImport::decode(module, &bytes).unwrap();
        let key = ImportKey {
            connection: 1,
            module,
            message: u16::try_from(n + 1).unwrap(),
        };
        let calls = meta.calls.get() + log.calls.get();
        assert!(!producer.prepare(key, request).unwrap());
        assert_eq!(calls, meta.calls.get() + log.calls.get());
        assert_eq!(producer.receipt(key, request), Err(Error::Busy));
        let mut latest = producer.latest();
        let mut output = [0; 9];
        assert!(
            latest
                .load(module, u16::from(module) * 256 + 1, &mut output)
                .is_err()
        );
        for _ in 0..10000 {
            producer.step().unwrap();
            if producer.prepare(key, request).unwrap() {
                break;
            }
        }
        assert_eq!(producer.receipt(key, request).unwrap(), 1);
        let stored = latest
            .load(module, u16::from(module) * 256 + 1, &mut output)
            .unwrap();
        assert_eq!(&output[..stored.info.length - 1], body);
        assert_eq!(output[stored.info.length - 1], 2);
        let imported = measurement(module, &output[..stored.info.length]).unwrap();
        let mut local = output;
        local[stored.info.length - 1] = if module == 0x27 { 3 } else { 1 };
        assert_eq!(
            imported,
            measurement(module, &local[..stored.info.length]).unwrap()
        );
        let info = ResultInfo {
            sample_id: 1,
            record_type: stored.info.record_type,
            schema: 2,
            timestamp: Timestamp::unknown(),
            length: stored.info.length,
            source: ResultSource::DeviceAlgorithm,
        };
        assert_eq!(
            latest.publish(module, 1, info, &local[..info.length]),
            Err(Error::Busy)
        );
        for _ in 0..10000 {
            producer.step().unwrap();
            if latest
                .publish(module, 1, info, &local[..info.length])
                .is_ok()
            {
                break;
            }
        }
        assert!(
            producer.current(key, 1),
            "later local values must not erase an App import receipt"
        );
    }

    verify_history(&h, &producer);
    drop(h);
    log.power_loss();
    meta.power_loss();
    let reopened = owner(&meta, &log, clock.clone());
    assert_eq!(
        finish(&reopened, reopened.begin_recover().unwrap()).unwrap(),
        JobResult::Recovered { initialized: true }
    );
    let restored = HealthLog::restore(&reopened, || clock.now_ms(), || {}).unwrap();
    for (module, _) in imports() {
        let mut b = [0; 9];
        let r = restored
            .latest()
            .load(module, u16::from(module) * 256 + 1, &mut b)
            .unwrap();
        assert_eq!(r.generation, 2);
        assert_eq!(b[r.info.length - 1], if module == 0x27 { 3 } else { 1 });
    }
}
#[test]
fn test_should_drive_history_read_preparation_without_an_active_import() {
    use device_history_service::{Preparation, RecordStore};
    use device_protocol::HistoryFilter;
    let meta = Control::new(0x2000, 256, 4096);
    let log = Control::new(0x62000, 256, 4096);
    let clock = Clock::new();
    let h = owner(&meta, &log, clock.clone());
    initialize(&h);
    let producer = HealthLog::restore(&h, || clock.now_ms(), || {}).unwrap();
    let mut reader = h.adapter().unwrap();
    let filter = HistoryFilter {
        domain: 3,
        filter_flags: 0,
        start_utc_ms: 0,
        end_utc_ms: 0,
        record_type: 0,
        parent_session: 0,
    };
    let request = Preparation::Count { filter };
    let mut ready = false;
    for _ in 0..10000 {
        if reader
            .prepare(request, 100, &medium::Access::yes())
            .unwrap()
        {
            ready = true;
            break;
        }
        producer.step().unwrap();
    }
    assert!(
        ready,
        "the sole health background worker must also drive history reads/ACKs"
    );
    assert_eq!(
        reader.count(filter, &medium::Access::yes()).unwrap(),
        (1, 0)
    );
}

fn drive<R: RecordStore>(
    producer: &HealthLog<'_, medium::Mem, medium::Mem, Clock>,
    reader: &mut R,
    request: Preparation,
    access: &dyn HistoryAccess,
) {
    for _ in 0..100_000 {
        if reader.prepare(request, 100, access).unwrap() {
            return;
        }
        producer.step().unwrap();
    }
    panic!("history preparation stalled");
}
struct Inputs;
impl wattch5_product::runtime::legacy_health::LegacyInputSource for Inputs {
    fn inputs(
        &self,
        _: u8,
    ) -> Result<wattch5_product::runtime::legacy_health::LegacyInputs, device_health::Error> {
        Ok(wattch5_product::runtime::legacy_health::LegacyInputs {
            heart_rate: Some(75),
            age: 30,
            male: true,
            reference_bp: None,
            hour: Some(6),
            clock_ticks: Some(0),
        })
    }
    fn timestamp(&self) -> Timestamp {
        Timestamp::unknown()
    }
}
struct Allowed(std::cell::Cell<bool>);
impl device_health_service::HealthAccess for Allowed {
    fn allowed(&self, _: u8, _: device_health_service::Operation, _: Option<u32>) -> bool {
        self.0.get()
    }
}
struct ServiceClock;
impl device_health_service::HealthClock for ServiceClock {
    fn now_ms(&self) -> Result<u64, Error> {
        Ok(100)
    }
}
fn frame(module: u8, op: u8, id: u16, body: &[u8]) -> device_protocol::Frame<'_> {
    device_protocol::Frame {
        header: device_protocol::Header {
            kind: device_protocol::Kind::Request,
            module,
            opcode: op,
            message_id: id,
            offset: 0,
            total_length: u16::try_from(body.len()).unwrap(),
        },
        payload: body,
    }
}
fn fixture_health_entries() -> [device_protocol::ModuleEntry; 3] {
    [0x25, 0x26, 0x27].map(|module| device_protocol::ModuleEntry {
        module,
        major: 1,
        minor: 0,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    })
}
fn fixture_history_handler<'a>(
    h: &'a History,
    access: &'a dyn HistoryAccess,
    lane: &'a device_protocol::SharedDataLane,
) -> impl ServiceHandler + 'a {
    device_history_service::OwnedHistoryHandler::<_, 3, 16>::new(
        device_protocol::ModuleEntry {
            module: 0x10,
            major: 1,
            minor: 2,
            availability: 1,
            features: 0,
            input_max: 128,
            output_max: 512,
        },
        h.adapter().unwrap(),
        access,
        lane,
        512,
        true,
    )
    .unwrap()
}
#[test]
fn test_should_use_real_health_handler_and_durable_import_before_latest_or_events() {
    let meta = Control::new(0x2000, 256, 4096);
    let log = Control::new(0x62000, 256, 4096);
    let clock = Clock::new();
    let h = owner(&meta, &log, clock.clone());
    initialize(&h);
    let producer = HealthLog::restore(&h, || clock.now_ms(), || {}).unwrap();
    let auth = Allowed(std::cell::Cell::new(true));
    let product = LegacyProduct::new(&Inputs, &producer);
    let backend = SharedHealthOwner::new(
        JobManager::<3>::new(1).unwrap(),
        LegacyBackend::new(&Inputs),
    );
    let mut latest = producer.latest();
    let entries = fixture_health_entries();
    let inner = HealthHandler::new(
        &backend,
        1,
        &entries,
        &auth,
        &ServiceClock,
        &product,
        &mut latest,
        None,
    )
    .unwrap();
    let mut service = ImportHealthHandler::new(inner, 1, &producer, &auth).unwrap();
    // Native composition keeps its history actor alive while C33 commits into the same owner.
    let history_access = medium::Access::yes();
    let lane = device_protocol::SharedDataLane::new(core::num::NonZeroU64::new(77).unwrap());
    let mut history = fixture_history_handler(&h, &history_access, &lane);
    let mut response = [0; 128];
    for (i, (module, value)) in imports().into_iter().enumerate() {
        let id = u16::try_from(i * 3 + 1).unwrap();
        let wire = import_wire(&value);
        let request = frame(module, 0x33, id, &wire);
        assert!(!service.response_ready(request).unwrap());
        let n = service.dispatch(request, &mut response).unwrap();
        assert_eq!(n, 4);
        assert_ne!(&response[..2], &[0, 0]);
        for _ in 0..10000 {
            producer.step().unwrap();
            history.advance(100).unwrap();
            assert!(!history.must_end_session());
            if service.response_ready(request).unwrap() {
                break;
            }
        }
        let n = service.dispatch(request, &mut response).unwrap();
        assert_eq!(n, 12);
        assert_eq!(&response[..4], &[0, 0, 0, 0]);
        assert_eq!(&response[4..12], &1u64.to_le_bytes());
        assert!(service.replay_valid(request, &response[..n]));
        service.response_admitted(request, &response[..n]).unwrap();
        let n = service
            .dispatch(frame(module, 2, id + 1, &[]), &mut response)
            .unwrap();
        assert_eq!(&response[..4], &[0, 0, 0, 0]);
        let l = Latest::decode(module, &response[4..n]).unwrap();
        assert_eq!(l.schema(), 2);
        assert_eq!(l.payload().last(), Some(&2));
        let start = [1, 1, module, 0, 0, 0, 0, 0];
        let request = frame(module, 0x30, id + 2, &start);
        let n = service.dispatch(request, &mut response).unwrap();
        assert_eq!(&response[..4], &[0, 0, 0, 0]);
        assert_eq!(n, 8);
        let job = u32::from_le_bytes(response[4..8].try_into().unwrap());
        service.response_admitted(request, &response[..n]).unwrap();
        let mut event = [0; 128];
        let mut staged = None;
        for _ in 0..10000 {
            match service
                .inner_mut()
                .prepare_result(module, job, &mut event, 128)
            {
                Ok(Some(v)) => {
                    staged = Some(v);
                    break;
                }
                Ok(None) | Err(Error::Busy) => producer.step().unwrap(),
                other => panic!("{other:?}"),
            }
        }
        let (token, n) = staged.expect("real persisted result event");
        let l = Latest::decode(module, &event[..n]).unwrap();
        assert_eq!(l.schema(), 2);
        assert_eq!(
            l.payload().last(),
            Some(&if module == 0x27 { 3 } else { 1 })
        );
        service.inner_mut().confirm_event(token).unwrap();
        service.inner_mut().poll_job(module, job).unwrap();
    }
    auth.0.set(false);
    let b = import_wire(&[0x58, 2, 1]);
    let request = frame(0x27, 0x33, 30, &b);
    assert!(!service.authorized(request));
    assert!(service.response_ready(request).is_err());
    service.connection_closed().unwrap();
}
#[test]
fn test_should_join_existing_six_domains_with_three_health_domains_without_lease_aliasing() {
    use device_history_service::JoinedHistory;
    let meta = Control::new(0x2000, 256, 4096);
    let log = Control::new(0x62000, 256, 4096);
    let clock = Clock::new();
    let health = owner(&meta, &log, clock.clone());
    initialize(&health);
    finish(
        &health,
        health
            .begin_append(device_history_store::AppendRecord {
                domain: 8,
                record_type: 0x2701,
                schema: 2,
                utc_ms: 0,
                flags: 0,
                parent_session: 0,
                payload: &[0x58, 2, 1, 2],
            })
            .unwrap(),
    )
    .unwrap();
    let old_meta = Control::new(0x2000, 256, 4096);
    let old_log = Control::new(0x0018_5000, 256, 4096);
    let old = medium::owner::<6, 255, 474, 464>(&old_meta, &old_log, clock, 259);
    medium::run(&old, old.begin_recover().unwrap(), &[&old_meta, &old_log]).unwrap();
    medium::run(
        &old,
        old.begin_initialize(
            [7; 16],
            &[0, 1, 2, 5, 6, 7].map(|domain| DomainConfig { domain, epoch: 1 }),
        )
        .unwrap(),
        &[&old_meta, &old_log],
    )
    .unwrap();
    let mut a = old.adapter().unwrap();
    let mut b = health.adapter().unwrap();
    let mut joined = JoinedHistory::<9>::new(&mut a, &mut b).unwrap();
    assert_eq!(
        joined
            .directory()
            .unwrap()
            .domains
            .iter()
            .map(|d| d.domain)
            .collect::<Vec<_>>(),
        vec![0, 1, 2, 3, 4, 5, 6, 7, 8]
    );
    for domain in [0, 3] {
        let filter = HistoryFilter {
            domain,
            filter_flags: 0,
            start_utc_ms: 0,
            end_utc_ms: 0,
            record_type: 0,
            parent_session: 0,
        };
        let p = Preparation::Open {
            snapshot: 1,
            filter,
        };
        for _ in 0..100_000 {
            if joined.prepare(p, 100, &medium::Access::yes()).unwrap() {
                break;
            }
            old.step().unwrap();
            health.step().unwrap();
        }
        let lease = joined.open(1, filter, 100, &medium::Access::yes()).unwrap();
        if domain == 0 {
            assert_eq!(lease.handle & 1, 0);
        } else {
            assert_eq!(lease.handle & 1, 1);
        }
        assert_close_replays(&mut joined, lease);
        assert!(
            joined
                .plan(lease, 0, 1, 100, &medium::Access::yes())
                .is_err()
        );
    }
    log.fail.set(Some(log.calls.get()));
    let filter = HistoryFilter {
        domain: 8,
        filter_flags: 0,
        start_utc_ms: 0,
        end_utc_ms: 0,
        record_type: 0,
        parent_session: 0,
    };
    let token = health
        .begin_prepare(Preparation::Count { filter }, 100, &medium::Access::yes())
        .unwrap();
    assert!(finish(&health, token).is_err());
    assert!(
        joined.directory().is_err(),
        "a pinned union must not hide either child failure"
    );
}
#[test]
fn test_should_never_publish_torn_imports_and_recover_only_committed_results() {
    for cut in [0, 80, 88, 255, 256, 300, 511] {
        let meta = Control::new(0x2000, 256, 4096);
        let log = Control::new(0x62000, 256, 4096);
        let clock = Clock::new();
        let h = owner(&meta, &log, clock.clone());
        initialize(&h);
        let producer = HealthLog::restore(&h, || clock.now_ms(), || {}).unwrap();
        let b = import_wire(&[118, 0, 78, 0, 60, 0, 7, 0]);
        let r = HealthImport::decode(0x25, &b).unwrap();
        let key = ImportKey {
            connection: 1,
            module: 0x25,
            message: 1,
        };
        log.cut.set(Some(cut));
        assert!(!producer.prepare(key, r).unwrap());
        let mut failed = false;
        for _ in 0..10000 {
            if producer.step().is_err() {
                failed = true;
                break;
            }
        }
        assert!(failed, "cut{cut}");
        assert!(producer.receipt(key, r).is_err());
        assert!(producer.latest().load(0x25, 0x2501, &mut [0; 9]).is_err());
        drop(h);
        log.cut.set(None);
        log.power_loss();
        meta.power_loss();
        let recovered = owner(&meta, &log, clock.clone());
        finish(&recovered, recovered.begin_recover().unwrap()).unwrap();
        let rebuilt = HealthLog::restore(&recovered, || clock.now_ms(), || {}).unwrap();
        assert!(rebuilt.latest().load(0x25, 0x2501, &mut [0; 9]).is_err());
    }
}
#[test]
fn test_should_include_origin_in_storage_integrity_and_keep_committing_after_disconnect() {
    let meta = Control::new(0x2000, 256, 4096);
    let log = Control::new(0x62000, 256, 4096);
    let clock = Clock::new();
    let h = owner(&meta, &log, clock.clone());
    initialize(&h);
    let producer = HealthLog::restore(&h, || clock.now_ms(), || {}).unwrap();
    let b = import_wire(&[118, 0, 78, 0, 60, 0, 7, 0]);
    let r = HealthImport::decode(0x25, &b).unwrap();
    let key = ImportKey {
        connection: 1,
        module: 0x25,
        message: 1,
    };
    assert!(!producer.prepare(key, r).unwrap());
    producer.detach(key);
    for _ in 0..10000 {
        producer.step().unwrap();
        if producer.current(key, 1) {
            break;
        }
    }
    assert_eq!(producer.receipt(key, r).unwrap(), 1);
    drop(h);
    log.bytes.borrow_mut()[88] ^= 3;
    log.durable.borrow_mut()[88] ^= 3;
    let corrupt = owner(&meta, &log, clock);
    assert!(finish(&corrupt, corrupt.begin_recover().unwrap()).is_err());
}

fn verify_history(h: &History, producer: &HealthLog<'_, medium::Mem, medium::Mem, Clock>) {
    let mut reader = h.adapter().unwrap();
    let access = medium::Access::yes();
    for domain in [3, 4, 8] {
        let filter = HistoryFilter {
            domain,
            filter_flags: 0,
            start_utc_ms: 0,
            end_utc_ms: 0,
            record_type: 0,
            parent_session: 0,
        };
        let snapshot = u32::from(domain) + 1;
        drive(
            producer,
            &mut reader,
            Preparation::Open { snapshot, filter },
            &access,
        );
        let lease = reader.open(snapshot, filter, 100, &access).unwrap();
        assert_eq!(lease.snapshot.count, 2);
        drive(
            producer,
            &mut reader,
            Preparation::Plan {
                lease,
                after_id: 0,
                max: 16,
            },
            &access,
        );
        let plan = reader.plan(lease, 0, 16, 100, &access).unwrap();
        assert_eq!(&plan.ids[..2], &[1, 2]);
        for id in [1, 2] {
            drive(
                producer,
                &mut reader,
                Preparation::Record { lease, id },
                &access,
            );
            let mut b = [0; 9];
            let record = reader.read(lease, id, &mut b, &access).unwrap();
            assert_eq!(record.schema, 2);
            assert_eq!(
                b[record.length - 1],
                if id == 1 {
                    2
                } else if domain == 8 {
                    3
                } else {
                    1
                }
            );
            let stored = device_storage::StoredRecord {
                domain,
                epoch: record.epoch,
                record_id: id,
                record_type: record.record_type,
                schema: record.schema,
                utc_ms: record.utc_ms,
                flags: record.flags,
                payload: &b[..record.length],
            };
            stored.verify(record.commit, record.stored_crc).unwrap();
            reader.validate_record(lease, stored).unwrap();
            reader.record_admitted(lease, id, &access).unwrap();
            while !reader.retire_record(lease, id, &access).unwrap() {
                producer.step().unwrap();
            }
        }
        drive(
            producer,
            &mut reader,
            Preparation::Ack {
                lease,
                filter,
                epoch: 1,
                upto: 2,
            },
            &access,
        );
        assert_eq!(
            reader
                .ack_atomic(lease, filter, 1, 2, 100, &access)
                .unwrap(),
            2
        );
        reader.close(lease).unwrap();
    }
}

#[test]
fn test_should_block_success_on_final_barrier_failure_even_when_recovery_finds_the_record() {
    let meta = Control::new(0x2000, 256, 4096);
    let log = Control::new(0x62000, 256, 4096);
    let clock = Clock::new();
    let h = owner(&meta, &log, clock.clone());
    initialize(&h);
    let producer = HealthLog::restore(&h, || clock.now_ms(), || {}).unwrap();
    let b = import_wire(&[118, 0, 78, 0, 60, 0, 7, 0]);
    let r = HealthImport::decode(0x25, &b).unwrap();
    let key = ImportKey {
        connection: 1,
        module: 0x25,
        message: 1,
    };
    assert!(!producer.prepare(key, r).unwrap());
    let mut armed = false;
    let mut failed = false;
    for _ in 0..10000 {
        if producer.step().is_err() {
            failed = true;
            break;
        }
        if !armed && log.durable.borrow()[256..272].iter().any(|x| *x != 255) {
            log.fail.set(Some(log.calls.get()));
            armed = true;
        }
    }
    assert!(armed && failed);
    assert!(producer.receipt(key, r).is_err());
    assert!(producer.latest().load(0x25, 0x2501, &mut [0; 9]).is_err());
    drop(h);
    log.fail.set(None);
    log.power_loss();
    meta.power_loss();
    let recovered = owner(&meta, &log, clock.clone());
    finish(&recovered, recovered.begin_recover().unwrap()).unwrap();
    let rebuilt = HealthLog::restore(&recovered, || clock.now_ms(), || {}).unwrap();
    let mut bytes = [0; 9];
    assert_eq!(
        rebuilt
            .latest()
            .load(0x25, 0x2501, &mut bytes)
            .unwrap()
            .generation,
        1
    );
    assert_eq!(bytes[8], 2);
}
#[test]
fn test_should_commit_page_actions_through_the_same_history_without_source_labels() {
    use wattch5_parity_ui_demo::{MeasureKind, MeasureState, UiRequest};
    use wattch5_product::runtime::{Actions, legacy_health::LegacyActions};
    let meta = Control::new(0x2000, 256, 4096);
    let log = Control::new(0x62000, 256, 4096);
    let clock = Clock::new();
    let h = owner(&meta, &log, clock.clone());
    initialize(&h);
    let producer = HealthLog::restore(&h, || clock.now_ms(), || {}).unwrap();
    let mut actions = LegacyActions::new(&Inputs, &producer);
    for kind in [
        MeasureKind::Pressure,
        MeasureKind::Oxygen,
        MeasureKind::Glucose,
    ] {
        actions.perform(UiRequest::MeasureStart(kind)).unwrap();
        assert_eq!(producer.page(kind).unwrap().state, MeasureState::Pending);
        assert!(actions.perform(UiRequest::MeasureStart(kind)).is_err());
        for _ in 0..10000 {
            producer.step().unwrap();
            if producer.page(kind).unwrap().state == MeasureState::Result {
                break;
            }
        }
        assert_eq!(producer.page(kind).unwrap().state, MeasureState::Result);
        let mut observations = LegacyObservations::new(
            BaseObservation(std::rc::Rc::new(std::cell::Cell::new(1))),
            &producer,
        );
        observations
            .capture()
            .unwrap()
            .snapshot
            .validate()
            .expect("all three results must be accepted by the actual UI snapshot contract");
    }
    assert!(
        actions
            .perform(UiRequest::MeasureStart(MeasureKind::Heart))
            .is_err()
    );
}

#[test]
fn test_should_reject_typed_empty_imports_and_mutated_pending_requests() {
    let meta = Control::new(0x2000, 256, 4096);
    let log = Control::new(0x62000, 256, 4096);
    let clock = Clock::new();
    let h = owner(&meta, &log, clock.clone());
    initialize(&h);
    let producer = HealthLog::restore(&h, || clock.now_ms(), || {}).unwrap();
    let key = ImportKey {
        connection: 1,
        module: 0x27,
        message: 1,
    };
    let absent = HealthImport {
        module: 0x27,
        timestamp: Timestamp::unknown(),
        value: &[0, 0, 0],
    };
    assert_eq!(producer.prepare(key, absent), Err(Error::Invalid));
    let request = HealthImport {
        value: &[0x58, 2, 1],
        ..absent
    };
    assert!(!producer.prepare(key, request).unwrap());
    assert_eq!(
        producer.prepare(
            key,
            HealthImport {
                value: &[0xbc, 2, 1],
                ..request
            }
        ),
        Err(Error::Invalid)
    );
}

#[test]
fn test_should_invalidate_cached_success_when_the_shared_history_owner_fails() {
    let meta = Control::new(0x2000, 256, 4096);
    let log = Control::new(0x62000, 256, 4096);
    let clock = Clock::new();
    let h = owner(&meta, &log, clock.clone());
    initialize(&h);
    let producer = HealthLog::restore(&h, || clock.now_ms(), || {}).unwrap();
    let key = ImportKey {
        connection: 1,
        module: 0x27,
        message: 1,
    };
    let bytes = import_wire(&[0x58, 2, 1]);
    let request = HealthImport::decode(0x27, &bytes).unwrap();
    producer.prepare(key, request).unwrap();
    for _ in 0..10000 {
        producer.step().unwrap();
        if producer.current(key, 1) {
            break;
        }
    }
    assert!(producer.current(key, 1));
    let mut observations = LegacyObservations::new(
        BaseObservation(std::rc::Rc::new(std::cell::Cell::new(1))),
        &producer,
    );
    let old_generation = observations.capture().unwrap().generation;
    let mut reader = h.adapter().unwrap();
    let preparation = Preparation::Count {
        filter: HistoryFilter {
            domain: 8,
            filter_flags: 0,
            start_utc_ms: 0,
            end_utc_ms: 0,
            record_type: 0,
            parent_session: 0,
        },
    };
    log.fail.set(Some(log.calls.get()));
    assert!(
        !reader
            .prepare(preparation, 100, &medium::Access::yes())
            .unwrap()
    );
    for _ in 0..100 {
        let _ = producer.step();
        if h.stats().is_err() {
            break;
        }
    }
    assert!(
        h.stats().is_err(),
        "actual history owner must have reached its storage failure"
    );
    assert!(!producer.available(0x27));
    assert!(!producer.current(key, 1));
    assert!(!producer.latest().current(0x27, 0x2701, 1));
    assert!(producer.latest().load(0x27, 0x2701, &mut [0; 9]).is_err());
    assert_eq!(
        producer
            .page(wattch5_parity_ui_demo::MeasureKind::Glucose)
            .unwrap()
            .state,
        wattch5_parity_ui_demo::MeasureState::Error
    );
    let observed = observations
        .capture()
        .expect("healthy base remains renderable after health storage fault");
    assert!(observed.generation > old_generation);
    assert_eq!(observed.snapshot.battery, 37);
    assert_eq!(
        observed
            .snapshot
            .measurement(wattch5_parity_ui_demo::MeasureKind::Glucose)
            .state,
        wattch5_parity_ui_demo::MeasureState::Error
    );
    verify_cache_requires_rebuild(&producer, &h, &log, &clock);
}

struct BaseObservation(std::rc::Rc<std::cell::Cell<u64>>);
impl wattch5_product::runtime::ObservationSource for BaseObservation {
    fn capture(
        &mut self,
    ) -> Result<wattch5_product::runtime::Observation<'_>, device_runtime::PortError> {
        if false {
            return Err(device_runtime::PortError::Unavailable);
        }
        // Full fixture, independent literal observations; no demo Snapshot::default.
        Ok(wattch5_product::runtime::Observation {
            generation: self.0.get(),
            snapshot: wattch5_parity_ui_demo::Snapshot {
                hour: 0,
                minute: 0,
                month: 1,
                day: 1,
                weekday: wattch5_parity_ui_demo::Weekday::Mon,
                battery: 37,
                measurements: [wattch5_parity_ui_demo::MeasurementSnapshot {
                    state: wattch5_parity_ui_demo::MeasureState::Unavailable,
                    reading: wattch5_parity_ui_demo::Reading::Missing,
                }; 6],
                heart_high: None,
                heart_low: None,
                activity_heart: None,
                steps: 23,
                distance: 1,
                calories: 2,
                sleep_total: None,
                sleep_deep: None,
                sleep_light: None,
                messages: &[],
                timer: wattch5_parity_ui_demo::TimerSnapshot {
                    seconds: 0,
                    state: wattch5_parity_ui_demo::TimerState::Unavailable,
                },
                stopwatch: wattch5_parity_ui_demo::StopwatchSnapshot {
                    centis: 0,
                    state: wattch5_parity_ui_demo::StopwatchState::Unavailable,
                    laps: &[],
                },
                sport: wattch5_parity_ui_demo::SportSnapshot {
                    phase: wattch5_parity_ui_demo::SportPhase::Done,
                    countdown: 0,
                    elapsed_seconds: 0,
                    distance: 0,
                    calories: 0,
                    heart: None,
                    locked: false,
                },
                weather: None,
                info: wattch5_parity_ui_demo::DeviceInfo {
                    model: None,
                    version: None,
                    address: None,
                },
                qr: None,
                applet_available: false,
                charging: 37,
                progress: 0,
            },
        })
    }
}
#[test]
fn test_should_overlay_only_health_pages_and_preserve_actual_base_observations() {
    use wattch5_parity_ui_demo::{MeasureKind, MeasureState, Reading, UiRequest};
    use wattch5_product::runtime::{
        Actions, ObservationSource,
        legacy_health::{LegacyActions, LegacyObservations},
    };
    let meta = Control::new(0x2000, 256, 4096);
    let log = Control::new(0x62000, 256, 4096);
    let clock = Clock::new();
    let h = owner(&meta, &log, clock.clone());
    initialize(&h);
    let producer = HealthLog::restore(&h, || clock.now_ms(), || {}).unwrap();
    let generation = std::rc::Rc::new(std::cell::Cell::new(5));
    let mut observations = LegacyObservations::new(BaseObservation(generation.clone()), &producer);
    let mut actions = LegacyActions::new(&Inputs, &producer);
    let initial = observations.capture().unwrap();
    assert_eq!((initial.snapshot.battery, initial.snapshot.steps), (37, 23));
    assert_eq!(
        initial.snapshot.measurement(MeasureKind::Heart).state,
        MeasureState::Unavailable
    );
    let first = initial.generation;
    assert_eq!(observations.capture().unwrap().generation, first);
    actions
        .perform(UiRequest::MeasureStart(MeasureKind::Pressure))
        .unwrap();
    let pending = observations.capture().unwrap();
    assert!(pending.generation > first);
    assert_eq!(
        pending.snapshot.measurement(MeasureKind::Pressure).state,
        MeasureState::Pending
    );
    actions
        .perform(UiRequest::MeasureStop(MeasureKind::Pressure))
        .unwrap();
    for _ in 0..10000 {
        producer.step().unwrap();
    }
    let stopped = observations.capture().unwrap();
    assert_eq!(
        stopped.snapshot.measurement(MeasureKind::Pressure).reading,
        Reading::Missing
    );
    // Stopping a page hides its value, but accepted persistence is never abandoned.
    assert_eq!(
        producer
            .latest()
            .load(0x25, 0x2501, &mut [0; 9])
            .unwrap()
            .generation,
        1
    );
    let key = ImportKey {
        connection: 1,
        module: 0x25,
        message: 1,
    };
    let bytes = import_wire(&[118, 0, 78, 0, 60, 0, 7, 0]);
    let request = HealthImport::decode(0x25, &bytes).unwrap();
    producer.prepare(key, request).unwrap();
    for _ in 0..10000 {
        producer.step().unwrap();
        if producer.current(key, 2) {
            break;
        }
    }
    let imported = observations.capture().unwrap();
    assert_eq!(
        imported.snapshot.measurement(MeasureKind::Pressure).state,
        MeasureState::Result
    );
    assert_eq!(
        (imported.snapshot.battery, imported.snapshot.steps),
        (37, 23)
    );
    let prior = imported.generation;
    generation.set(6);
    assert!(observations.capture().unwrap().generation > prior);
    generation.set(4);
    assert!(observations.capture().is_err());
    generation.set(0);
    assert!(observations.capture().is_err());
}

#[test]
fn test_should_recover_real_builder_metadata_with_the_actual_async_store_format() {
    let meta = Control::new(0x2000, 256, 4096);
    let image = include_bytes!("fixtures/legacy_health_metadata.bin");
    meta.bytes.borrow_mut().copy_from_slice(image);
    meta.durable.borrow_mut().copy_from_slice(image);
    let log = Control::new(0x62000, 256, 4096);
    let clock = Clock::new();
    let namespace = [
        0x13, 0x71, 0xfa, 0x67, 0xc9, 0x48, 0x48, 0x77, 0x9a, 0x85, 0x44, 0xa0, 0x81, 0x12, 0x3e,
        0xa1,
    ];
    let dataset = [
        0x60, 0xa4, 0xc7, 0x7b, 0x99, 0x2f, 0x49, 0x36, 0xac, 0xce, 0xc7, 0x27, 0xfc, 0x77, 0x64,
        0x36,
    ];
    let h = HealthHistory::new_unrecovered(
        IncrementalSnapshot::new_unrecovered(
            meta.medium(),
            Layout {
                start: 0,
                bank_bytes: 4096,
            },
        )
        .unwrap(),
        log.medium(),
        HealthSchema,
        clock.clone(),
        LogConfig {
            namespace,
            log_start: 0x0039_d000,
            slots_per_domain: 259,
        },
        300,
    )
    .unwrap();
    assert_eq!(
        finish(&h, h.begin_recover().unwrap()).unwrap(),
        JobResult::Recovered { initialized: true }
    );
    let reader = h.adapter().unwrap();
    assert_eq!(reader.directory().unwrap().dataset, dataset);
    assert_eq!(
        reader
            .directory()
            .unwrap()
            .domains
            .iter()
            .map(|d| (d.domain, d.epoch))
            .collect::<Vec<_>>(),
        vec![(3, 1), (4, 1), (8, 1)]
    );
    let producer = HealthLog::restore(&h, || clock.now_ms(), || {}).unwrap();
    assert!(producer.available(0x25));
    assert!(producer.latest().load(0x25, 0x2501, &mut [0; 9]).is_err());
}

fn protocol_session(entries: &[device_protocol::ModuleEntry]) -> device_services::Session<'_> {
    let mut session = device_services::Session::new(device_services::Config {
        session: 1,
        capability_rev: 1,
        profile: device_services::BuildProfile::Commercial,
        info: device_services::DeviceInfo {
            product_id: 5,
            fw_major: 0,
            fw_minor: 1,
            fw_patch: 0,
            build_id: 1,
        },
        modules: entries,
        limits: device_protocol::ProtocolLimits {
            rx_frame_max: 20,
            tx_frame_max: 20,
            control_max: 128,
            data_max: 0,
            idle_ms: 2000,
            fragment_ms: 100,
            response_ms: 10000,
        },
    })
    .unwrap();
    session.control_notifications_enabled().unwrap();
    session
        .dispatch(frame(0, 1, 1, &[1, 1, 128, 0, 0, 0, 3, 0]), &mut [0; 128])
        .unwrap();
    assert_eq!(session.state(), device_services::SessionState::Active);
    session
}
#[test]
fn test_should_keep_import_defer_replay_mutation_and_revocation_inside_the_real_session() {
    for scenario in 0..4 {
        verify_import_session(scenario);
    }
}
fn verify_import_session(scenario: u8) {
    use device_services::{SessionError, SessionState};
    let meta = Control::new(0x2000, 256, 4096);
    let log = Control::new(0x62000, 256, 4096);
    let clock = Clock::new();
    let h = owner(&meta, &log, clock.clone());
    initialize(&h);
    let producer = HealthLog::restore(&h, || clock.now_ms(), || {}).unwrap();
    let auth = Allowed(std::cell::Cell::new(true));
    let product = LegacyProduct::new(&Inputs, &producer);
    let backend = SharedHealthOwner::new(
        JobManager::<3>::new(1).unwrap(),
        LegacyBackend::new(&Inputs),
    );
    let mut latest = producer.latest();
    let entries = import_entries();
    let inner = HealthHandler::new(
        &backend,
        1,
        &entries[1..],
        &auth,
        &ServiceClock,
        &product,
        &mut latest,
        None,
    )
    .unwrap();
    let mut service = ImportHealthHandler::new(inner, 1, &producer, &auth).unwrap();
    let mut session = protocol_session(&entries);
    let bytes = import_wire(&[0x58, 2, 1]);
    let request = frame(0x27, 0x33, 2, &bytes);
    let mut out = [0xa7; 128];
    let calls = meta.calls.get() + log.calls.get();
    assert_eq!(
        session.dispatch_with_handler(request, &mut service, &mut out),
        Err(SessionError::Deferred),
        "wire status {:?}",
        &out[..4]
    );
    assert_eq!(meta.calls.get() + log.calls.get(), calls);
    assert_eq!(out, [0; 128]);
    if scenario == 1 {
        let mut changed = bytes.clone();
        changed[11] += 1;
        assert_eq!(
            session.dispatch_with_handler(frame(0x27, 0x33, 2, &changed), &mut service, &mut out),
            Err(SessionError::RequestConflict)
        );
        assert_eq!(session.state(), SessionState::Faulted);
    } else if scenario == 2 {
        auth.0.set(false);
        assert_eq!(
            session.dispatch_with_handler(request, &mut service, &mut out),
            Err(SessionError::AuthorizationRevoked)
        );
        assert_eq!(session.state(), SessionState::Closed);
    } else {
        finish_session_import(&mut session, &mut service, &producer, request, &mut out);
        assert_eq!(&out[..12], &[0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]);
        let calls = meta.calls.get() + log.calls.get();
        assert_eq!(
            session.dispatch_with_handler(request, &mut service, &mut out),
            Ok(12)
        );
        assert_eq!(meta.calls.get() + log.calls.get(), calls);
        if scenario == 3 {
            auth.0.set(false);
            assert_eq!(
                session.dispatch_with_handler(request, &mut service, &mut out),
                Err(SessionError::AuthorizationRevoked)
            );
        } else {
            let mut changed = bytes.clone();
            changed[11] += 1;
            assert_eq!(
                session.dispatch_with_handler(
                    frame(0x27, 0x33, 2, &changed),
                    &mut service,
                    &mut out
                ),
                Err(SessionError::RequestConflict)
            );
        }
    }
    service.connection_closed().unwrap();
    for _ in 0..10000 {
        producer.step().unwrap();
    }
    // A failed/mutated delivery never abandons an already accepted durable operation.
    let mut bytes = [0; 9];
    assert_eq!(
        producer
            .latest()
            .load(0x27, 0x2701, &mut bytes)
            .unwrap()
            .generation,
        1
    );
    assert_eq!(&bytes[..4], &[0x58, 2, 1, 2]);
    assert_eq!(h.stats().unwrap().retained, [0, 0, 1]);
}

fn finish_session_import(
    session: &mut device_services::Session<'_>,
    service: &mut dyn ServiceHandler,
    producer: &HealthLog<'_, medium::Mem, medium::Mem, Clock>,
    request: device_protocol::Frame<'_>,
    out: &mut [u8],
) {
    for _ in 0..10000 {
        producer.step().unwrap();
        match session.dispatch_with_handler(request, service, out) {
            Ok(n) => {
                assert_eq!(n, 12);
                return;
            }
            Err(device_services::SessionError::Deferred) => {}
            other => panic!("unexpected import completion {other:?}"),
        }
    }
    panic!("durable import did not reach Session admission");
}

fn verify_cache_requires_rebuild(
    producer: &HealthLog<'_, medium::Mem, medium::Mem, Clock>,
    h: &History,
    log: &Control,
    clock: &Clock,
) {
    log.fail.set(None);
    finish(h, h.begin_recover().unwrap()).unwrap();
    assert!(
        !producer.available(0x27),
        "recovering media must not silently revalidate an old cache"
    );
    let rebuilt = HealthLog::restore(h, || clock.now_ms(), || {}).unwrap();
    assert!(rebuilt.available(0x27));
    assert_eq!(
        rebuilt
            .latest()
            .load(0x27, 0x2701, &mut [0; 9])
            .unwrap()
            .generation,
        1
    );
}

fn import_entries() -> [device_protocol::ModuleEntry; 2] {
    [0, 0x27].map(|module| device_protocol::ModuleEntry {
        module,
        major: 1,
        minor: if module == 0 { 2 } else { 0 },
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    })
}

#[test]
fn test_should_preserve_full_imported_pressure_when_only_the_page_format_cannot_show_it() {
    use wattch5_parity_ui_demo::{MeasureKind, MeasureState, Reading};
    let meta = Control::new(0x2000, 256, 4096);
    let log = Control::new(0x62000, 256, 4096);
    let clock = Clock::new();
    let h = owner(&meta, &log, clock.clone());
    initialize(&h);
    let producer = HealthLog::restore(&h, || clock.now_ms(), || {}).unwrap();
    let key = ImportKey {
        connection: 1,
        module: 0x25,
        message: 1,
    };
    let bytes = import_wire(&[0xff, 0xff, 80, 0, 0, 0, 3, 0]);
    let request = HealthImport::decode(0x25, &bytes).unwrap();
    producer.prepare(key, request).unwrap();
    for _ in 0..10000 {
        producer.step().unwrap();
        if producer.current(key, 1) {
            break;
        }
    }
    assert_eq!(producer.receipt(key, request).unwrap(), 1);
    let mut stored = [0; 9];
    producer.latest().load(0x25, 0x2501, &mut stored).unwrap();
    assert_eq!(stored, [0xff, 0xff, 80, 0, 0, 0, 3, 0, 2]);
    let mut observations = LegacyObservations::new(
        BaseObservation(std::rc::Rc::new(std::cell::Cell::new(1))),
        &producer,
    );
    let observation = observations.capture().unwrap();
    observation.snapshot.validate().unwrap();
    assert_eq!(
        observation
            .snapshot
            .measurement(MeasureKind::Pressure)
            .state,
        MeasureState::Error
    );
    assert_eq!(
        observation
            .snapshot
            .measurement(MeasureKind::Pressure)
            .reading,
        Reading::Missing
    );
    assert_eq!(observation.snapshot.battery, 37);
}

fn assert_close_replays(store: &mut impl RecordStore, lease: device_history_service::StoreLease) {
    let mut forged = lease;
    forged.snapshot.epoch += 1;
    assert!(store.close(forged).is_err());
    store.close(lease).unwrap();
    store.close(lease).unwrap();
    assert!(store.close(forged).is_err());
}
