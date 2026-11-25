//! Actual Session deferred CONTROL plus the unchanged DATA actor over async storage.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod async_paged_support;
use async_paged_support::*;
use core::num::NonZeroU64;
use device_history_service::{HistoryPendingEvent, OwnedHistoryHandler};
use device_history_store::{
    DomainConfig,
    async_paged::{JobResult, PreparedHistory},
};
use device_protocol::{
    DataKind, Frame, Header, HistoryRecord, HistoryRequest, Kind, ModuleEntry, ProtocolLimits,
    SharedDataLane,
};
use device_services::{BuildProfile, Config, DeviceInfo, ServiceHandler, Session, SessionError};
fn metadata(module: u8) -> ModuleEntry {
    ModuleEntry {
        module,
        major: 1,
        minor: 2,
        availability: 1,
        features: 0,
        input_max: 512,
        output_max: 512,
    }
}
fn frame(opcode: u8, id: u16, payload: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module: 0x10,
            opcode,
            message_id: id,
            offset: 0,
            total_length: u16::try_from(payload.len()).unwrap(),
        },
        payload,
    }
}
fn session(modules: &[ModuleEntry]) -> Session<'_> {
    let mut s = Session::new(Config {
        session: 1,
        capability_rev: 1,
        profile: BuildProfile::Commercial,
        info: DeviceInfo {
            product_id: 0,
            fw_major: 0,
            fw_minor: 0,
            fw_patch: 0,
            build_id: 0,
        },
        modules,
        limits: ProtocolLimits {
            rx_frame_max: 20,
            tx_frame_max: 20,
            control_max: 512,
            data_max: 512,
            idle_ms: 2000,
            fragment_ms: 100,
            response_ms: 10_000,
        },
    })
    .unwrap();
    s.control_notifications_enabled().unwrap();
    let mut hello = frame(1, 1, &[1, 1, 0, 2, 0, 2, 3, 0]);
    hello.header.module = 0;
    s.dispatch(hello, &mut [0; 512]).unwrap();
    s
}
type Actor<'a> =
    OwnedHistoryHandler<'a, PreparedHistory<'a, Mem, Mem, Schema, Clock, 1, 4, 474, 144>, 1, 474>;
fn control(
    s: &Store<1, 4, 474, 144>,
    session: &mut Session<'_>,
    actor: &mut Actor<'_>,
    request: HistoryRequest,
    id: u16,
) -> Vec<u8> {
    let mut bytes = [0; 28];
    let n = request.encode(&mut bytes).unwrap();
    let f = frame(request.opcode(), id, &bytes[..n]);
    let mut out = [0x55; 512];
    for _ in 0..10_000 {
        match session.dispatch_with_handler(f, actor, &mut out) {
            Ok(n) => return out[..n].to_vec(),
            Err(SessionError::Deferred) => {
                assert_eq!(out, [0; 512]);
                s.step().unwrap();
            }
            result => panic!("unexpected CONTROL result {result:?}"),
        }
    }
    panic!("deferred CONTROL stalled")
}
#[test]
#[allow(clippy::too_many_lines)] // One end-to-end CONTROL/DATA lifecycle preserves its observable sequence.
fn test_should_defer_real_session_prepare_data_without_credit_and_ack_only_after_final_admission() {
    let meta = Control::new(8192, 256, 4096);
    let log = Control::new(8192, 256, 4096);
    let s: Store<1, 4, 474, 144> = owner(&meta, &log, Clock::new(), 5);
    initialize(
        &s,
        &meta,
        &log,
        &[DomainConfig {
            domain: 0,
            epoch: 8,
        }],
    );
    run(
        &s,
        s.begin_append(input(&[42; 474], 0)).unwrap(),
        &[&meta, &log],
    )
    .unwrap();
    let access = Access::yes();
    let lane = SharedDataLane::new(NonZeroU64::new(11).unwrap());
    let modules = [metadata(0), metadata(0x10)];
    let mut session = session(&modules);
    let mut actor = OwnedHistoryHandler::<_, 1, 474>::new(
        metadata(0x10),
        s.adapter().unwrap(),
        &access,
        &lane,
        512,
        true,
    )
    .unwrap();
    actor.advance(100).unwrap();
    assert_eq!(
        &control(
            &s,
            &mut session,
            &mut actor,
            HistoryRequest::OpenV2(filter(0)),
            2
        )[..4],
        &[0; 4]
    );
    let request = HistoryRequest::Read {
        snapshot: 1,
        after_id: 0,
        max_records: 1,
    };
    let response = control(&s, &mut session, &mut actor, request, 3);
    let stream = u16::from_le_bytes(response[4..6].try_into().unwrap());
    assert_eq!(actor.pending_event().unwrap(), None);
    assert_eq!(actor.next_data(&mut [0; 512]).unwrap(), None);
    let mut body = [0; 28];
    let n = request.encode(&mut body).unwrap();
    actor
        .response_admitted(frame(3, 3, &body[..n]), &response)
        .unwrap();
    assert_eq!(actor.pending_event().unwrap(), None);
    actor.grant(DataKind::History, stream, 1).unwrap();
    let calls = log.calls.get();
    for _ in 0..4 {
        assert_eq!(actor.pending_event().unwrap(), None);
    }
    assert_eq!(log.calls.get(), calls);
    assert_eq!(
        actor.next_data(&mut [0; 512]),
        Err(device_history_service::HistoryFailure::Busy)
    );
    for _ in 0..1000 {
        s.step().unwrap();
        if actor.pending_event().unwrap() == Some(HistoryPendingEvent::Record { stream_id: stream })
        {
            break;
        }
    }
    let mut data = [0; 512];
    let event = actor.next_data(&mut data).unwrap().unwrap();
    assert_eq!(event.message_id, 1);
    assert_eq!(event.length, 512);
    assert_eq!(
        HistoryRecord::decode(&data, 512).unwrap().payload,
        &[42; 474]
    );
    assert_eq!(
        {
            let mut retry = [0; 512];
            (actor.next_data(&mut retry).unwrap(), retry)
        },
        (Some(event), data)
    );
    let ack = HistoryRequest::Ack {
        snapshot: 1,
        epoch: 8,
        upto_id: 1,
    };
    assert_eq!(control(&s, &mut session, &mut actor, ack, 4), [4, 0, 0, 0]);
    actor.finish_data(event.message_id).unwrap();
    let reply = control(&s, &mut session, &mut actor, ack, 5);
    assert_eq!(reply, [0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]);
    let before = meta.calls.get() + log.calls.get();
    assert_eq!(control(&s, &mut session, &mut actor, ack, 5), reply);
    assert_eq!(before, meta.calls.get() + log.calls.get());
    actor.connection_closed().unwrap();
    drop(actor);
    assert_eq!(
        run(&s, s.begin_recover().unwrap(), &[&meta, &log]),
        Ok(JobResult::Recovered { initialized: true })
    );
}
#[test]
fn test_should_bind_full_frame_and_release_detached_open_when_actor_drops() {
    let meta = Control::new(8192, 256, 4096);
    let log = Control::new(8192, 256, 4096);
    let s: Store<1, 4, 474, 144> = owner(&meta, &log, Clock::new(), 5);
    initialize(
        &s,
        &meta,
        &log,
        &[DomainConfig {
            domain: 0,
            epoch: 8,
        }],
    );
    let access = Access::yes();
    let lane = SharedDataLane::new(NonZeroU64::new(12).unwrap());
    let mut actor = OwnedHistoryHandler::<_, 1, 474>::new(
        metadata(0x10),
        s.adapter().unwrap(),
        &access,
        &lane,
        512,
        true,
    )
    .unwrap();
    actor.advance(100).unwrap();
    let req = HistoryRequest::OpenV2(filter(0));
    let mut body = [0; 28];
    let n = req.encode(&mut body).unwrap();
    assert!(
        !actor
            .response_ready(frame(req.opcode(), 2, &body[..n]))
            .unwrap()
    );
    assert_eq!(
        actor.response_ready(frame(req.opcode(), 3, &body[..n])),
        Err(SessionError::InvalidHandlerResponse)
    );
    drop(actor);
    for _ in 0..100 {
        s.step().unwrap();
    }
    let mut fresh = s.adapter().unwrap();
    let request = device_history_service::Preparation::Open {
        snapshot: 2,
        filter: filter(0),
    };
    prepare(&s, &mut fresh, request, &access, &[&meta, &log]);
}
fn opened_actor<'a>(
    s: &'a Store<1, 4, 474, 144>,
    session: &mut Session<'_>,
    access: &'a Access,
    lane: &'a SharedDataLane,
) -> Actor<'a> {
    let mut actor = OwnedHistoryHandler::<_, 1, 474>::new(
        metadata(0x10),
        s.adapter().unwrap(),
        access,
        lane,
        512,
        true,
    )
    .unwrap();
    actor.advance(100).unwrap();
    control(s, session, &mut actor, HistoryRequest::OpenV2(filter(0)), 2);
    actor
}
fn admitted_read(
    s: &Store<1, 4, 474, 144>,
    session: &mut Session<'_>,
    actor: &mut Actor<'_>,
    id: u16,
) -> u16 {
    let request = HistoryRequest::Read {
        snapshot: 1,
        after_id: 0,
        max_records: 1,
    };
    let response = control(s, session, actor, request, id);
    assert_eq!(&response[..4], &[0; 4]);
    let stream = u16::from_le_bytes(response[4..6].try_into().unwrap());
    let mut bytes = [0; 28];
    let n = request.encode(&mut bytes).unwrap();
    actor
        .response_admitted(frame(3, id, &bytes[..n]), &response)
        .unwrap();
    actor.grant(DataKind::History, stream, 1).unwrap();
    stream
}
#[test]
fn test_should_retire_unstarted_prefetch_at_each_pending_or_ready_boundary_without_losing_lease() {
    for boundary in 0..4 {
        let meta = Control::new(8192, 256, 4096);
        let log = Control::new(8192, 256, 4096);
        let s: Store<1, 4, 474, 144> = owner(&meta, &log, Clock::new(), 5);
        initialize(
            &s,
            &meta,
            &log,
            &[DomainConfig {
                domain: 0,
                epoch: 8,
            }],
        );
        run(
            &s,
            s.begin_append(input(&[42; 474], 0)).unwrap(),
            &[&meta, &log],
        )
        .unwrap();
        let access = Access::yes();
        let lane = SharedDataLane::new(NonZeroU64::new(20 + boundary).unwrap());
        let modules = [metadata(0), metadata(0x10)];
        let mut session = session(&modules);
        let mut actor = opened_actor(&s, &mut session, &access, &lane);
        let stream = admitted_read(&s, &mut session, &mut actor, 3);
        assert_eq!(actor.pending_event().unwrap(), None);
        if boundary == 1 {
            s.step().unwrap();
        } // An actual Read is accepted, still pending.
        if boundary >= 2 {
            for _ in 0..1000 {
                s.step().unwrap();
            }
        }
        if boundary == 3 {
            assert_eq!(
                actor.pending_event().unwrap(),
                Some(HistoryPendingEvent::Record { stream_id: stream })
            );
        }
        let before = meta.calls.get();
        assert_eq!(
            control(
                &s,
                &mut session,
                &mut actor,
                HistoryRequest::CancelRead {
                    snapshot: 1,
                    stream_id: stream
                },
                4
            ),
            [0; 6]
        );
        assert_eq!(meta.calls.get(), before);
        assert_eq!(
            control(
                &s,
                &mut session,
                &mut actor,
                HistoryRequest::Ack {
                    snapshot: 1,
                    epoch: 8,
                    upto_id: 1
                },
                5
            ),
            [4, 0, 0, 0]
        );
        assert_eq!(meta.calls.get(), before);
        let next = admitted_read(&s, &mut session, &mut actor, 6);
        assert_ne!(next, stream);
        assert_eq!(
            control(
                &s,
                &mut session,
                &mut actor,
                HistoryRequest::CancelRead {
                    snapshot: 1,
                    stream_id: next
                },
                7
            ),
            [0; 6]
        );
        assert_eq!(
            run(
                &s,
                s.begin_append(input(&[43; 474], 0)).unwrap(),
                &[&meta, &log]
            ),
            Ok(JobResult::Appended { id: 2 })
        );
    }
}
#[test]
fn test_should_keep_started_flight_pinned_through_cancel_drain_and_resume_same_snapshot() {
    let meta = Control::new(8192, 256, 4096);
    let log = Control::new(8192, 256, 4096);
    let s: Store<1, 4, 474, 144> = owner(&meta, &log, Clock::new(), 5);
    initialize(
        &s,
        &meta,
        &log,
        &[DomainConfig {
            domain: 0,
            epoch: 8,
        }],
    );
    run(
        &s,
        s.begin_append(input(&[42; 474], 0)).unwrap(),
        &[&meta, &log],
    )
    .unwrap();
    let access = Access::yes();
    let lane = SharedDataLane::new(NonZeroU64::new(31).unwrap());
    let modules = [metadata(0), metadata(0x10)];
    let mut session = session(&modules);
    let mut actor = opened_actor(&s, &mut session, &access, &lane);
    let stream = admitted_read(&s, &mut session, &mut actor, 3);
    while actor.pending_event().unwrap().is_none() {
        s.step().unwrap();
    }
    let mut data = [0; 512];
    let event = actor.next_data(&mut data).unwrap().unwrap();
    assert_eq!(event.message_id, 1);
    let saved = data;
    let cancel = HistoryRequest::CancelRead {
        snapshot: 1,
        stream_id: stream,
    };
    let mut bytes = [0; 28];
    let n = cancel.encode(&mut bytes).unwrap();
    let request = frame(cancel.opcode(), 4, &bytes[..n]);
    let before = meta.calls.get();
    for _ in 0..3 {
        let mut out = [55; 512];
        assert_eq!(
            session.dispatch_with_handler(request, &mut actor, &mut out),
            Err(SessionError::Deferred)
        );
        assert_eq!(out, [0; 512]);
        s.step().unwrap();
    }
    assert_eq!(
        s.begin_append(input(&[43; 474], 0)).unwrap_err(),
        device_history_service::HistoryFailure::Busy
    );
    assert_eq!(actor.next_data(&mut data).unwrap(), Some(event));
    assert_eq!(data, saved);
    assert_eq!(
        actor.finish_data(event.message_id + 1),
        Err(device_history_service::HistoryFailure::Identity)
    );
    assert_eq!(meta.calls.get(), before);
    actor.finish_data(event.message_id).unwrap();
    assert_eq!(
        control(&s, &mut session, &mut actor, cancel, 4),
        [0, 0, 0, 0, 1, 0]
    );
    assert_eq!(meta.calls.get(), before);
    let next = admitted_read(&s, &mut session, &mut actor, 5);
    assert_ne!(next, stream);
    assert_eq!(
        control(
            &s,
            &mut session,
            &mut actor,
            HistoryRequest::CancelRead {
                snapshot: 1,
                stream_id: next
            },
            6
        ),
        [0, 0, 0, 0, 0, 0]
    );
    assert_eq!(
        run(
            &s,
            s.begin_append(input(&[43; 474], 0)).unwrap(),
            &[&meta, &log]
        ),
        Ok(JobResult::Appended { id: 2 })
    );
    assert_eq!(meta.calls.get(), before);
}

#[test]
fn test_should_keep_pinned_actor_alive_after_append_and_reject_recovered_identity_change() {
    use device_history_service::{HistoryFailure, RecordStore};
    for changed_dataset in [false, true] {
        let meta = Control::new(8192, 256, 4096);
        let log = Control::new(8192, 256, 4096);
        let s: Store<1, 4, 474, 144> = owner(&meta, &log, Clock::new(), 5);
        initialize(
            &s,
            &meta,
            &log,
            &[DomainConfig {
                domain: 0,
                epoch: 8,
            }],
        );
        run(
            &s,
            s.begin_append(input(&[41; 474], 0)).unwrap(),
            &[&meta, &log],
        )
        .unwrap();
        let access = Access::yes();
        let lane = SharedDataLane::new(NonZeroU64::new(90).unwrap());
        let mut adapter = s.adapter().unwrap();
        let pinned = adapter.directory().unwrap().domains.to_vec();
        let mut actor = OwnedHistoryHandler::<_, 1, 474>::new(
            metadata(0x10),
            s.adapter().unwrap(),
            &access,
            &lane,
            512,
            true,
        )
        .unwrap();
        run(
            &s,
            s.begin_append(input(&[42; 474], 0)).unwrap(),
            &[&meta, &log],
        )
        .unwrap();
        assert_eq!(adapter.directory().unwrap().domains, pinned);
        actor.advance(100).unwrap();
        assert!(!actor.must_end_session());
        // The next OPEN still obtains actual post-append bounds independently.
        let p = device_history_service::Preparation::Open {
            snapshot: 1,
            filter: filter(0),
        };
        while !adapter.prepare(p, 100, &access).unwrap() {
            s.step().unwrap();
        }
        let lease = adapter.open(1, filter(0), 100, &access).unwrap();
        assert_eq!(lease.snapshot.last_id, 2);
        adapter.close(lease).unwrap();
        // Replace with a separately initialized, valid durable owner image, then recover.
        let replacement_meta = Control::new(8192, 256, 4096);
        let replacement_log = Control::new(8192, 256, 4096);
        let replacement: Store<1, 4, 474, 144> =
            owner(&replacement_meta, &replacement_log, Clock::new(), 5);
        run(
            &replacement,
            replacement.begin_recover().unwrap(),
            &[&replacement_meta, &replacement_log],
        )
        .unwrap();
        let dataset = if changed_dataset { [9; 16] } else { [3; 16] };
        let epoch = if changed_dataset { 8 } else { 9 };
        run(
            &replacement,
            replacement
                .begin_initialize(dataset, &[DomainConfig { domain: 0, epoch }])
                .unwrap(),
            &[&replacement_meta, &replacement_log],
        )
        .unwrap();
        meta.bytes
            .borrow_mut()
            .clone_from(&replacement_meta.bytes.borrow());
        log.bytes
            .borrow_mut()
            .clone_from(&replacement_log.bytes.borrow());
        // Recovery itself rejects identity replacement before an adapter can observe it.
        assert_eq!(
            run(&s, s.begin_recover().unwrap(), &[&meta, &log]),
            Err(HistoryFailure::StoredIntegrity)
        );
        assert!(adapter.directory().is_err());
        assert!(actor.advance(101).is_err());
        assert!(actor.must_end_session());
    }
}
