//! Synthetic typed ports, never physical algorithm/Flash acceptance.
#![cfg(any(
    feature = "sport",
    feature = "sleep",
    feature = "activity",
    feature = "wear"
))]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use core::cell::Cell;
#[cfg(feature = "activity")]
use device_motion::ActivityRecord;
#[cfg(feature = "sleep")]
use device_motion::SleepProvisional;
#[cfg(feature = "sport")]
use device_motion::{
    MotionError, SportBackend, SportEngine, SportRealtime, SportState, SportSummary,
};
#[cfg(feature = "wear")]
use device_motion::{PostureInterval, WearInterval};
use device_motion_service::*;
use device_protocol::{Frame, Header, Kind, ModuleEntry};
use device_services::ServiceHandler;
#[cfg(feature = "sport")]
use device_services::SessionError;
#[cfg(feature = "sport")]
const SID: u64 = 0x0000_0001_0000_0001;
struct Access(Cell<bool>);
impl MotionAccess for Access {
    fn allowed(&self, _: u8, _: Operation, _: Resource) -> bool {
        self.0.get()
    }
}
struct Clock(Cell<u64>);
impl MotionClock for Clock {
    fn now_ms(&self) -> Result<u64, Failure> {
        Ok(self.0.get())
    }
}
struct Backend {
    selected: bool,
    missing: Option<Failure>,
    latest_value: Option<Observation>,
    #[cfg(feature = "sport")]
    actions: Vec<&'static str>,
    events: Vec<BackendEvent>,
    confirmed: Vec<u64>,
    detaches: u32,
    detach_fail: Option<Failure>,
    #[cfg(feature = "sport")]
    state: SportState,
    #[cfg(feature = "sport")]
    revoke_on_support: Option<std::rc::Rc<Cell<bool>>>,
    #[cfg(feature = "sport")]
    saving: bool,
    #[cfg(feature = "sport")]
    session: u64,
    #[cfg(feature = "sleep")]
    window: Cell<Option<SleepWindow>>,
    #[cfg(feature = "sleep")]
    no_utc: bool,
    #[cfg(feature = "activity")]
    clears: Vec<u8>,
}
impl Backend {
    fn new() -> Self {
        Self {
            selected: true,
            missing: None,
            latest_value: None,
            #[cfg(feature = "sport")]
            actions: Vec::new(),
            events: Vec::new(),
            confirmed: Vec::new(),
            detaches: 0,
            detach_fail: None,
            #[cfg(feature = "sport")]
            state: SportState::Stopped,
            #[cfg(feature = "sport")]
            revoke_on_support: None,
            #[cfg(feature = "sport")]
            saving: false,
            #[cfg(feature = "sport")]
            session: SID,
            #[cfg(feature = "sleep")]
            window: Cell::new(None),
            #[cfg(feature = "sleep")]
            no_utc: false,
            #[cfg(feature = "activity")]
            clears: Vec::new(),
        }
    }
}
fn stamp() -> Timestamp {
    Timestamp {
        utc_ms: 10_000,
        valid: true,
    }
}
#[cfg(feature = "sport")]
fn summary() -> SportSummary {
    SportSummary {
        session: SID,
        start_utc_ms: 10_000,
        end_utc_ms: 20_000,
        duration_ms: 1000,
        sport_type: 2,
        environment: 2,
        count_kind: 1,
        count: 100,
        distance_mm: 1000,
        energy_j: 2000,
        valid_mask: 63,
    }
}
#[cfg(feature = "sport")]
impl SportBackend for Backend {
    fn supports(&self, sport: u16, env: u8) -> bool {
        if let Some(access) = &self.revoke_on_support {
            access.set(false);
        }
        self.selected && sport == 2 && env == 2
    }
    fn start(&mut self, session: u64, _: u16, _: u8) -> Result<(), MotionError> {
        self.actions.push("start");
        self.session = session;
        self.state = SportState::Running;
        Ok(())
    }
    fn supports_saving(&self) -> bool {
        self.selected
    }
    fn transition(&mut self, _: u64, state: SportState) -> Result<(), MotionError> {
        self.state = state;
        self.actions.push(match state {
            SportState::Running => "resume",
            SportState::Paused => "pause",
            SportState::Stopped => "stop",
        });
        if state == SportState::Stopped {
            self.saving = false;
        }
        Ok(())
    }
    fn set_saving(&mut self, _: u64, enabled: bool) -> Result<bool, MotionError> {
        self.saving = enabled;
        self.actions
            .push(if enabled { "save-on" } else { "save-off" });
        Ok(enabled)
    }
}
impl MotionBackend for Backend {
    fn selected(&self, _: u8) -> bool {
        self.selected
    }
    fn status(&self, _: u8) -> Result<ModuleStatus, Failure> {
        Ok(ModuleStatus {
            state: 1,
            reason: 0,
        })
    }
    fn latest(&self, selector: LatestSelector) -> Result<Observation, Failure> {
        if let Some(failure) = self.missing {
            return Err(failure);
        }
        if let Some(value) = self.latest_value {
            return Ok(value);
        }
        Ok(match selector {
            #[cfg(feature = "sleep")]
            LatestSelector::Sleep => Observation::SleepCurrent(Snapshot {
                generation: 1,
                timestamp: stamp(),
                value: SleepProvisional {
                    duration_ms: 6000,
                    stage: 2,
                    quality: 100,
                    flags: 1,
                    provisional: 1,
                },
            }),
            #[cfg(feature = "sport")]
            LatestSelector::Sport => Observation::Sport(Snapshot {
                generation: 1,
                timestamp: stamp(),
                value: summary(),
            }),
            #[cfg(feature = "sport")]
            LatestSelector::LegacySport => return Err(Failure::NotFound),
            #[cfg(feature = "activity")]
            LatestSelector::Activity => Observation::Activity(Snapshot {
                generation: 1,
                timestamp: stamp(),
                value: ActivityRecord {
                    kind: 4,
                    start_utc_ms: 10_000,
                    end_utc_ms: 20_000,
                    steps: 100,
                    distance_mm: 1000,
                    energy_j: 2000,
                    valid_mask: 31,
                },
            }),
            #[cfg(feature = "wear")]
            LatestSelector::Wear => Observation::Wear(Snapshot {
                generation: 1,
                timestamp: stamp(),
                value: WearInterval {
                    start_utc_ms: 10_000,
                    end_utc_ms: 0,
                    state: 1,
                    valid_mask: 5,
                    provisional: 1,
                },
            }),
            #[cfg(feature = "wear")]
            LatestSelector::Posture => Observation::Posture(Snapshot {
                generation: 1,
                timestamp: stamp(),
                value: PostureInterval {
                    start_utc_ms: 10_000,
                    end_utc_ms: 0,
                    posture: 0,
                    valid_mask: 5,
                    provisional: 1,
                },
            }),
        })
    }
    #[cfg(feature = "sport")]
    fn sport_backend(&mut self) -> Result<&mut dyn SportBackend, Failure> {
        Ok(self)
    }
    #[cfg(feature = "sleep")]
    fn sleep_summary(&self, window: SleepWindow) -> Result<SleepSummary, Failure> {
        self.window.set(Some(window));
        if self.no_utc {
            return Err(Failure::NotFound);
        }
        Ok(SleepSummary {
            total_ms: 5000,
            deep_ms: 2000,
            light_ms: 3000,
            awake_ms: 1000,
            rem_ms: 0,
            interrupted_ms: 0,
            valid_mask: 15,
        })
    }
    #[cfg(feature = "activity")]
    fn clear_activity(
        &mut self,
        scope: u8,
        access: &dyn MotionAccess,
    ) -> Result<Snapshot<ActivityRecord>, Failure> {
        if !access.allowed(0x23, Operation::Clear, Resource::ActivityScope(scope)) {
            return Err(Failure::Denied);
        }
        self.clears.push(scope);
        Ok(Snapshot {
            generation: 2,
            timestamp: stamp(),
            value: ActivityRecord {
                kind: if scope == 1 { 1 } else { 3 },
                start_utc_ms: 10_000,
                end_utc_ms: 20_000,
                steps: 0,
                distance_mm: 0,
                energy_j: 0,
                valid_mask: 31,
            },
        })
    }
    fn pending_event(
        &self,
        _: u64,
        module: u8,
        resource: Resource,
        after: u64,
    ) -> Result<Option<BackendEvent>, Failure> {
        Ok(self
            .events
            .iter()
            .find(|event| {
                let (actual_module, actual_resource) = match event.body {
                    #[cfg(feature = "sport")]
                    EventBody::SportRealtime(value) => (0x22, Resource::Session(value.session)),
                    #[cfg(feature = "sport")]
                    EventBody::SportEnd { result, .. } => {
                        (0x22, Resource::Session(result.value.session))
                    }
                    #[cfg(feature = "sleep")]
                    EventBody::Sleep(_) => (0x21, Resource::Record(0x0021)),
                    #[cfg(feature = "activity")]
                    EventBody::ActivityCurrent(_) | EventBody::ActivityEnd(_) => {
                        (0x23, Resource::Record(0x2301))
                    }
                    #[cfg(feature = "wear")]
                    EventBody::WearChange { kind, .. } => (
                        0x24,
                        Resource::Record(match kind {
                            WearKind::Wear => 0x2401,
                            WearKind::Posture => 0x2402,
                        }),
                    ),
                    #[cfg(feature = "wear")]
                    EventBody::WearEnd(_) => (0x24, Resource::Record(0x2401)),
                    #[cfg(feature = "wear")]
                    EventBody::PostureEnd(_) => (0x24, Resource::Record(0x2402)),
                };
                event.generation > after
                    && !self.confirmed.contains(&event.generation)
                    && module == actual_module
                    && (resource == actual_resource
                        || matches!(resource, Resource::ActivityScope(_))
                            && actual_resource == Resource::Record(0x2301))
            })
            .copied())
    }
    fn event_current(&self, _: u64, _: u8, generation: u64) -> bool {
        self.events
            .iter()
            .any(|event| event.generation == generation)
            && !self.confirmed.contains(&generation)
    }
    fn event_admitted(&mut self, _: u64, _: u8, generation: u64) -> Result<(), Failure> {
        self.confirmed.push(generation);
        Ok(())
    }
    fn discard_event(&mut self, _: u64, _: u8, generation: u64) -> Result<(), Failure> {
        self.events.retain(|event| event.generation != generation);
        Ok(())
    }
    fn detach(&mut self, _: u64) -> Result<(), Failure> {
        self.detaches += 1;
        if let Some(failure) = self.detach_fail {
            Err(failure)
        } else {
            Ok(())
        }
    }
}
fn owner(backend: Backend) -> MotionOwner<Backend> {
    #[cfg(feature = "sport")]
    {
        MotionOwner::new(SportEngine::new(SID).unwrap(), backend)
    }
    #[cfg(not(feature = "sport"))]
    {
        MotionOwner::new(backend)
    }
}
fn metadata(module: u8) -> ModuleEntry {
    ModuleEntry {
        module,
        major: 1,
        minor: 0,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    }
}
fn frame(module: u8, opcode: u8, bytes: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module,
            opcode,
            message_id: 1,
            offset: 0,
            total_length: u16::try_from(bytes.len()).unwrap(),
        },
        payload: bytes,
    }
}
type Handler<'a> = MotionHandler<'a, Backend, 8>;
fn run(handler: &mut Handler<'_>, module: u8, opcode: u8, bytes: &[u8]) -> Vec<u8> {
    let mut out = [0xa5; 128];
    let length = handler
        .dispatch(frame(module, opcode, bytes), &mut out)
        .unwrap();
    out[..length].to_vec()
}
#[cfg(feature = "sport")]
fn command(session: u64, action: u8) -> [u8; 9] {
    let mut out = [0; 9];
    out[..8].copy_from_slice(&session.to_le_bytes());
    out[8] = action;
    out
}
#[cfg(feature = "sport")]
fn acknowledge(handler: &mut Handler<'_>, module: u8, opcode: u8, body: &[u8], response: &[u8]) {
    handler
        .response_admitted(frame(module, opcode, body), response)
        .unwrap();
}

#[cfg(feature = "sport")]
#[test]
fn sport_actual_controls_saving_and_repeated_actions_keep_device_identity() {
    let owner = owner(Backend::new());
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x22)];
    {
        let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
        let start = run(&mut handler, 0x22, 0x30, &[2, 0, 2]);
        assert_eq!(&start[..4], &[0; 4]);
        assert_eq!(&start[4..], &SID.to_le_bytes());
        acknowledge(&mut handler, 0x22, 0x30, &[2, 0, 2], &start);
        clock.0.set(2000);
        assert_eq!(
            run(&mut handler, 0x22, 0x31, &command(SID, 1)),
            [0, 0, 0, 0, 2]
        );
        clock.0.set(3000);
        assert_eq!(
            run(&mut handler, 0x22, 0x31, &command(SID, 1)),
            [0, 0, 0, 0, 2]
        );
        assert_eq!(
            run(&mut handler, 0x22, 0x32, &command(SID, 1)),
            [0, 0, 0, 0, 1]
        );
        assert_eq!(
            run(&mut handler, 0x22, 0x32, &command(SID, 1)),
            [0, 0, 0, 0, 1]
        );
        clock.0.set(4000);
        assert_eq!(
            run(&mut handler, 0x22, 0x31, &command(SID, 2)),
            [0, 0, 0, 0, 1]
        );
        clock.0.set(5000);
        assert_eq!(
            run(&mut handler, 0x22, 0x31, &command(SID, 3)),
            [0, 0, 0, 0, 3]
        );
        assert_eq!(
            run(&mut handler, 0x22, 0x31, &command(SID, 3)),
            [0, 0, 0, 0, 3]
        );
        assert_eq!(
            run(&mut handler, 0x22, 0x31, &command(SID, 2)),
            [11, 0, 0, 0]
        );
        assert_eq!(
            run(&mut handler, 0x22, 0x32, &command(SID, 2)),
            [0, 0, 0, 0, 0]
        );
    }
    assert_eq!(
        {
            owner
                .with_sport(|engine| {
                    assert_eq!(engine.active_ms(SID, 6000), Ok(2000));
                    Ok(())
                })
                .unwrap();
            owner
                .with_backend(|backend| Ok(backend.actions.clone()))
                .unwrap()
        },
        ["start", "pause", "save-on", "resume", "stop"]
    );
}
#[cfg(feature = "sport")]
#[test]
fn aliases_preserve_full_u64_and_disconnect_mutes_app_without_stopping_device_sport() {
    let owner = owner(Backend::new());
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x22)];
    {
        let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
        run(&mut handler, 0x22, 0x30, &[2, 0, 2]);
        assert_eq!(
            run(&mut handler, 0x22, 0x34, &SID.to_le_bytes()),
            [0, 0, 0, 0, 1, 0, 0, 0]
        );
        assert_eq!(handler.resolve_context(1), Ok(SID));
        assert_eq!(handler.connection_closed(), Ok(()));
        assert!(handler.must_end_session());
        assert_eq!(handler.resolve_context(1), Err(Failure::Closed));
        assert_eq!(handler.connection_closed(), Ok(()));
    }
    owner
        .with_sport(|engine| {
            assert_eq!(
                engine.session(SID).unwrap().state(),
                Ok(SportState::Running)
            );

            Ok(())
        })
        .unwrap();
    owner
        .with_backend(|backend| {
            assert_eq!(backend.actions, ["start"]);
            assert_eq!(backend.detaches, 1);
            Ok(())
        })
        .unwrap();
    assert_eq!(
        Handler::new(&owner, 1, &entries, &access, &clock).err(),
        Some(Failure::Invalid)
    );
    assert!(Handler::new(&owner, 2, &entries, &access, &clock).is_ok());
}
#[cfg(feature = "sport")]
#[test]
fn no_side_effect_before_complete_body_or_response_capacity() {
    let owner = owner(Backend::new());
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x22)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    assert_eq!(run(&mut handler, 0x22, 0x30, &[2, 0]), [1, 0, 0, 0]);
    assert_eq!(run(&mut handler, 0x22, 0x30, &[24, 0, 2]), [1, 0, 0, 0]);
    assert_eq!(
        handler.dispatch(frame(0x22, 0x30, &[2, 0, 2]), &mut [0; 11]),
        Err(SessionError::Capacity)
    );
    owner
        .with_backend(|backend| {
            assert!(backend.actions.is_empty());
            Ok(())
        })
        .unwrap();
}
#[cfg(feature = "sport")]
#[test]
fn sport_events_wait_for_real_reply_admission_and_have_exact_bodies_without_crc() {
    let mut backend = Backend::new();
    backend.events.push(BackendEvent {
        generation: 1,
        body: EventBody::SportRealtime(SportRealtime {
            session: SID,
            time_offset_ms: 1000,
            valid_mask: 15,
            bpm: 60,
            cadence_per_min: 100,
            steps: 10,
            rri_us: 800_000,
        }),
    });
    let owner = owner(backend);
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x22)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    let start = run(&mut handler, 0x22, 0x30, &[2, 0, 2]);
    let mut out = [0xa5; 128];
    assert_eq!(handler.next_event(&mut out), Ok(None));
    acknowledge(&mut handler, 0x22, 0x30, &[2, 0, 2], &start);
    let event = handler.next_event(&mut out).unwrap().unwrap();
    assert_eq!((event.module, event.opcode, event.length), (0x22, 0x80, 26));
    let body = out;
    assert_eq!(&body[..8], &SID.to_le_bytes());
    assert_eq!(handler.next_event(&mut out), Ok(Some(event)));
    assert_eq!(body, out);
    assert!(SportRealtime::decode(&out[..26]).is_ok());
    assert!(SportRealtime::decode(&out[..30]).is_err());
    handler.event_admitted(event.token).unwrap();
    handler.event_admitted(event.token).unwrap();
    assert_eq!(handler.next_event(&mut out), Ok(None));
    owner
        .with_backend(|backend| {
            assert_eq!(backend.confirmed, vec![1]);
            Ok(())
        })
        .unwrap();
}
#[cfg(feature = "sport")]
#[test]
fn sport_summary_has_explicit_end_reason_and_latest_wrapper_not_an_extra_crc() {
    let mut backend = Backend::new();
    backend.events.push(BackendEvent {
        generation: 1,
        body: EventBody::SportEnd {
            reason: 0x0104,
            result: Snapshot {
                generation: 1,
                timestamp: stamp(),
                value: summary(),
            },
        },
    });
    let owner = owner(backend);
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x22)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    let start = run(&mut handler, 0x22, 0x30, &[2, 0, 2]);
    acknowledge(&mut handler, 0x22, 0x30, &[2, 0, 2], &start);
    clock.0.set(2000);
    let stop = run(&mut handler, 0x22, 0x31, &command(SID, 3));
    let mut out = [0; 128];
    assert_eq!(handler.next_event(&mut out), Ok(None));
    acknowledge(&mut handler, 0x22, 0x31, &command(SID, 3), &stop);
    let event = handler.next_event(&mut out).unwrap().unwrap();
    assert_eq!((event.opcode, event.length), (0x81, 63));
    assert_eq!(&out[..6], &[4, 1, 1, 0x22, 2, 0]);
    assert!(SportSummary::decode(&out[17..63]).is_ok());
}
#[cfg(feature = "sleep")]
#[test]
fn actual_sleep_latest_uses_schema2_while_final_records_stay_schema1() {
    let owner = owner(Backend::new());
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x21)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    let out = run(&mut handler, 0x21, 2, &[]);
    assert_eq!(out.len(), 27);
    assert_eq!(&out[..8], &[0, 0, 0, 0, 0x21, 0, 2, 0]);
    assert_eq!(&out[17..19], &[8, 0]);
    assert!(SleepProvisional::decode(&out[19..]).is_ok());
    owner
        .with_backend(|backend| {
            backend.missing = Some(Failure::NotFound);
            Ok(())
        })
        .unwrap();
    assert_eq!(run(&mut handler, 0x21, 2, &[]), [8, 0, 0, 0]);
}
#[cfg(feature = "sleep")]
#[test]
fn sleep_window_summary_uses_actual_utc_window_and_keeps_unsupported_stages_absent() {
    let owner = owner(Backend::new());
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x21)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    let mut body = [0; 16];
    body[..8].copy_from_slice(&10_000i64.to_le_bytes());
    body[8..].copy_from_slice(&20_000i64.to_le_bytes());
    let out = run(&mut handler, 0x21, 0x33, &body);
    assert_eq!(out.len(), 30);
    assert_eq!(&out[28..30], &15u16.to_le_bytes());
    owner
        .with_backend(|backend| {
            assert_eq!(
                backend.window.get(),
                Some(SleepWindow {
                    start_ms: 10_000,
                    end_ms: 20_000
                })
            );
            backend.no_utc = true;
            Ok(())
        })
        .unwrap();
    assert_eq!(run(&mut handler, 0x21, 0x33, &body), [8, 0, 0, 0]);
    body[8..].copy_from_slice(&10_000i64.to_le_bytes());
    assert_eq!(run(&mut handler, 0x21, 0x33, &body), [1, 0, 0, 0]);
}
#[cfg(feature = "activity")]
#[test]
fn activity_clear_is_real_atomic_typed_post_clear_snapshot_not_history_erasure() {
    let owner = owner(Backend::new());
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x23)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    for (scope, kind) in [(1, 1), (2, 3)] {
        let out = run(&mut handler, 0x23, 0x30, &[scope]);
        assert_eq!(out.len(), 35);
        let value = ActivityRecord::decode(&out[4..]).unwrap();
        assert_eq!(value.kind, kind);
        assert_eq!((value.steps, value.distance_mm, value.energy_j), (0, 0, 0));
    }
    assert_eq!(run(&mut handler, 0x23, 0x30, &[3]), [1, 0, 0, 0]);
    owner
        .with_backend(|backend| {
            assert_eq!(backend.clears, [1, 2]);
            Ok(())
        })
        .unwrap();
}
#[cfg(feature = "wear")]
#[test]
fn wear_latest_requires_explicit_selector_and_preserves_sitting_zero_as_real_value() {
    let owner = owner(Backend::new());
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x24)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    assert_eq!(run(&mut handler, 0x24, 2, &[]), [1, 0, 0, 0]);
    let wear = run(&mut handler, 0x24, 2, &[1, 0x24]);
    assert_eq!(wear.len(), 38);
    assert!(WearInterval::decode(&wear[19..]).is_ok());
    let posture = run(&mut handler, 0x24, 2, &[2, 0x24]);
    assert_eq!(posture.len(), 38);
    assert_eq!(PostureInterval::decode(&posture[19..]).unwrap().posture, 0);
    assert_eq!(run(&mut handler, 0x24, 2, &[3, 0x24]), [1, 0, 8, 255]);
}
#[cfg(feature = "wear")]
#[test]
fn wear_change_is_exact_plain_12bytes_and_scope_revocation_clears_pending_contents() {
    let mut backend = Backend::new();
    backend.events.push(BackendEvent {
        generation: 1,
        body: EventBody::WearChange {
            kind: WearKind::Wear,
            timestamp: stamp(),
            value: 2,
        },
    });
    let owner = owner(backend);
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x24)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    run(&mut handler, 0x24, 2, &[1, 0x24]);
    let mut out = [0xa5; 128];
    let event = handler.next_event(&mut out).unwrap().unwrap();
    assert_eq!((event.module, event.opcode, event.length), (0x24, 0x80, 12));
    assert_eq!(&out[..2], &[1, 0x24]);
    assert_eq!(&out[10..12], &[1, 2]);
    access.0.set(false);
    assert!(!handler.event_valid(event.token));
    assert!(handler.must_end_session());
    assert_eq!(handler.next_event(&mut out), Err(Failure::Closed));
    assert_eq!(out, [0; 128]);
    handler.connection_closed().unwrap();
}
#[test]
fn business_version_and_actual_backend_selection_are_not_just_sdk_code_presence() {
    let backend = Backend::new();
    let owner = owner(backend);
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let module = if cfg!(feature = "sport") {
        0x22
    } else if cfg!(feature = "sleep") {
        0x21
    } else if cfg!(feature = "activity") {
        0x23
    } else {
        0x24
    };
    let mut wrong = metadata(module);
    wrong.minor = 2;
    assert_eq!(
        Handler::new(&owner, 1, &[wrong], &access, &clock).err(),
        Some(Failure::Invalid)
    );
    owner
        .with_backend(|backend| {
            backend.selected = false;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        Handler::new(&owner, 1, &[metadata(module)], &access, &clock).err(),
        Some(Failure::Unsupported)
    );
}
#[cfg(feature = "sport")]
#[test]
fn frozen_legacy_sport_summary20bytes_is_preserved_without_synthesizing_it_from_schema2() {
    let bytes = [
        0xc0, 0x27, 9, 0, 0xe8, 3, 0, 0, 0x60, 0xae, 10, 0, 0xa0, 0x86, 1, 0, 2, 0, 15, 0,
    ];
    let value = LegacySportSummary::decode(&bytes).unwrap();
    assert_eq!(
        (
            value.duration_ms,
            value.steps,
            value.distance_mm,
            value.energy_j,
            value.sport_type,
            value.valid_mask
        ),
        (600_000, 1000, 700_000, 100_000, 2, 15)
    );
    let mut out = [0xa5; 21];
    assert_eq!(value.encode(&mut out), Ok(20));
    assert_eq!(&out[..20], &bytes);
    assert_eq!(out[20], 0xa5);
    let mut bad = value;
    bad.valid_mask = 16;
    assert_eq!(bad.validate(), Err(Failure::Invalid));
    bad = value;
    bad.valid_mask = 0;
    assert_eq!(bad.validate(), Err(Failure::Invalid));
    assert!(LegacySportSummary::decode(&bytes[..19]).is_err());
}

#[cfg(feature = "sport")]
#[test]
fn cached_latest_scope_is_bound_to_full_session_and_context_scope_revocation_closes() {
    struct Scoped {
        denied: Cell<Option<Operation>>,
    }
    impl MotionAccess for Scoped {
        fn allowed(&self, module: u8, operation: Operation, resource: Resource) -> bool {
            !(module == 0x22
                && resource == Resource::Session(SID)
                && self.denied.get() == Some(operation))
        }
    }
    let owner = owner(Backend::new());
    let access = Scoped {
        denied: Cell::new(None),
    };
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x22)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    let response = run(&mut handler, 0x22, 2, &[]);
    assert!(handler.replay_valid(frame(0x22, 2, &[]), &response));
    access.denied.set(Some(Operation::Latest));
    assert!(!handler.replay_valid(frame(0x22, 2, &[]), &response));
    assert!(handler.must_end_session());
}
#[cfg(feature = "sport")]
#[test]
fn command_authority_does_not_implicitly_require_or_grant_event_authority() {
    struct NoEvents;
    impl MotionAccess for NoEvents {
        fn allowed(&self, _: u8, operation: Operation, _: Resource) -> bool {
            operation != Operation::Event
        }
    }
    let owner = owner(Backend::new());
    let access = NoEvents;
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x22)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    let response = run(&mut handler, 0x22, 0x30, &[2, 0, 2]);
    assert_eq!(response.len(), 12);
    assert!(!handler.must_end_session());
    acknowledge(&mut handler, 0x22, 0x30, &[2, 0, 2], &response);
    assert_eq!(handler.next_event(&mut [0; 128]), Ok(None));
}
#[cfg(feature = "sport")]
#[test]
fn wrong_reply_admission_cannot_release_start_event_gate_or_repeat_effect() {
    let mut backend = Backend::new();
    backend.events.push(BackendEvent {
        generation: 1,
        body: EventBody::SportRealtime(SportRealtime {
            session: SID,
            time_offset_ms: 0,
            valid_mask: 1,
            bpm: 60,
            cadence_per_min: 0,
            steps: 0,
            rri_us: 0,
        }),
    });
    let owner = owner(backend);
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x22)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    let start = run(&mut handler, 0x22, 0x30, &[2, 0, 2]);
    let mut wrong = start.clone();
    wrong[4] ^= 1;
    acknowledge(&mut handler, 0x22, 0x30, &[2, 0, 2], &wrong);
    assert_eq!(handler.next_event(&mut [0; 128]), Ok(None));
    acknowledge(&mut handler, 0x22, 0x30, &[2, 0, 2], &start);
    assert!(handler.next_event(&mut [0; 128]).unwrap().is_some());
    owner
        .with_backend(|backend| {
            assert_eq!(backend.actions, ["start"]);
            Ok(())
        })
        .unwrap();
}
#[cfg(feature = "sport")]
#[test]
fn cleanup_failure_is_retryable_and_prevents_a_second_app_owner() {
    let mut backend = Backend::new();
    backend.detach_fail = Some(Failure::Backend);
    let owner = owner(backend);
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x22)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    run(&mut handler, 0x22, 0x30, &[2, 0, 2]);
    assert_eq!(
        handler.connection_closed(),
        Err(SessionError::InvalidHandlerResponse)
    );
    assert!(handler.must_end_session());
    assert_eq!(
        Handler::new(&owner, 2, &entries, &access, &clock).err(),
        Some(Failure::Busy)
    );
    owner
        .with_backend(|backend| {
            backend.detach_fail = None;
            Ok(())
        })
        .unwrap();
    handler.connection_closed().unwrap();
    assert!(Handler::new(&owner, 2, &entries, &access, &clock).is_ok());
    owner
        .with_sport(|engine| {
            assert_eq!(
                engine.session(SID).unwrap().state(),
                Ok(SportState::Running)
            );
            Ok(())
        })
        .unwrap();
    owner
        .with_backend(|backend| {
            assert_eq!(backend.actions, ["start"]);
            Ok(())
        })
        .unwrap();
}
#[cfg(feature = "sport")]
#[test]
fn trusted_clock_regression_ends_app_and_never_fabricates_a_stop() {
    let owner = owner(Backend::new());
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x22)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    run(&mut handler, 0x22, 0x30, &[2, 0, 2]);
    clock.0.set(999);
    let mut out = [0xa5; 128];
    assert_eq!(
        handler.dispatch(frame(0x22, 0x31, &command(SID, 3)), &mut out),
        Err(SessionError::AuthorizationRevoked)
    );
    assert_eq!(out, [0; 128]);
    handler.connection_closed().unwrap();
    owner
        .with_backend(|backend| {
            assert_eq!(backend.actions, ["start"]);
            Ok(())
        })
        .unwrap();
}
#[cfg(feature = "sport")]
#[test]
fn zero_and_unknown_id_cannot_control_real_device_resources() {
    let owner = owner(Backend::new());
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x22)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    assert_eq!(run(&mut handler, 0x22, 0x31, &command(0, 1)), [1, 0, 0, 0]);
    assert_eq!(
        run(&mut handler, 0x22, 0x31, &command(SID, 1)),
        [8, 0, 0, 0]
    );
    assert_eq!(run(&mut handler, 0x22, 0x33, &[]), [3, 0, 0, 0]);
    assert_eq!(run(&mut handler, 0x22, 1, &[]), [0, 0, 0, 0, 1, 0, 0]);
}

#[cfg(feature = "sport")]
#[test]
fn legacy_read_returns_only_actual_0022_content_and_normal_latest_rejects_wrong_schema() {
    let old = LegacySportSummary {
        duration_ms: 600_000,
        steps: 1000,
        distance_mm: 700_000,
        energy_j: 100_000,
        sport_type: 2,
        valid_mask: 15,
    };
    let mut backend = Backend::new();
    backend.latest_value = Some(Observation::LegacySport(Snapshot {
        generation: 42,
        timestamp: stamp(),
        value: old,
    }));
    let owner = owner(backend);
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x22)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    assert_eq!(handler.legacy_sport().unwrap().value, old);
    assert_eq!(run(&mut handler, 0x22, 2, &[]), [17, 0, 0, 0]);
    owner
        .with_backend(|backend| {
            backend.missing = Some(Failure::NotFound);
            Ok(())
        })
        .unwrap();
    assert_eq!(handler.legacy_sport(), Err(Failure::NotFound));
}
#[cfg(feature = "sleep")]
#[test]
fn sleep_final_and_provisional_events_are_typed_22_and_23_bytes_without_crc() {
    let final_value = device_motion::SleepInterval {
        duration_ms: 6000,
        stage: 2,
        quality: 100,
        flags: 1,
    };
    let current = device_motion::SleepProvisional {
        duration_ms: 6000,
        stage: 2,
        quality: 100,
        flags: 1,
        provisional: 1,
    };
    let mut backend = Backend::new();
    backend.events.push(BackendEvent {
        generation: 1,
        body: EventBody::Sleep(Observation::SleepFinal(Snapshot {
            generation: 1,
            timestamp: stamp(),
            value: final_value,
        })),
    });
    backend.events.push(BackendEvent {
        generation: 2,
        body: EventBody::Sleep(Observation::SleepCurrent(Snapshot {
            generation: 2,
            timestamp: stamp(),
            value: current,
        })),
    });
    let owner = owner(backend);
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x21)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    run(&mut handler, 0x21, 2, &[]);
    let mut out = [0xa5; 128];
    let first = handler.next_event(&mut out).unwrap().unwrap();
    assert_eq!((first.module, first.opcode, first.length), (0x21, 0x80, 22));
    assert_eq!(&out[..4], &[0x21, 0, 1, 0]);
    assert_eq!(
        device_motion::SleepInterval::decode(&out[15..22]),
        Ok(final_value)
    );
    handler.event_admitted(first.token).unwrap();
    let next = handler.next_event(&mut out).unwrap().unwrap();
    assert_eq!(next.length, 23);
    assert_eq!(&out[2..4], &[2, 0]);
    assert_eq!(
        device_motion::SleepProvisional::decode(&out[15..23]),
        Ok(current)
    );
    assert_eq!(
        handler.event_admitted(first.token),
        Err(Failure::InvalidState)
    );
}
#[cfg(feature = "activity")]
#[test]
fn full_activity_current_and_final_events_use_exact_latest46_bytes() {
    let current = ActivityRecord {
        kind: 4,
        start_utc_ms: 10_000,
        end_utc_ms: 20_000,
        steps: 100,
        distance_mm: 1000,
        energy_j: 2000,
        valid_mask: 31,
    };
    let mut closed = current;
    closed.kind = 3;
    let mut backend = Backend::new();
    backend.events.push(BackendEvent {
        generation: 1,
        body: EventBody::ActivityCurrent(Snapshot {
            generation: 1,
            timestamp: stamp(),
            value: current,
        }),
    });
    backend.events.push(BackendEvent {
        generation: 2,
        body: EventBody::ActivityEnd(Snapshot {
            generation: 2,
            timestamp: stamp(),
            value: closed,
        }),
    });
    let owner = owner(backend);
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x23)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    run(&mut handler, 0x23, 2, &[]);
    let mut out = [0xa5; 128];
    for (opcode, value) in [(0x80, current), (0x81, closed)] {
        let event = handler.next_event(&mut out).unwrap().unwrap();
        assert_eq!((event.opcode, event.length), (opcode, 46));
        assert_eq!(&out[..4], &[1, 0x23, 1, 0]);
        assert_eq!(ActivityRecord::decode(&out[15..46]), Ok(value));
        handler.event_admitted(event.token).unwrap();
    }
    assert_eq!(handler.next_event(&mut out), Ok(None));
}
#[cfg(feature = "wear")]
#[test]
fn closed_wear_and_posture_events_are_strict34_bytes_and_absent_utc_is_not_read_time() {
    for posture in [false, true] {
        let stamp = Timestamp {
            utc_ms: 0,
            valid: false,
        };
        let mut backend = Backend::new();
        backend.events.push(BackendEvent {
            generation: 1,
            body: if posture {
                EventBody::PostureEnd(Snapshot {
                    generation: 1,
                    timestamp: stamp,
                    value: PostureInterval {
                        start_utc_ms: 10_000,
                        end_utc_ms: 20_000,
                        posture: 0,
                        valid_mask: 7,
                        provisional: 0,
                    },
                })
            } else {
                EventBody::WearEnd(Snapshot {
                    generation: 1,
                    timestamp: stamp,
                    value: WearInterval {
                        start_utc_ms: 10_000,
                        end_utc_ms: 20_000,
                        state: 1,
                        valid_mask: 7,
                        provisional: 0,
                    },
                })
            },
        });
        let owner = owner(backend);
        let access = Access(Cell::new(true));
        let clock = Clock(Cell::new(1000));
        let entries = [metadata(0x24)];
        let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
        run(&mut handler, 0x24, 2, &[if posture { 2 } else { 1 }, 0x24]);
        let mut out = [0xa5; 128];
        let event = handler.next_event(&mut out).unwrap().unwrap();
        assert_eq!((event.opcode, event.length), (0x81, 34));
        assert_eq!(&out[4..13], &[0; 9]);
        if posture {
            assert_eq!(PostureInterval::decode(&out[15..34]).unwrap().posture, 0);
        } else {
            assert_eq!(WearInterval::decode(&out[15..34]).unwrap().state, 1);
        }
        handler.event_admitted(event.token).unwrap();
    }
}
#[cfg(feature = "sport")]
#[test]
fn paused_new_realtime_is_discarded_without_synthetic_data_or_delivery_confirmation() {
    let mut backend = Backend::new();
    backend.events.push(BackendEvent {
        generation: 1,
        body: EventBody::SportRealtime(SportRealtime {
            session: SID,
            time_offset_ms: 1000,
            valid_mask: 1,
            bpm: 60,
            cadence_per_min: 0,
            steps: 0,
            rri_us: 0,
        }),
    });
    let owner = owner(backend);
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x22)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    let response = run(&mut handler, 0x22, 0x30, &[2, 0, 2]);
    acknowledge(&mut handler, 0x22, 0x30, &[2, 0, 2], &response);
    clock.0.set(2000);
    run(&mut handler, 0x22, 0x31, &command(SID, 1));
    let mut out = [0xa5; 128];
    assert_eq!(handler.next_event(&mut out), Ok(None));
    assert_eq!(out, [0xa5; 128]);
    owner
        .with_backend(|backend| {
            assert!(backend.events.is_empty());
            assert!(backend.confirmed.is_empty());
            Ok(())
        })
        .unwrap();
}
#[cfg(feature = "sport")]
#[test]
fn initial_denial_is_wire_error_and_never_calls_actual_start() {
    let owner = owner(Backend::new());
    let access = Access(Cell::new(false));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x22)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    assert_eq!(run(&mut handler, 0x22, 0x30, &[2, 0, 2]), [5, 0, 3, 255]);
    assert!(!handler.must_end_session());
    owner
        .with_backend(|backend| {
            assert!(backend.actions.is_empty());
            Ok(())
        })
        .unwrap();
}

#[cfg(feature = "sport")]
#[test]
fn real_session_identical_start_replay_uses_cache_and_never_restarts_backend() {
    use device_protocol::ProtocolLimits;
    use device_services::{BuildProfile, Config, DeviceInfo, Session};
    let owner = owner(Backend::new());
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [
        ModuleEntry {
            module: 0,
            major: 1,
            minor: 2,
            availability: 1,
            features: 0,
            input_max: 128,
            output_max: 128,
        },
        metadata(0x22),
    ];
    let mut handler = Handler::new(&owner, 1, &entries[1..], &access, &clock).unwrap();
    let mut session = Session::new(Config {
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
        modules: &entries,
        limits: ProtocolLimits {
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
    let mut out = [0; 128];
    session
        .dispatch_with_handler(
            frame(0, 1, &[1, 2, 128, 0, 0, 0, 3, 0]),
            &mut handler,
            &mut out,
        )
        .unwrap();
    let mut request = frame(0x22, 0x30, &[2, 0, 2]);
    request.header.message_id = 2;
    let length = session
        .dispatch_with_handler(request, &mut handler, &mut out)
        .unwrap();
    let first = out;
    handler.response_admitted(request, &out[..length]).unwrap();
    clock.0.set(2000);
    assert_eq!(
        session.dispatch_with_handler(request, &mut handler, &mut out),
        Ok(length)
    );
    assert_eq!(out, first);
    owner
        .with_backend(|backend| {
            assert_eq!(backend.actions, ["start"]);
            Ok(())
        })
        .unwrap();
}

#[cfg(feature = "sport")]
#[test]
fn terminal_event_requires_confirmed_stopped_engine_in_same_device_owner() {
    let mut backend = Backend::new();
    backend.events.push(BackendEvent {
        generation: 1,
        body: EventBody::SportEnd {
            reason: 0,
            result: Snapshot {
                generation: 1,
                timestamp: stamp(),
                value: summary(),
            },
        },
    });
    let owner = owner(backend);
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x22)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    let reply = run(&mut handler, 0x22, 0x30, &[2, 0, 2]);
    acknowledge(&mut handler, 0x22, 0x30, &[2, 0, 2], &reply);
    assert_eq!(handler.next_event(&mut [0; 128]), Err(Failure::Contract));
}
#[cfg(feature = "sport")]
#[test]
fn reconnect_can_control_existing_device_session_and_receive_real_end_after_response() {
    let owner = owner(Backend::new());
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x22)];
    let mut first = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    run(&mut first, 0x22, 0x30, &[2, 0, 2]);
    first.connection_closed().unwrap();
    owner
        .with_backend(|backend| {
            backend.events.push(BackendEvent {
                generation: 1,
                body: EventBody::SportEnd {
                    reason: 0,
                    result: Snapshot {
                        generation: 1,
                        timestamp: stamp(),
                        value: summary(),
                    },
                },
            });
            Ok(())
        })
        .unwrap();
    let mut second = Handler::new(&owner, 2, &entries, &access, &clock).unwrap();
    clock.0.set(2000);
    let command = command(SID, 3);
    let reply = run(&mut second, 0x22, 0x31, &command);
    assert_eq!(second.next_event(&mut [0; 128]), Ok(None));
    acknowledge(&mut second, 0x22, 0x31, &command, &reply);
    assert_eq!(
        second.next_event(&mut [0; 128]).unwrap().unwrap().length,
        63
    );
}
#[cfg(feature = "sport")]
#[test]
fn retained_alias_checks_context_authority_independently_of_start_or_event_scope() {
    struct Scoped(Cell<bool>);
    impl MotionAccess for Scoped {
        fn allowed(&self, _: u8, op: Operation, _: Resource) -> bool {
            op != Operation::Context || self.0.get()
        }
    }
    let owner = owner(Backend::new());
    let access = Scoped(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x22)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    run(&mut handler, 0x22, 0x30, &[2, 0, 2]);
    run(&mut handler, 0x22, 0x34, &SID.to_le_bytes());
    access.0.set(false);
    assert!(handler.must_end_session());
    assert_eq!(handler.resolve_context(1), Err(Failure::Closed));
}

#[cfg(feature = "sport")]
#[test]
fn live_scope_is_rechecked_after_capability_query_before_actual_start_side_effect() {
    struct Shared(std::rc::Rc<Cell<bool>>);
    impl MotionAccess for Shared {
        fn allowed(&self, _: u8, _: Operation, _: Resource) -> bool {
            self.0.get()
        }
    }
    let flag = std::rc::Rc::new(Cell::new(true));
    let access = Shared(flag.clone());
    let mut backend = Backend::new();
    backend.revoke_on_support = Some(flag);
    let owner = owner(backend);
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x22)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    let mut out = [0xa5; 128];
    assert_eq!(
        handler.dispatch(frame(0x22, 0x30, &[2, 0, 2]), &mut out),
        Err(SessionError::AuthorizationRevoked)
    );
    assert_eq!(out, [0; 128]);
    assert!(handler.must_end_session());
    owner
        .with_backend(|backend| {
            assert!(backend.actions.is_empty());
            Ok(())
        })
        .unwrap();
}

#[test]
fn debug_uses_lengths_and_never_prints_query_measurement_or_identity_payloads() {
    let status = format!(
        "{:?}",
        ModuleStatus {
            state: 3,
            reason: 0xabcd
        }
    );
    assert!(status.contains("encoded_length"));
    assert!(!status.contains("43981"));
    #[cfg(feature = "sleep")]
    {
        let query = format!(
            "{:?}",
            SleepWindow {
                start_ms: 1_234_567_890,
                end_ms: 1_234_567_999
            }
        );
        assert!(query.contains("encoded_length"));
        assert!(!query.contains("1234567890"));
    }
    #[cfg(feature = "wear")]
    {
        let event = format!(
            "{:?}",
            EventBody::WearChange {
                kind: WearKind::Wear,
                timestamp: Timestamp {
                    utc_ms: 1_234_567_890,
                    valid: true
                },
                value: 2
            }
        );
        assert!(event.contains("encoded_length"));
        assert!(!event.contains("1234567890"));
        assert!(!event.contains("value: 2"));
    }
}

#[cfg(feature = "activity")]
#[test]
fn activity_realtime_cannot_smuggle_a_finalized_segment_as_full_current_kind4() {
    let mut backend = Backend::new();
    backend.events.push(BackendEvent {
        generation: 1,
        body: EventBody::ActivityCurrent(Snapshot {
            generation: 1,
            timestamp: stamp(),
            value: ActivityRecord {
                kind: 3,
                start_utc_ms: 10_000,
                end_utc_ms: 20_000,
                steps: 100,
                distance_mm: 1000,
                energy_j: 2000,
                valid_mask: 31,
            },
        }),
    });
    let owner = owner(backend);
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x23)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    run(&mut handler, 0x23, 2, &[]);
    assert_eq!(handler.next_event(&mut [0; 128]), Err(Failure::Contract));
}

#[cfg(feature = "sport")]
#[test]
fn review_should_keep_old_terminal_event_deliverable_when_next_sport_starts() {
    let mut backend = Backend::new();
    backend.events.push(BackendEvent {
        generation: 1,
        body: EventBody::SportEnd {
            reason: 0,
            result: Snapshot {
                generation: 1,
                timestamp: stamp(),
                value: summary(),
            },
        },
    });
    let owner = owner(backend);
    owner
        .with_sport(|engine| {
            engine.start(2, 2, 1000).unwrap();
            engine.control(SID, 3, 2000).unwrap();
            Ok(())
        })
        .unwrap();
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(3000));
    let entries = [metadata(0x22)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    assert_eq!(run(&mut handler, 0x22, 2, &[]).len(), 65);
    let reply = run(&mut handler, 0x22, 0x30, &[2, 0, 2]);
    assert_eq!(&reply[..4], &[0; 4]);
    acknowledge(&mut handler, 0x22, 0x30, &[2, 0, 2], &reply);
    let next = handler.next_event(&mut [0; 128]);
    eprintln!("old terminal event after confirmed new START: {next:?}");
    assert!(matches!(next, Ok(Some(event)) if event.opcode == 0x81 && event.length == 63));
}

#[cfg(feature = "sport")]
#[test]
fn review_should_release_ended_device_owned_context_slots_without_reusing_alias_ids() {
    let owner = owner(Backend::new());
    owner
        .with_sport(|engine| {
            engine.start(2, 2, 1000).unwrap();
            Ok(())
        })
        .unwrap();
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x22)];
    let mut handler = MotionHandler::<_, 1>::new(&owner, 1, &entries, &access, &clock).unwrap();
    let mut first = [0; 128];
    assert_eq!(
        handler.dispatch(frame(0x22, 0x34, &SID.to_le_bytes()), &mut first),
        Ok(8)
    );
    owner
        .with_sport(|engine| {
            engine.control(SID, 3, 2000).unwrap();
            engine.start(2, 2, 3000).unwrap();
            Ok(())
        })
        .unwrap();
    clock.0.set(3000);
    let mut second = [0; 128];
    let result = handler.dispatch(frame(0x22, 0x34, &(SID + 1).to_le_bytes()), &mut second);
    eprintln!(
        "context for next device-owned session: {result:?}, status={:?}",
        &second[..4]
    );
    assert_eq!(result, Ok(8));
    assert_eq!(&second[..4], &[0; 4]);
    assert_ne!(&first[4..8], &second[4..8]);
    let old = u32::from_le_bytes(first[4..8].try_into().unwrap());
    let current = u32::from_le_bytes(second[4..8].try_into().unwrap());
    assert_eq!(handler.resolve_context(old), Err(Failure::NotFound));
    assert_eq!(handler.resolve_context(current), Ok(SID + 1));
}

#[cfg(feature = "wear")]
#[test]
fn review_should_encode_resource_capacity_as_limit_exceeded_with_sufficient_output() {
    let owner = owner(Backend::new());
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x24)];
    let mut handler = MotionHandler::<_, 1>::new(&owner, 1, &entries, &access, &clock).unwrap();
    let mut out = [0; 128];
    assert_eq!(
        handler.dispatch(frame(0x24, 2, &[1, 0x24]), &mut out),
        Ok(38)
    );
    out.fill(0xa5);
    let result = handler.dispatch(frame(0x24, 2, &[2, 0x24]), &mut out);
    eprintln!(
        "full resource watch with 128B output: {result:?}, prefix={:?}",
        &out[..4]
    );
    assert_eq!(result, Ok(4));
    assert_eq!(&out[..4], &[7, 0, 0, 0]);
}

#[cfg(feature = "sport")]
#[test]
fn device_ui_stop_and_next_start_keep_actual_old_end_then_allow_new_realtime() {
    let mut backend = Backend::new();
    backend.events.push(BackendEvent {
        generation: 1,
        body: EventBody::SportEnd {
            reason: 0,
            result: Snapshot {
                generation: 1,
                timestamp: stamp(),
                value: summary(),
            },
        },
    });
    let owner = owner(backend);
    owner
        .with_sport(|sport| {
            assert_eq!(sport.start(2, 2, 1000), Ok(SID));
            Ok(())
        })
        .unwrap();
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x22)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    assert_eq!(run(&mut handler, 0x22, 2, &[]).len(), 65);
    owner
        .with_sport(|sport| {
            assert_eq!(sport.control(SID, 3, 2000), Ok(SportState::Stopped));
            assert_eq!(sport.start(2, 2, 3000), Ok(SID + 1));
            Ok(())
        })
        .unwrap();
    clock.0.set(3000);
    let mut bytes = [0; 128];
    let old = handler.next_event(&mut bytes).unwrap().unwrap();
    assert_eq!((old.opcode, old.length), (0x81, 63));
    assert_eq!(&bytes[17..25], &SID.to_le_bytes());
    handler.event_admitted(old.token).unwrap();
    owner
        .with_backend(|backend| {
            let mut value = summary();
            value.session = SID + 1;
            backend.latest_value = Some(Observation::Sport(Snapshot {
                generation: 2,
                timestamp: stamp(),
                value,
            }));
            backend.events.push(BackendEvent {
                generation: 2,
                body: EventBody::SportRealtime(SportRealtime {
                    session: SID + 1,
                    time_offset_ms: 0,
                    valid_mask: 1,
                    bpm: 75,
                    cadence_per_min: 0,
                    steps: 0,
                    rri_us: 0,
                }),
            });
            Ok(())
        })
        .unwrap();
    assert_eq!(run(&mut handler, 0x22, 2, &[]).len(), 65);
    let current = handler.next_event(&mut bytes).unwrap().unwrap();
    assert_eq!((current.opcode, current.length), (0x80, 26));
    assert_eq!(&bytes[..8], &(SID + 1).to_le_bytes());
    handler.event_admitted(current.token).unwrap();
    owner
        .with_backend(|backend| {
            assert_eq!(backend.confirmed, [1, 2]);
            Ok(())
        })
        .unwrap();
}

#[cfg(feature = "sport")]
#[test]
fn full_actual_stop_retention_rejects_new_start_but_never_blocks_stop_or_loses_old_proof() {
    let mut backend = Backend::new();
    backend.events.push(BackendEvent {
        generation: 1,
        body: EventBody::SportEnd {
            reason: 0,
            result: Snapshot {
                generation: 1,
                timestamp: stamp(),
                value: summary(),
            },
        },
    });
    let owner = owner(backend);
    owner
        .with_sport(|sport| {
            sport.start(2, 2, 1000)?;
            Ok(())
        })
        .unwrap();
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x22)];
    let mut handler = MotionHandler::<_, 1>::new(&owner, 1, &entries, &access, &clock).unwrap();
    let mut reply = [0; 128];
    assert_eq!(handler.dispatch(frame(0x22, 2, &[]), &mut reply), Ok(65));
    let mut now = 1000;
    owner
        .with_sport(|sport| {
            for index in 0..SPORT_PROOF_CAPACITY {
                let index = u64::try_from(index).unwrap();
                if index != 0 {
                    now += 1000;
                    assert_eq!(sport.start(2, 2, now), Ok(SID + index));
                }
                now += 1000;
                assert_eq!(sport.control(SID + index, 3, now), Ok(SportState::Stopped));
            }
            now += 1000;
            assert_eq!(sport.start(2, 2, now), Err(Failure::Capacity));
            assert_eq!(sport.session(SID).unwrap().state(), Ok(SportState::Stopped));
            Ok(())
        })
        .unwrap();
    owner
        .with_backend(|backend| {
            assert_eq!(
                backend
                    .actions
                    .iter()
                    .filter(|action| **action == "start")
                    .count(),
                SPORT_PROOF_CAPACITY
            );
            assert_eq!(
                backend
                    .actions
                    .iter()
                    .filter(|action| **action == "stop")
                    .count(),
                SPORT_PROOF_CAPACITY
            );
            Ok(())
        })
        .unwrap();
    clock.0.set(now);
    let mut out = [0; 128];
    let old = handler.next_event(&mut out).unwrap().unwrap();
    assert_eq!(&out[17..25], &SID.to_le_bytes());
    handler.event_admitted(old.token).unwrap();
    let next = SID + u64::try_from(SPORT_PROOF_CAPACITY).unwrap();
    owner
        .with_sport(|sport| {
            now += 1000;
            assert_eq!(sport.start(2, 2, now), Ok(next));
            Ok(())
        })
        .unwrap();
    handler.connection_closed().unwrap();
    owner
        .with_sport(|sport| {
            assert_eq!(
                sport.session(next).unwrap().state(),
                Ok(SportState::Running)
            );
            now += 1000;
            assert_eq!(sport.control(next, 3, now), Ok(SportState::Stopped));
            now += 1000;
            assert_eq!(sport.start(2, 2, now), Ok(next + 1));
            Ok(())
        })
        .unwrap();
}

#[cfg(feature = "sport")]
#[test]
fn app_stop_cannot_be_blocked_by_old_terminal_watch_and_both_real_ends_are_retained() {
    let mut backend = Backend::new();
    let mut second = summary();
    second.session = SID + 1;
    for (generation, value) in [(1, summary()), (2, second)] {
        backend.events.push(BackendEvent {
            generation,
            body: EventBody::SportEnd {
                reason: 0,
                result: Snapshot {
                    generation,
                    timestamp: stamp(),
                    value,
                },
            },
        });
    }
    let owner = owner(backend);
    owner
        .with_sport(|sport| {
            sport.start(2, 2, 1000)?;
            sport.control(SID, 3, 2000)?;
            Ok(())
        })
        .unwrap();
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(3000));
    let entries = [metadata(0x22)];
    let mut handler = MotionHandler::<_, 1>::new(&owner, 1, &entries, &access, &clock).unwrap();
    let mut out = [0; 128];
    assert_eq!(handler.dispatch(frame(0x22, 2, &[]), &mut out), Ok(65));
    owner
        .with_sport(|sport| {
            assert_eq!(sport.start(2, 2, 3000), Ok(SID + 1));
            Ok(())
        })
        .unwrap();
    clock.0.set(4000);
    let stop = command(SID + 1, 3);
    let request = frame(0x22, 0x31, &stop);
    assert_eq!(handler.dispatch(request, &mut out), Ok(5));
    assert_eq!(&out[..5], &[0, 0, 0, 0, 3]);
    handler.response_admitted(request, &out[..5]).unwrap();
    for id in [SID, SID + 1] {
        let event = handler.next_event(&mut out).unwrap().unwrap();
        assert_eq!((event.opcode, event.length), (0x81, 63));
        assert_eq!(&out[17..25], &id.to_le_bytes());
        handler.event_admitted(event.token).unwrap();
    }
    owner
        .with_backend(|backend| {
            assert_eq!(backend.actions, ["start", "stop", "start", "stop"]);
            assert_eq!(backend.confirmed, [1, 2]);
            Ok(())
        })
        .unwrap();
}

#[cfg(feature = "sport")]
#[test]
fn deferred_stops_remain_fifo_when_an_earlier_static_slot_is_reused() {
    let mut backend = Backend::new();
    for offset in 0..4 {
        let mut value = summary();
        value.session += offset;
        let generation = offset + 1;
        backend.events.push(BackendEvent {
            generation,
            body: EventBody::SportEnd {
                reason: 0,
                result: Snapshot {
                    generation,
                    timestamp: stamp(),
                    value,
                },
            },
        });
    }
    let owner = owner(backend);
    owner
        .with_sport(|sport| {
            sport.start(2, 2, 1000)?;
            sport.control(SID, 3, 2000)?;
            Ok(())
        })
        .unwrap();
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(3000));
    let entries = [metadata(0x22)];
    let mut handler = MotionHandler::<_, 1>::new(&owner, 1, &entries, &access, &clock).unwrap();
    let mut out = [0; 128];
    assert_eq!(handler.dispatch(frame(0x22, 2, &[]), &mut out), Ok(65));
    let mut now = 3000;
    for offset in 1..=3 {
        owner
            .with_sport(|sport| {
                assert_eq!(sport.start(2, 2, now), Ok(SID + offset));
                Ok(())
            })
            .unwrap();
        now += 1000;
        clock.0.set(now);
        let stop = command(SID + offset, 3);
        let request = frame(0x22, 0x31, &stop);
        assert_eq!(handler.dispatch(request, &mut out), Ok(5));
        handler.response_admitted(request, &out[..5]).unwrap();
        if offset == 2 {
            let oldest = handler.next_event(&mut out).unwrap().unwrap();
            assert_eq!(&out[17..25], &SID.to_le_bytes());
            handler.event_admitted(oldest.token).unwrap();
            // Promote B so its old deferred slot becomes reusable by D while C is still waiting.
            assert_eq!(handler.next_event(&mut out).unwrap().unwrap().opcode, 0x81);
        }
        now += 1000;
    }
    for offset in 1..=3 {
        let event = handler.next_event(&mut out).unwrap().unwrap();
        assert_eq!(&out[17..25], &(SID + offset).to_le_bytes());
        handler.event_admitted(event.token).unwrap();
    }
    owner
        .with_backend(|backend| {
            assert_eq!(backend.confirmed, [1, 2, 3, 4]);
            Ok(())
        })
        .unwrap();
}

#[cfg(feature = "sport")]
#[test]
fn revoked_deferred_stop_scope_closes_app_without_repeating_or_undoing_real_stop() {
    struct Scope(Cell<u64>);
    impl MotionAccess for Scope {
        fn allowed(&self, _: u8, _: Operation, resource: Resource) -> bool {
            resource != Resource::Session(self.0.get())
        }
    }
    let owner = owner(Backend::new());
    owner
        .with_sport(|sport| {
            sport.start(2, 2, 1000)?;
            sport.control(SID, 3, 2000)?;
            Ok(())
        })
        .unwrap();
    let access = Scope(Cell::new(0));
    let clock = Clock(Cell::new(3000));
    let entries = [metadata(0x22)];
    let mut handler = MotionHandler::<_, 1>::new(&owner, 1, &entries, &access, &clock).unwrap();
    let mut out = [0; 128];
    assert_eq!(handler.dispatch(frame(0x22, 2, &[]), &mut out), Ok(65));
    owner
        .with_sport(|sport| {
            sport.start(2, 2, 3000)?;
            Ok(())
        })
        .unwrap();
    clock.0.set(4000);
    let stop = command(SID + 1, 3);
    assert_eq!(handler.dispatch(frame(0x22, 0x31, &stop), &mut out), Ok(5));
    access.0.set(SID + 1);
    assert!(handler.must_end_session());
    out.fill(0xa5);
    assert_eq!(handler.next_event(&mut out), Err(Failure::Closed));
    assert_eq!(out, [0; 128]);
    owner
        .with_backend(|backend| {
            assert_eq!(backend.actions, ["start", "stop", "start", "stop"]);
            Ok(())
        })
        .unwrap();
}

#[cfg(all(feature = "sport", feature = "wear"))]
#[test]
fn deferred_stop_can_emit_even_when_all_normal_watches_are_unrelated_live_sources() {
    struct Roles;
    impl MotionAccess for Roles {
        fn allowed(&self, module: u8, operation: Operation, _: Resource) -> bool {
            module != 0x24 || operation != Operation::Event
        }
    }
    let mut backend = Backend::new();
    backend.events.push(BackendEvent {
        generation: 1,
        body: EventBody::SportEnd {
            reason: 0,
            result: Snapshot {
                generation: 1,
                timestamp: stamp(),
                value: summary(),
            },
        },
    });
    let owner = owner(backend);
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x22), metadata(0x24)];
    let mut handler = MotionHandler::<_, 2>::new(&owner, 1, &entries, &Roles, &clock).unwrap();
    let mut out = [0; 128];
    assert_eq!(
        handler.dispatch(frame(0x24, 2, &[1, 0x24]), &mut out),
        Ok(38)
    );
    assert_eq!(
        handler.dispatch(frame(0x24, 2, &[2, 0x24]), &mut out),
        Ok(38)
    );
    owner
        .with_sport(|sport| {
            assert_eq!(sport.start(2, 2, 1000), Ok(SID));
            Ok(())
        })
        .unwrap();
    clock.0.set(2000);
    let stop = command(SID, 3);
    let request = frame(0x22, 0x31, &stop);
    assert_eq!(handler.dispatch(request, &mut out), Ok(5));
    handler.response_admitted(request, &out[..5]).unwrap();
    let event = handler
        .next_event(&mut out)
        .unwrap()
        .expect("unrelated retained observations must not permanently block actual stopped E81");
    assert_eq!((event.opcode, event.length), (0x81, 63));
    handler.event_admitted(event.token).unwrap();
    owner
        .with_backend(|backend| {
            assert_eq!(backend.confirmed, [1]);
            Ok(())
        })
        .unwrap();
}

#[cfg(feature = "sport")]
#[test]
fn recheck_should_not_rearm_delivered_stop_as_a_fifo_blocker() {
    let mut backend = Backend::new();
    for offset in 0..2 {
        let mut value = summary();
        value.session += offset;
        backend.events.push(BackendEvent {
            generation: offset + 1,
            body: EventBody::SportEnd {
                reason: 0,
                result: Snapshot {
                    generation: offset + 1,
                    timestamp: stamp(),
                    value,
                },
            },
        });
    }
    let owner = owner(backend);
    owner
        .with_sport(|sport| {
            sport.start(2, 2, 1000)?;
            Ok(())
        })
        .unwrap();
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(1000));
    let entries = [metadata(0x22)];
    let mut handler = MotionHandler::<_, 1>::new(&owner, 1, &entries, &access, &clock).unwrap();
    let mut out = [0; 128];
    let stop_a = command(SID, 3);
    clock.0.set(2000);
    assert_eq!(
        handler.dispatch(frame(0x22, 0x31, &stop_a), &mut out),
        Ok(5)
    );
    handler
        .response_admitted(frame(0x22, 0x31, &stop_a), &out[..5])
        .unwrap();
    let first = handler.next_event(&mut out).unwrap().unwrap();
    assert_eq!(&out[17..25], &SID.to_le_bytes());
    handler.event_admitted(first.token).unwrap();
    assert_eq!(handler.next_event(&mut out), Ok(None));
    // A new MID repeats a genuinely terminal STOP after its only E81 was admitted.
    let mut repeat = frame(0x22, 0x31, &stop_a);
    repeat.header.message_id = 2;
    assert_eq!(handler.dispatch(repeat, &mut out), Ok(5));
    handler.response_admitted(repeat, &out[..5]).unwrap();
    owner
        .with_sport(|sport| {
            sport.start(2, 2, 3000)?;
            Ok(())
        })
        .unwrap();
    clock.0.set(4000);
    let stop_b = command(SID + 1, 3);
    let mut request_b = frame(0x22, 0x31, &stop_b);
    request_b.header.message_id = 3;
    assert_eq!(handler.dispatch(request_b, &mut out), Ok(5));
    handler.response_admitted(request_b, &out[..5]).unwrap();
    let second = handler.next_event(&mut out);
    eprintln!("B.E81 after delivered A STOP is repeated: {second:?}");
    assert!(matches!(second, Ok(Some(event)) if event.opcode == 0x81));
    assert_eq!(&out[17..25], &(SID + 1).to_le_bytes());
}

#[cfg(feature = "sport")]
fn delivered_terminal_nonstop_probe(opcode: u8) {
    let mut backend = Backend::new();
    for offset in 0..2 {
        let mut value = summary();
        value.session += offset;
        let generation = offset + 1;
        backend.events.push(BackendEvent {
            generation,
            body: EventBody::SportEnd {
                reason: 0,
                result: Snapshot {
                    generation,
                    timestamp: stamp(),
                    value,
                },
            },
        });
    }
    let owner = owner(backend);
    owner
        .with_sport(|sport| {
            sport.start(2, 2, 1000)?;
            Ok(())
        })
        .unwrap();
    let access = Access(Cell::new(true));
    let clock = Clock(Cell::new(2000));
    let entries = [metadata(0x22)];
    let mut handler = MotionHandler::<_, 1>::new(&owner, 1, &entries, &access, &clock).unwrap();
    let mut out = [0; 128];
    let stop_a = command(SID, 3);
    let request = frame(0x22, 0x31, &stop_a);
    assert_eq!(handler.dispatch(request, &mut out), Ok(5));
    handler.response_admitted(request, &out[..5]).unwrap();
    let first = handler.next_event(&mut out).unwrap().unwrap();
    handler.event_admitted(first.token).unwrap();
    let save = command(SID, 2);
    let body = if opcode == 2 { &[][..] } else { &save[..] };
    let mut historical = frame(0x22, opcode, body);
    historical.header.message_id = 2;
    assert_eq!(
        handler.dispatch(historical, &mut out),
        Ok(if opcode == 2 { 65 } else { 5 })
    );
    owner
        .with_sport(|sport| {
            sport.start(2, 2, 3000)?;
            Ok(())
        })
        .unwrap();
    clock.0.set(4000);
    if opcode == 2 {
        historical.header.message_id = 3;
        assert_eq!(handler.dispatch(historical, &mut out), Ok(65));
    }
    let stop_b = command(SID + 1, 3);
    let mut request = frame(0x22, 0x31, &stop_b);
    request.header.message_id = 4;
    assert_eq!(handler.dispatch(request, &mut out), Ok(5));
    handler.response_admitted(request, &out[..5]).unwrap();
    let second = handler
        .next_event(&mut out)
        .unwrap()
        .expect("delivered terminal history must not create an empty watch blocking B");
    assert_eq!(&out[17..25], &(SID + 1).to_le_bytes());
    handler.event_admitted(second.token).unwrap();
    owner
        .with_backend(|backend| {
            assert_eq!(backend.confirmed, [1, 2]);
            assert_eq!(backend.actions, ["start", "stop", "start", "stop"]);
            Ok(())
        })
        .unwrap();
}
#[cfg(feature = "sport")]
#[test]
fn delivered_terminal_latest_does_not_rearm_old_event_subscription() {
    delivered_terminal_nonstop_probe(2);
}
#[cfg(feature = "sport")]
#[test]
fn delivered_terminal_save_query_does_not_rearm_old_event_subscription() {
    delivered_terminal_nonstop_probe(0x32);
}
