//! Actual async history actor, shared EventPump and physical20-byte FIFO admission.
#![allow(
    clippy::unwrap_used,
    reason = "Test assertions fail loudly on invalid fixture."
)]
use super::*;
use crate::{
    history::{HistoryEventClock, OwnedHistoryEvents},
    history_support::{Clock, Control, Mem},
    schema::StrictSchema,
};
use core::{num::NonZeroU64, task::Poll};
use device_application::ServiceCell;
use device_connection::{
    BoundedControlConnection, EventPlane, EventPump, FirstFragmentQueue, FirstSlotOutcome,
    SharedControlChannel,
};
use device_history_service::{
    HistoryAccess, HistoryFailure, HistoryOperation, OwnedHistoryHandler,
};
use device_history_store::{
    AppendRecord, DomainConfig,
    async_paged::{AsyncPagedHistoryStore, JobResult, JobToken, PreparedHistory},
    paged::LogConfig,
};
use device_protocol::{
    DataKind, Frame, Header, HistoryFilter, HistoryRequest, Kind, ModuleEntry, ProtocolLimits,
    SharedDataLane,
};
use device_services::{BuildProfile, Config, DeviceInfo, ServiceHandler};
use device_storage::{incremental::IncrementalSnapshot, persistence::Layout};
use std::{vec, vec::Vec};
type Store = AsyncPagedHistoryStore<Mem, Mem, StrictSchema, Clock, 1, 16, 474, 144>;
type Actor<'a> = OwnedHistoryHandler<
    'a,
    PreparedHistory<'a, Mem, Mem, StrictSchema, Clock, 1, 16, 474, 144>,
    1,
    474,
>;
struct Access;
impl HistoryAccess for Access {
    fn module_allowed(&self) -> bool {
        true
    }
    fn allowed(&self, _: [u8; 16], _: Option<u8>, _: HistoryOperation) -> bool {
        true
    }
}
impl HistoryEventClock for Clock {
    fn now_ms(&self) -> Result<u64, HistoryFailure> {
        device_history_store::HistoryClock::now_ms(self)
    }
}
fn settle(s: &Store, token: JobToken<'_>) -> JobResult {
    for _ in 0..10000 {
        if let Poll::Ready(result) = s.result(token).unwrap() {
            return result.unwrap();
        }
        s.step().unwrap();
    }
    panic!("history job stalled")
}
fn metadata(module: u8) -> ModuleEntry {
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
fn request(
    control: &mut BoundedControlConnection<'_, 128>,
    handler: &mut impl ServiceHandler,
    s: &Store,
    bridge: &Bridge<1, 1>,
    epoch: u64,
    route: (u8, u8, u16),
    body: &[u8],
) -> Vec<u8> {
    let (module, opcode, id) = route;
    let mut frame = [0; 20];
    let mut offset = 0;
    loop {
        let end = (offset + 10).min(body.len());
        let f = Frame {
            header: Header {
                kind: Kind::Request,
                module,
                opcode,
                message_id: id,
                offset: offset as u16,
                total_length: body.len() as u16,
            },
            payload: &body[offset..end],
        };
        let len = f.encode(&mut frame).unwrap();
        control.receive(&frame[..len], 100, handler).unwrap();
        if end == body.len() {
            break;
        }
        offset = end;
    }
    let mut response = Vec::new();
    for _ in 0..10000 {
        s.step().unwrap();
        if control.waiting_for_handler() {
            control.poll_pending(100, handler).unwrap();
        }
        if control.response_pending() {
            let scope = bridge.scope().unwrap().unwrap();
            let mut q = bridge.queue(scope).unwrap();
            let admitted = q
                .with_reserved_slot(|bytes| control.next_frame(handler, bytes).map(|len| (len, ())))
                .unwrap();
            if matches!(admitted, FirstSlotOutcome::Admitted(())) {
                control.admit_frame(handler).unwrap();
            }
            drop(q);
            if let Some(frame) = bridge.outgoing(epoch).unwrap() {
                assert_eq!(frame.plane, EventPlane::Control);
                let f = Frame::decode(&frame.bytes[..frame.len], 20, 128).unwrap();
                response.extend_from_slice(f.payload);
            }
        }
        if !response.is_empty() && !control.response_pending() && !control.waiting_for_handler() {
            return response;
        }
    }
    panic!("CONTROL stalled")
}
fn history_request(
    control: &mut BoundedControlConnection<'_, 128>,
    handler: &mut impl ServiceHandler,
    s: &Store,
    bridge: &Bridge<1, 1>,
    epoch: u64,
    id: u16,
    r: HistoryRequest,
) -> Vec<u8> {
    let mut body = [0; 28];
    let n = r.encode(&mut body).unwrap();
    request(
        control,
        handler,
        s,
        bridge,
        epoch,
        (0x10, r.opcode(), id),
        &body[..n],
    )
}
#[test]
fn test_should_reserve_first_real_data_fifo_and_allow_durable_ack_only_after_actual_last_fragment()
{
    history_batch(1);
}
#[test]
fn test_should_admit_all_sixteen_records_before_full_prefix_ack_is_durable() {
    history_batch(16);
}
fn history_batch(count: u16) {
    let meta = Control::new(8192, 256, 4096);
    let log = Control::new(32768, 256, 4096);
    let clock = Clock::new();
    let s = Store::new_unrecovered(
        IncrementalSnapshot::new_unrecovered(
            meta.medium(),
            Layout {
                start: 0,
                bank_bytes: 4096,
            },
        )
        .unwrap(),
        log.medium(),
        StrictSchema,
        clock.clone(),
        LogConfig {
            namespace: [9; 16],
            log_start: 0x202000,
            slots_per_domain: 17,
        },
        60,
    )
    .unwrap();
    assert_eq!(
        settle(&s, s.begin_recover().unwrap()),
        JobResult::Recovered { initialized: false }
    );
    assert_eq!(
        settle(
            &s,
            s.begin_initialize(
                [3; 16],
                &[DomainConfig {
                    domain: 1,
                    epoch: 8
                }]
            )
            .unwrap()
        ),
        JobResult::Initialized
    );
    let body = [1, 0, 0, 0, 1, 80, 0];
    for id in 1..=count {
        assert_eq!(
            settle(
                &s,
                s.begin_append(AppendRecord {
                    domain: 1,
                    record_type: 0x0021,
                    schema: 1,
                    utc_ms: 0,
                    flags: 0,
                    parent_session: 0,
                    payload: &body
                })
                .unwrap()
            ),
            JobResult::Appended { id: u64::from(id) }
        );
    }
    let lane = SharedDataLane::new(NonZeroU64::new(1).unwrap());
    let actor: Actor<'_> = OwnedHistoryHandler::new(
        metadata(0x10),
        s.adapter().unwrap(),
        &Access,
        &lane,
        512,
        true,
    )
    .unwrap();
    let actor = ServiceCell::new(actor);
    let mut handler = actor.handle();
    let bridge = Bridge::<1, 1>::new().unwrap();
    let epoch = bridge.begin().unwrap();
    bridge.subscription(epoch, true).unwrap();
    bridge.data_subscription(epoch, true).unwrap();
    let scope = bridge.scope().unwrap().unwrap();
    let entries = [metadata(0), metadata(0x10)];
    let config = Config {
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
        modules: &entries,
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
    let channel = SharedControlChannel::new();
    let (mut control, token) =
        BoundedControlConnection::<128>::with_events(config, &channel).unwrap();
    control.notifications_enabled().unwrap();
    let source = OwnedHistoryEvents::new(&actor, &clock);
    let mut pump = EventPump::new(token, source).unwrap();
    assert_eq!(
        &request(
            &mut control,
            &mut handler,
            &s,
            &bridge,
            epoch,
            (0, 1, 1),
            &[1, 1, 128, 0, 0, 2, 3, 0]
        )[..4],
        &[0; 4]
    );
    let filter = HistoryFilter {
        domain: 1,
        filter_flags: 0,
        start_utc_ms: 0,
        end_utc_ms: 0,
        record_type: 0,
        parent_session: 0,
    };
    assert_eq!(
        &history_request(
            &mut control,
            &mut handler,
            &s,
            &bridge,
            epoch,
            2,
            HistoryRequest::OpenV2(filter)
        )[..4],
        &[0; 4]
    );
    let response = history_request(
        &mut control,
        &mut handler,
        &s,
        &bridge,
        epoch,
        3,
        HistoryRequest::Read {
            snapshot: 1,
            after_id: 0,
            max_records: count,
        },
    );
    let stream = u16::from_le_bytes(response[4..6].try_into().unwrap());
    actor
        .with_mut(|h| h.grant(DataKind::History, stream, u32::from(count)))
        .unwrap()
        .unwrap();
    for _ in 0..1000 {
        s.step().unwrap();
        if actor
            .with_mut(|h| h.pending_event())
            .unwrap()
            .unwrap()
            .is_some()
        {
            break;
        }
    }
    // Saturate the actual physical FIFO. Pump prefetch must not allocate a DATA ID/credit here.
    let mut queue = bridge.event_queue(scope, EventPlane::Data).unwrap();
    assert!(matches!(
        queue
            .with_reserved_slot(|out| {
                out[0] = 99;
                Ok::<_, ()>((1, ()))
            })
            .unwrap(),
        FirstSlotOutcome::Admitted(())
    ));
    assert!(!pump.flush(100, EventPlane::Data, &mut queue).unwrap());
    drop(queue);
    assert!(
        !actor
            .with(|h| h.event_valid(
                device_history_service::HistoryPendingEvent::Record { stream_id: stream },
                Some(1)
            ))
            .unwrap()
    );
    assert!(
        actor
            .with(|h| h.grant_replay_valid(stream, u32::from(count)))
            .unwrap()
    );
    let _ = bridge.outgoing(epoch).unwrap();
    let mut encoded = Vec::new();
    let mut fragments = 0;
    let mut received = 0u16;
    let mut attempts = 0;
    loop {
        attempts += 1;
        assert!(attempts < 10000, "asynchronous batch stalled");
        s.step().unwrap();
        let mut q = bridge.event_queue(scope, EventPlane::Data).unwrap();
        let flushed = pump.flush(100, EventPlane::Data, &mut q).unwrap();
        drop(q);
        if !flushed {
            continue;
        }
        let frame = bridge.outgoing(epoch).unwrap().unwrap();
        assert_eq!(frame.plane, EventPlane::Data);
        assert!(frame.len <= 20);
        let f = Frame::decode(&frame.bytes[..frame.len], 20, 512).unwrap();
        assert_eq!(f.header.message_id, received + 1);
        encoded.extend_from_slice(f.payload);
        fragments += 1;
        if !pump.pending() {
            let record = device_protocol::HistoryRecord::decode(&encoded, 512).unwrap();
            assert_eq!(record.record_id, u64::from(received + 1));
            assert_eq!(record.payload, &body);
            received += 1;
            if received == count {
                break;
            }
            encoded.clear();
        }
        if fragments == 1 {
            let committed_before = meta.durable.borrow().clone();
            let rejected = history_request(
                &mut control,
                &mut handler,
                &s,
                &bridge,
                epoch,
                4,
                HistoryRequest::Ack {
                    snapshot: 1,
                    epoch: 8,
                    upto_id: u64::from(count),
                },
            );
            // Depending on worker progress, reject at the busy worker or at
            // the unproven-prefix guard. Neither may publish a durable ACK.
            assert!(rejected == [4, 0, 0, 0] || rejected == [6, 0, 0, 0]);
            assert_eq!(*meta.durable.borrow(), committed_before);
        }
    }
    assert!(fragments > 1);
    assert_eq!(
        device_protocol::HistoryRecord::decode(&encoded, 512)
            .unwrap()
            .payload,
        &body
    );
    assert_eq!(
        history_request(
            &mut control,
            &mut handler,
            &s,
            &bridge,
            epoch,
            5,
            HistoryRequest::Ack {
                snapshot: 1,
                epoch: 8,
                upto_id: u64::from(count)
            }
        ),
        [vec![0; 4], u64::from(count).to_le_bytes().to_vec()].concat()
    );
    // Independently recover the actual committed HPGMET01 bytes and inspect its durable ACK floor.
    let mut disk = IncrementalSnapshot::<_, 144>::new_unrecovered(
        meta.medium(),
        Layout {
            start: 0,
            bank_bytes: 4096,
        },
    )
    .unwrap();
    disk.begin_recover().unwrap();
    for _ in 0..1000 {
        if let Poll::Ready(result) = disk.poll_recover() {
            result.unwrap();
            break;
        }
    }
    let mut persisted = [0; 144];
    let snap = disk.load_committed(&mut persisted).unwrap().unwrap();
    assert_eq!(snap.length, 144);
    assert_eq!(&persisted[..8], b"HPGMET01");
    assert_eq!(
        u64::from_le_bytes(persisted[96..104].try_into().unwrap()),
        u64::from(count)
    );
}
