//! Offline integration of the actual retained history source, async NOR and 20-byte admission.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "../../../wattch5/crates/device-history-store/tests/async_paged_support/mod.rs"]
mod medium;

use core::{cell::Cell, num::NonZeroU64, task::Poll};
use device_application::{ResourceRouter, ServiceCell};
use device_connection::{
    BoundedControlConnection, EventPlane, EventPump, FirstFragmentQueue, FirstSlotError,
    FirstSlotOutcome, PumpFailure, SharedControlChannel,
};
use device_history_service::{
    HistoryAccess, HistoryFailure, HistoryOperation, HistoryPendingEvent, OwnedHistoryHandler,
    RecordStore,
};
use device_history_store::{
    AppendRecord, DomainConfig, HistoryClock, HistorySchema,
    async_paged::{AsyncPagedHistoryStore, JobResult, JobToken},
    paged::LogConfig,
};
use device_protocol::{
    Frame, Header, HistoryFilter, HistoryRecord, HistoryRequest, HistoryResponse, Kind,
    ModuleEntry, ProtocolLimits, SharedDataLane,
};
use device_services::{
    AccessSnapshot, AccessState, BuildProfile, Config, CoreResourceHandler, DeviceInfo,
    HandlerTable, ServiceHandler, TrustedAccessProvider,
};
use device_storage::{incremental::IncrementalSnapshot, persistence::Layout};
use medium::{Clock, Control, Mem};
use wattch5_candidate::history::{HistoryEventClock, OwnedHistoryEvents, OwnedHistoryPort};

impl HistoryEventClock for Clock {
    fn now_ms(&self) -> Result<u64, HistoryFailure> {
        HistoryClock::now_ms(self)
    }
}
struct Access(Cell<bool>);
impl HistoryAccess for Access {
    fn module_allowed(&self) -> bool {
        self.0.get()
    }
    fn allowed(&self, _: [u8; 16], _: Option<u8>, _: HistoryOperation) -> bool {
        self.0.get()
    }
}
impl TrustedAccessProvider for Access {
    fn access_snapshot(&self) -> AccessSnapshot {
        AccessSnapshot {
            state: AccessState::Authorized,
            reason: 0,
        }
    }
}

/// One physical notification slot. A full FIFO never invokes the fill callback.
#[derive(Default)]
struct Fifo {
    frame: Option<Vec<u8>>,
    admissions: usize,
}
impl FirstFragmentQueue for Fifo {
    fn with_reserved_slot<T, E>(
        &mut self,
        fill: impl FnOnce(&mut [u8]) -> Result<(usize, T), E>,
    ) -> Result<FirstSlotOutcome<T>, FirstSlotError<E>> {
        if self.frame.is_some() {
            return Ok(FirstSlotOutcome::NoSlot);
        }
        let mut slot = [0; 20];
        let (length, result) = fill(&mut slot).map_err(FirstSlotError::Fill)?;
        if !(1..=20).contains(&length) {
            return Err(FirstSlotError::Contract);
        }
        self.frame = Some(slot[..length].to_vec());
        self.admissions += 1;
        Ok(FirstSlotOutcome::Admitted(result))
    }
}
const fn metadata(module: u8) -> ModuleEntry {
    ModuleEntry {
        module,
        major: 1,
        minor: 2,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: if module == 0 { 128 } else { 512 },
    }
}
const ENTRIES: [ModuleEntry; 2] = [metadata(0), metadata(0x10)];
fn config() -> Config<'static> {
    Config {
        session: 1,
        capability_rev: 1,
        profile: BuildProfile::Development,
        info: DeviceInfo {
            product_id: 0,
            fw_major: 0,
            fw_minor: 1,
            fw_patch: 0,
            build_id: 1,
        },
        modules: &ENTRIES,
        limits: ProtocolLimits {
            rx_frame_max: 20,
            tx_frame_max: 20,
            control_max: 128,
            data_max: 512,
            idle_ms: 2000,
            fragment_ms: 100,
            response_ms: 10000,
        },
    }
}
fn submit(
    control: &mut BoundedControlConnection<'_, 128>,
    handler: &mut impl ServiceHandler,
    module: u8,
    opcode: u8,
    id: u16,
    body: &[u8],
) {
    for offset in (0..body.len().max(1)).step_by(10) {
        let end = (offset + 10).min(body.len());
        let mut bytes = [0; 20];
        let n = Frame {
            header: Header {
                kind: Kind::Request,
                module,
                opcode,
                message_id: id,
                offset: offset as u16,
                total_length: body.len() as u16,
            },
            payload: &body[offset..end],
        }
        .encode(&mut bytes)
        .unwrap();
        control.receive(&bytes[..n], 100, handler).unwrap();
    }
}
fn response(
    control: &mut BoundedControlConnection<'_, 128>,
    handler: &mut impl ServiceHandler,
    step: &impl Fn(),
) -> Vec<u8> {
    let mut body = Vec::new();
    let mut fifo = Fifo::default();
    for _ in 0..100_000 {
        step();
        if control.waiting_for_handler() {
            control.poll_pending(100, handler).unwrap();
        }
        if control.response_pending() {
            let admitted = fifo
                .with_reserved_slot(|out| control.next_frame(handler, out).map(|n| (n, ())))
                .unwrap();
            assert!(matches!(admitted, FirstSlotOutcome::Admitted(())));
            control.admit_frame(handler).unwrap();
            let bytes = fifo.frame.take().unwrap();
            let frame = Frame::decode(&bytes, 20, 128).unwrap();
            body.extend_from_slice(frame.payload);
        }
        if !body.is_empty() && !control.response_pending() && !control.waiting_for_handler() {
            return body;
        }
    }
    panic!("async control response stalled");
}
fn request(
    control: &mut BoundedControlConnection<'_, 128>,
    handler: &mut impl ServiceHandler,
    step: &impl Fn(),
    module: u8,
    opcode: u8,
    id: u16,
    body: &[u8],
) -> Vec<u8> {
    submit(control, handler, module, opcode, id, body);
    response(control, handler, step)
}
fn history(
    control: &mut BoundedControlConnection<'_, 128>,
    handler: &mut impl ServiceHandler,
    step: &impl Fn(),
    id: u16,
    r: HistoryRequest,
) -> Vec<u8> {
    let mut body = [0; 28];
    let n = r.encode(&mut body).unwrap();
    request(control, handler, step, 0x10, r.opcode(), id, &body[..n])
}
fn durable_ack<const N: usize>(meta: &Control, domain: u8) -> u64 {
    // Inspect only independently recovered durable bytes, never an actor's cached watermark.
    let disk = Control::new(meta.durable.borrow().len(), meta.w, meta.e);
    disk.bytes.borrow_mut().clone_from(&meta.durable.borrow());
    let mut snapshot = IncrementalSnapshot::<_, N>::new_unrecovered(
        disk.medium(),
        Layout {
            start: 0,
            bank_bytes: 4096,
        },
    )
    .unwrap();
    snapshot.begin_recover().unwrap();
    let mut recovered = false;
    for _ in 0..1000 {
        if let Poll::Ready(result) = snapshot.poll_recover() {
            result.unwrap();
            recovered = true;
            break;
        }
    }
    assert!(recovered);
    let mut bytes = [0; N];
    let saved = snapshot.load_committed(&mut bytes).unwrap().unwrap();
    assert_eq!(&bytes[..8], b"HPGMET01");
    for entry in bytes[80..saved.length].chunks_exact(64) {
        if entry[0] == domain {
            return u64::from_le_bytes(entry[16..24].try_into().unwrap());
        }
    }
    panic!("domain absent from recovered metadata");
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum End {
    Commit,
    PowerLossDuringAck,
    CancelThenDisconnect,
    Revoke,
}

/// Each peer command uses the real framed CONTROL path, including core GRANT through its port.
fn transfer<R: RecordStore, const D: usize, const N: usize>(
    adapter: R,
    clock: &Clock,
    meta: &Control,
    step: impl Fn(),
    record: AppendRecord<'_>,
    end: End,
) {
    let access = Access(Cell::new(true));
    let lane = SharedDataLane::new(NonZeroU64::new(1).unwrap());
    let actor = ServiceCell::new(
        OwnedHistoryHandler::<_, D, 474>::new(ENTRIES[1], adapter, &access, &lane, 512, true)
            .unwrap(),
    );
    let mut history_handle = actor.handle();
    let mut credits = OwnedHistoryPort::new(&actor);
    let mut dataset = OwnedHistoryPort::new(&actor);
    let mut resources = ResourceRouter::new(&access, &mut credits, &mut dataset);
    let mut core = CoreResourceHandler::new(ENTRIES[0], &mut resources).unwrap();
    let mut handles: [&mut dyn ServiceHandler; 2] = [&mut core, &mut history_handle];
    let mut handler = HandlerTable::new(&ENTRIES, &mut handles).unwrap();
    let channel = SharedControlChannel::new();
    let (mut control, token) =
        BoundedControlConnection::<128>::with_events(config(), &channel).unwrap();
    control.notifications_enabled().unwrap();
    let mut pump = EventPump::new_retained(token, OwnedHistoryEvents::new(&actor, clock)).unwrap();
    assert_eq!(
        &request(
            &mut control,
            &mut handler,
            &step,
            0,
            1,
            1,
            &[1, 1, 128, 0, 0, 2, 3, 0]
        )[..4],
        &[0; 4]
    );
    let opened = history(
        &mut control,
        &mut handler,
        &step,
        2,
        HistoryRequest::OpenV2(HistoryFilter {
            domain: record.domain,
            filter_flags: 0,
            start_utc_ms: 0,
            end_utc_ms: 0,
            record_type: 0,
            parent_session: 0,
        }),
    );
    let HistoryResponse::OpenV2(snapshot) = HistoryResponse::decode(6, &opened).unwrap() else {
        panic!("OPEN failed");
    };
    assert_eq!(
        (snapshot.first_id, snapshot.last_id, snapshot.count),
        (1, 1, 1)
    );
    let read = history(
        &mut control,
        &mut handler,
        &step,
        3,
        HistoryRequest::Read {
            snapshot: snapshot.snapshot,
            after_id: 0,
            max_records: 1,
        },
    );
    let HistoryResponse::Read(read) = HistoryResponse::decode(3, &read).unwrap() else {
        panic!("READ failed");
    };
    let mut grant = [1, 0, 0, 1, 0, 0, 0];
    grant[1..3].copy_from_slice(&read.stream_id.to_le_bytes());
    assert_eq!(
        &request(&mut control, &mut handler, &step, 0, 7, 4, &grant)[..4],
        &[0; 4]
    );
    let pending = HistoryPendingEvent::Record {
        stream_id: read.stream_id,
    };
    for _ in 0..10_000 {
        step();
        if actor.with_mut(|h| h.pending_event()).unwrap().unwrap() == Some(pending) {
            break;
        }
    }
    assert_eq!(
        actor.with_mut(|h| h.pending_event()).unwrap().unwrap(),
        Some(pending)
    );
    let durable_before = meta.durable.borrow().clone();
    let mut fifo = Fifo {
        frame: Some(vec![0xff]),
        ..Fifo::default()
    };
    assert!(!pump.flush(100, EventPlane::Data, &mut fifo).unwrap());
    assert!(!pump.pending());
    assert_eq!(fifo.admissions, 0);
    assert!(!actor.with(|h| h.event_valid(pending, Some(1))).unwrap());
    assert!(
        actor
            .with(|h| h.grant_replay_valid(read.stream_id, 1))
            .unwrap()
    );
    fifo.frame.take();
    let length = HistoryRecord::OVERHEAD + record.payload.len();
    let fragments = length.div_ceil(10);
    let mut body = Vec::new();
    let ack = HistoryRequest::Ack {
        snapshot: snapshot.snapshot,
        epoch: snapshot.epoch,
        upto_id: 1,
    };
    for index in 0..fragments {
        assert!(pump.flush(100, EventPlane::Data, &mut fifo).unwrap());
        assert_eq!(pump.pending(), index + 1 != fragments);
        if index + 1 < fragments {
            assert!(actor.with(|h| h.event_valid(pending, Some(1))).unwrap());
            // Keeping the admitted frame queued applies real callback backpressure.
            assert!(!pump.flush(100, EventPlane::Data, &mut fifo).unwrap());
            assert_eq!(fifo.admissions, index + 1);
        }
        if index == 0 || index + 2 == fragments {
            let denied = history(&mut control, &mut handler, &step, 10 + index as u16, ack);
            assert!(matches!(
                HistoryResponse::decode(4, &denied).unwrap(),
                HistoryResponse::Failure { .. }
            ));
            assert_eq!(*meta.durable.borrow(), durable_before);
            assert_eq!(durable_ack::<N>(meta, record.domain), 0);
        }
        let bytes = fifo.frame.take().unwrap();
        let frame = Frame::decode(&bytes, 20, 512).unwrap();
        assert_eq!(
            (frame.header.kind, frame.header.module, frame.header.opcode),
            (Kind::Event, 0x10, 0x80)
        );
        assert_eq!(
            (
                frame.header.message_id,
                usize::from(frame.header.total_length),
                usize::from(frame.header.offset)
            ),
            (1, length, body.len())
        );
        body.extend_from_slice(frame.payload);
        if end == End::CancelThenDisconnect {
            // CANCEL waits for the actual in-flight message barrier. Disconnect
            // while only one fragment exists must never manufacture that barrier.
            let cancel = HistoryRequest::CancelRead {
                snapshot: snapshot.snapshot,
                stream_id: read.stream_id,
            };
            let mut cancel_body = [0; 28];
            let n = cancel.encode(&mut cancel_body).unwrap();
            submit(
                &mut control,
                &mut handler,
                0x10,
                cancel.opcode(),
                90,
                &cancel_body[..n],
            );
            assert!(control.waiting_for_handler());
            assert!(!control.response_pending());
            fifo.frame = Some(vec![0xff]);
            assert!(!pump.flush(100, EventPlane::Data, &mut fifo).unwrap());
            assert_eq!(fifo.admissions, 1);
            assert!(pump.pending());
            assert_eq!(*meta.durable.borrow(), durable_before);
            pump.close().unwrap();
            assert!(channel.is_closed());
            assert!(!actor.with(|h| h.event_valid(pending, Some(1))).unwrap());
            assert!(actor.with(|h| h.with_data(1, |_| ())).unwrap().is_err());
            assert_eq!(durable_ack::<N>(meta, record.domain), 0);
            return;
        }
        if end == End::Revoke {
            access.0.set(false);
            fifo.frame = Some(vec![0xff]);
            assert_eq!(
                pump.flush(100, EventPlane::Data, &mut fifo)
                    .unwrap_err()
                    .failure,
                PumpFailure::AuthorizationRevoked
            );
            assert_eq!(fifo.admissions, 1);
            assert!(!actor.with(|h| h.event_valid(pending, Some(1))).unwrap());
            assert!(actor.with(|h| h.with_data(1, |_| ())).unwrap().is_err());
            assert_eq!(*meta.durable.borrow(), durable_before);
            assert_eq!(durable_ack::<N>(meta, record.domain), 0);
            return;
        }
    }
    assert_eq!(body.len(), length);
    assert_eq!(fifo.admissions, fragments);
    if record.payload.len() == 474 {
        assert_eq!((length, fragments), (512, 52));
    }
    let decoded = HistoryRecord::decode(&body, 512).unwrap();
    assert_eq!(
        (
            decoded.snapshot,
            decoded.stream_id,
            decoded.epoch,
            decoded.record_id
        ),
        (snapshot.snapshot, read.stream_id, snapshot.epoch, 1)
    );
    assert_eq!(
        (
            decoded.record_type,
            decoded.schema,
            decoded.utc_ms,
            decoded.flags,
            decoded.payload
        ),
        (
            record.record_type,
            record.schema,
            record.utc_ms,
            record.flags,
            record.payload
        )
    );
    assert!(!actor.with(|h| h.event_valid(pending, Some(1))).unwrap());
    assert!(actor.with(|h| h.with_data(1, |_| ())).unwrap().is_err());
    assert_eq!(
        *meta.durable.borrow(),
        durable_before,
        "FIFO admission must never persist peer ACK"
    );
    meta.buffered.set(true);
    let mut ack_body = [0; 28];
    let n = ack.encode(&mut ack_body).unwrap();
    submit(&mut control, &mut handler, 0x10, 4, 100, &ack_body[..n]);
    assert!(control.waiting_for_handler());
    let mut saw_uncommitted_program = false;
    for _ in 0..100_000 {
        step();
        if control.waiting_for_handler() {
            control.poll_pending(100, &mut handler).unwrap();
        }
        if *meta.bytes.borrow() != *meta.durable.borrow() {
            saw_uncommitted_program = true;
            assert!(
                !control.response_pending(),
                "ACK reply must wait for durable commit"
            );
            assert_eq!(durable_ack::<N>(meta, record.domain), 0);
            if end == End::PowerLossDuringAck {
                return;
            }
            break;
        }
    }
    assert!(
        saw_uncommitted_program,
        "fixture must exercise buffered metadata writes"
    );
    let committed = response(&mut control, &mut handler, &step);
    assert!(matches!(
        HistoryResponse::decode(4, &committed).unwrap(),
        HistoryResponse::Ack { committed_id: 1 }
    ));
    assert_eq!(durable_ack::<N>(meta, record.domain), 1);
    pump.close().unwrap();
}

type Store<V, const D: usize, const B: usize, const N: usize> =
    AsyncPagedHistoryStore<Mem, Mem, V, Clock, D, 255, B, N>;
fn owner<V: HistorySchema, const D: usize, const B: usize, const N: usize>(
    meta: &Control,
    log: &Control,
    clock: Clock,
    schema: V,
) -> Store<V, D, B, N> {
    Store::new_unrecovered(
        IncrementalSnapshot::new_unrecovered(
            meta.medium(),
            Layout {
                start: 0,
                bank_bytes: 4096,
            },
        )
        .unwrap(),
        log.medium(),
        schema,
        clock,
        LogConfig {
            namespace: [9; 16],
            log_start: 0x202000,
            slots_per_domain: 259,
        },
        60,
    )
    .unwrap()
}
fn finish<V: HistorySchema, const D: usize, const B: usize, const N: usize>(
    store: &Store<V, D, B, N>,
    token: JobToken<'_>,
) -> JobResult {
    for _ in 0..100_000 {
        if let Poll::Ready(result) = store.result(token).unwrap() {
            return result.unwrap();
        }
        store.step().unwrap();
    }
    panic!("NOR job stalled");
}
fn initialize<V: HistorySchema, const D: usize, const B: usize, const N: usize>(
    store: &Store<V, D, B, N>,
    domains: &[u8],
) {
    assert_eq!(
        finish(store, store.begin_recover().unwrap()),
        JobResult::Recovered { initialized: false }
    );
    let domains: Vec<_> = domains
        .iter()
        .map(|&domain| DomainConfig { domain, epoch: 8 })
        .collect();
    assert_eq!(
        finish(store, store.begin_initialize([3; 16], &domains).unwrap()),
        JobResult::Initialized
    );
}

#[test]
fn maximum_retained_record_needs_52_admissions_and_only_durable_ack_survives_power_loss() {
    for end in [
        End::Commit,
        End::PowerLossDuringAck,
        End::CancelThenDisconnect,
        End::Revoke,
    ] {
        let meta = Control::new(8192, 256, 4096);
        let log = Control::new(0x62000, 256, 4096);
        let clock = Clock::new();
        let store = owner::<_, 1, 474, 144>(&meta, &log, clock.clone(), medium::Schema);
        initialize(&store, &[1]);
        let body: Vec<_> = (0..474).map(|i| (i % 251) as u8).collect();
        let record = medium::input(&body, 1);
        assert_eq!(
            finish(&store, store.begin_append(record).unwrap()),
            JobResult::Appended { id: 1 }
        );
        transfer::<_, 1, 144>(
            store.adapter().unwrap(),
            &clock,
            &meta,
            || store.step().unwrap(),
            record,
            end,
        );
        drop(store);
        meta.power_loss();
        log.power_loss();
        let recovered = owner::<_, 1, 474, 144>(&meta, &log, clock, medium::Schema);
        assert_eq!(
            finish(&recovered, recovered.begin_recover().unwrap()),
            JobResult::Recovered { initialized: true }
        );
        assert_eq!(durable_ack::<144>(&meta, 1), u64::from(end == End::Commit));
        assert_eq!(recovered.stats().unwrap().retained, [1]);
    }
}

#[cfg(feature = "native-health")]
#[test]
fn strict_old_six_and_source_aware_health_three_domains_use_the_same_retained_joined_path() {
    use device_history_service::JoinedHistory;
    use wattch5_candidate::schema::StrictSchema;
    use wattch5_product::runtime::legacy_health::HealthSchema;
    let meta = Control::new(8192, 256, 4096);
    let log = Control::new(0x0018_5000, 256, 4096);
    let health_meta = Control::new(8192, 256, 4096);
    let health_log = Control::new(0x62000, 256, 4096);
    let clock = Clock::new();
    let old = owner::<_, 6, 474, 464>(&meta, &log, clock.clone(), StrictSchema);
    let health = owner::<_, 3, 16, 272>(&health_meta, &health_log, clock.clone(), HealthSchema);
    initialize(&old, &[0, 1, 2, 5, 6, 7]);
    initialize(&health, &[3, 4, 8]);
    let mut activity = vec![0; 31];
    activity[0] = 1;
    let mut exercise = activity.clone();
    exercise[0] = 2;
    let mut sport = vec![0; 46];
    sport[0] = 1;
    sport[4] = 1;
    sport[28] = 1;
    sport[31] = 1;
    let fixtures = [
        (0, 0x2301, 1, activity),
        (1, 0x0021, 1, vec![1, 0, 0, 0, 1, 80, 0]),
        (2, 0x0020, 1, vec![60, 0, 100, 1]),
        (3, 0x2501, 2, vec![120, 0, 80, 0, 60, 0, 7, 0, 2]),
        (4, 0x2601, 2, vec![0x48, 0x26, 0, 0, 1, 0, 2]),
        (5, 0x2301, 1, exercise),
        (6, 0x2201, 2, sport),
        (7, 0x2801, 1, vec![0x42, 0x0e, 3, 1]),
        (8, 0x2701, 2, vec![0x58, 2, 1, 2]),
    ];
    for (domain, kind, schema, body) in &fixtures {
        let record = AppendRecord {
            domain: *domain,
            record_type: *kind,
            schema: *schema,
            utc_ms: 0,
            flags: 0,
            parent_session: 0,
            payload: body,
        };
        let result = if [3, 4, 8].contains(domain) {
            finish(&health, health.begin_append(record).unwrap())
        } else {
            finish(&old, old.begin_append(record).unwrap())
        };
        assert_eq!(result, JobResult::Appended { id: 1 });
    }
    for (domain, kind, schema, body) in &fixtures {
        let mut left = old.adapter().unwrap();
        let mut right = health.adapter().unwrap();
        let joined = JoinedHistory::<9>::new(&mut left, &mut right).unwrap();
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
        let record = AppendRecord {
            domain: *domain,
            record_type: *kind,
            schema: *schema,
            utc_ms: 0,
            flags: 0,
            parent_session: 0,
            payload: body,
        };
        let step = || {
            old.step().unwrap();
            health.step().unwrap();
        };
        if [3, 4, 8].contains(domain) {
            transfer::<_, 9, 272>(joined, &clock, &health_meta, step, record, End::Commit);
        } else {
            transfer::<_, 9, 464>(joined, &clock, &meta, step, record, End::Commit);
        }
    }
    drop(old);
    drop(health);
    meta.power_loss();
    log.power_loss();
    health_meta.power_loss();
    health_log.power_loss();
    let old = owner::<_, 6, 474, 464>(&meta, &log, clock.clone(), StrictSchema);
    let health = owner::<_, 3, 16, 272>(&health_meta, &health_log, clock, HealthSchema);
    assert_eq!(
        finish(&old, old.begin_recover().unwrap()),
        JobResult::Recovered { initialized: true }
    );
    assert_eq!(
        finish(&health, health.begin_recover().unwrap()),
        JobResult::Recovered { initialized: true }
    );
    for domain in [0, 1, 2, 5, 6, 7] {
        assert_eq!(durable_ack::<464>(&meta, domain), 1);
    }
    for domain in [3, 4, 8] {
        assert_eq!(durable_ack::<272>(&health_meta, domain), 1);
    }
}

fn exclusive_owner<V: HistorySchema, const D: usize, const B: usize, const N: usize>(
    meta: &Control,
    log: &Control,
    clock: Clock,
    schema: V,
) -> Store<V, D, B, N> {
    Store::new_exclusive_unrecovered(
        (
            meta.medium(),
            Layout {
                start: 0,
                bank_bytes: 4096,
            },
        ),
        log.medium(),
        schema,
        clock,
        LogConfig {
            namespace: [9; 16],
            log_start: 0x202000,
            slots_per_domain: 259,
        },
        60,
    )
    .unwrap()
}

#[test]
fn exclusive_constructor_preserves_six_domain_flash_recovery_append_and_durable_ack() {
    use wattch5_candidate::schema::StrictSchema;
    let invalid_meta = Control::new(8192, 256, 4096);
    let untouched_log = Control::new(0x0018_5000, 256, 4096);
    let rejected = Store::<StrictSchema, 6, 474, 464>::new_exclusive_unrecovered(
        (
            invalid_meta.medium(),
            Layout {
                start: 0,
                bank_bytes: 0,
            },
        ),
        untouched_log.medium(),
        StrictSchema,
        Clock::new(),
        LogConfig {
            namespace: [9; 16],
            log_start: 0x202000,
            slots_per_domain: 259,
        },
        60,
    );
    assert!(matches!(rejected, Err(HistoryFailure::Invalid)));
    assert_eq!(
        invalid_meta.calls.get() + untouched_log.calls.get(),
        0,
        "invalid metadata layout must be rejected without physical I/O"
    );
    // The two-RAM-image legacy constructor and direct one-RAM-image constructor
    // must retain the exact same two-bank physical format and transaction bytes.
    for end in [End::Commit, End::PowerLossDuringAck] {
        let images = [false, true].map(|direct| {
            let meta = Control::new(8192, 256, 4096);
            let log = Control::new(0x0018_5000, 256, 4096);
            let clock = Clock::new();
            let open = |exclusive| {
                if exclusive {
                    exclusive_owner::<_, 6, 474, 464>(&meta, &log, clock.clone(), StrictSchema)
                } else {
                    owner::<_, 6, 474, 464>(&meta, &log, clock.clone(), StrictSchema)
                }
            };
            let store = open(direct);
            initialize(&store, &[0, 1, 2, 5, 6, 7]);
            let initialized = (meta.durable.borrow().clone(), log.durable.borrow().clone());
            drop(store);
            meta.power_loss();
            log.power_loss();
            let store = open(direct);
            assert_eq!(
                finish(&store, store.begin_recover().unwrap()),
                JobResult::Recovered { initialized: true }
            );
            let mut activity = vec![0; 31];
            activity[0] = 1;
            let mut exercise = activity.clone();
            exercise[0] = 2;
            let mut sport = vec![0; 46];
            sport[0] = 1;
            sport[4] = 1;
            sport[28] = 1;
            sport[31] = 1;
            let fixtures = [
                (0, 0x2301, 1, activity),
                (1, 0x0021, 1, vec![1, 0, 0, 0, 1, 80, 0]),
                (2, 0x0020, 1, vec![60, 0, 100, 1]),
                (5, 0x2301, 1, exercise),
                (6, 0x2201, 2, sport),
                (7, 0x2801, 1, vec![0x42, 0x0e, 3, 1]),
            ];
            for (domain, kind, schema, body) in &fixtures {
                let record = AppendRecord {
                    domain: *domain,
                    record_type: *kind,
                    schema: *schema,
                    utc_ms: 0,
                    flags: 0,
                    parent_session: 0,
                    payload: body,
                };
                assert_eq!(
                    finish(&store, store.begin_append(record).unwrap()),
                    JobResult::Appended { id: 1 }
                );
            }
            let appended = (meta.durable.borrow().clone(), log.durable.borrow().clone());
            for (domain, kind, schema, body) in &fixtures {
                let record = AppendRecord {
                    domain: *domain,
                    record_type: *kind,
                    schema: *schema,
                    utc_ms: 0,
                    flags: 0,
                    parent_session: 0,
                    payload: body,
                };
                transfer::<_, 6, 464>(
                    store.adapter().unwrap(),
                    &clock,
                    &meta,
                    || store.step().unwrap(),
                    record,
                    end,
                );
                if end == End::PowerLossDuringAck {
                    break;
                }
            }
            let pending = (
                meta.bytes.borrow().clone(),
                meta.durable.borrow().clone(),
                log.bytes.borrow().clone(),
                log.durable.borrow().clone(),
            );
            drop(store);
            meta.power_loss();
            log.power_loss();
            // Cross-open each layout with the other public constructor as well.
            let recovered = open(!direct);
            assert_eq!(
                finish(&recovered, recovered.begin_recover().unwrap()),
                JobResult::Recovered { initialized: true }
            );
            assert_eq!(recovered.stats().unwrap().retained, [1; 6]);
            for domain in [0, 1, 2, 5, 6, 7] {
                assert_eq!(
                    durable_ack::<464>(&meta, domain),
                    u64::from(end == End::Commit)
                );
            }
            let recovered_bytes = (meta.bytes.borrow().clone(), log.bytes.borrow().clone());
            (initialized, appended, pending, recovered_bytes)
        });
        assert_eq!(
            images[0], images[1],
            "constructors changed physical NOR transaction bytes"
        );
    }
}

#[cfg(feature = "native-health")]
#[test]
fn test_should_match_dynamic_joined_history_across_nine_domains_and_recovery() {
    use device_history_service::TypedJoinedHistory;
    use wattch5_candidate::schema::StrictSchema;
    use wattch5_product::runtime::legacy_health::HealthSchema;
    let meta = Control::new(8192, 256, 4096);
    let log = Control::new(0x0018_5000, 256, 4096);
    let health_meta = Control::new(8192, 256, 4096);
    let health_log = Control::new(0x62000, 256, 4096);
    let clock = Clock::new();
    let old = owner::<_, 6, 474, 464>(&meta, &log, clock.clone(), StrictSchema);
    let health = owner::<_, 3, 16, 272>(&health_meta, &health_log, clock.clone(), HealthSchema);
    initialize(&old, &[0, 1, 2, 5, 6, 7]);
    initialize(&health, &[3, 4, 8]);
    let mut activity = vec![0; 31];
    activity[0] = 1;
    let mut exercise = activity.clone();
    exercise[0] = 2;
    let mut sport = vec![0; 46];
    sport[0] = 1;
    sport[4] = 1;
    sport[28] = 1;
    sport[31] = 1;
    let fixtures = [
        (0, 0x2301, 1, activity),
        (1, 0x0021, 1, vec![1, 0, 0, 0, 1, 80, 0]),
        (2, 0x0020, 1, vec![60, 0, 100, 1]),
        (3, 0x2501, 2, vec![120, 0, 80, 0, 60, 0, 7, 0, 2]),
        (4, 0x2601, 2, vec![0x48, 0x26, 0, 0, 1, 0, 2]),
        (5, 0x2301, 1, exercise),
        (6, 0x2201, 2, sport),
        (7, 0x2801, 1, vec![0x42, 0x0e, 3, 1]),
        (8, 0x2701, 2, vec![0x58, 2, 1, 2]),
    ];
    for (domain, kind, schema, body) in &fixtures {
        let record = AppendRecord {
            domain: *domain,
            record_type: *kind,
            schema: *schema,
            utc_ms: 0,
            flags: 0,
            parent_session: 0,
            payload: body,
        };
        let result = if [3, 4, 8].contains(domain) {
            finish(&health, health.begin_append(record).unwrap())
        } else {
            finish(&old, old.begin_append(record).unwrap())
        };
        assert_eq!(result, JobResult::Appended { id: 1 });
    }
    for (domain, kind, schema, body) in &fixtures {
        let mut left = old.adapter().unwrap();
        let mut right = health.adapter().unwrap();
        let joined = TypedJoinedHistory::<_, _, 9>::new(&mut left, &mut right).unwrap();
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
        let record = AppendRecord {
            domain: *domain,
            record_type: *kind,
            schema: *schema,
            utc_ms: 0,
            flags: 0,
            parent_session: 0,
            payload: body,
        };
        let step = || {
            old.step().unwrap();
            health.step().unwrap();
        };
        if [3, 4, 8].contains(domain) {
            transfer::<_, 9, 272>(joined, &clock, &health_meta, step, record, End::Commit);
        } else {
            transfer::<_, 9, 464>(joined, &clock, &meta, step, record, End::Commit);
        }
    }
    drop(old);
    drop(health);
    meta.power_loss();
    log.power_loss();
    health_meta.power_loss();
    health_log.power_loss();
    let old = owner::<_, 6, 474, 464>(&meta, &log, clock.clone(), StrictSchema);
    let health = owner::<_, 3, 16, 272>(&health_meta, &health_log, clock, HealthSchema);
    assert_eq!(
        finish(&old, old.begin_recover().unwrap()),
        JobResult::Recovered { initialized: true }
    );
    assert_eq!(
        finish(&health, health.begin_recover().unwrap()),
        JobResult::Recovered { initialized: true }
    );
    for domain in [0, 1, 2, 5, 6, 7] {
        assert_eq!(durable_ack::<464>(&meta, domain), 1);
    }
    for domain in [3, 4, 8] {
        assert_eq!(durable_ack::<272>(&health_meta, domain), 1);
    }
}
