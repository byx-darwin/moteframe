//! Synthetic ports exercise state contracts; no physical device/algorithm is installed.
#![cfg(any(feature = "rri", feature = "uv", feature = "womens-health"))]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use core::cell::Cell;
use device_protocol::{Frame, Header, Kind, ModuleEntry};
use device_services::{ServiceHandler, SessionError};
use device_wellness::Timestamp;
#[cfg(feature = "womens-health")]
use device_wellness::{BackendError, DayNumber, WomensBackend, WomensConfig, WomensMode};
#[cfg(feature = "rri")]
use device_wellness::{BeatClass, RriEvent, RriObservation, RriOrigin, RriTracker};
#[cfg(feature = "uv")]
use device_wellness::{
    UvAlarmEvent, UvEngine, UvPolicy, UvProvenance, UvReading, UvReportEvent, UvSample, UvSource,
    UvValueOrigin,
};
use device_wellness_service::*;
#[cfg(feature = "rri")]
const MEASUREMENT: u64 = 0x1122_3344_5566_7788;
#[cfg(feature = "rri")]
const PARENT: u64 = 0x8877_6655_4433_2211;
struct Roles {
    allow: Cell<bool>,
    source: Cell<bool>,
}
impl Roles {
    fn new() -> Self {
        Self {
            allow: Cell::new(true),
            source: Cell::new(true),
        }
    }
}
impl Access for Roles {
    fn allowed(&self, _: u8, _: Operation, _: Resource) -> bool {
        self.allow.get()
    }
    fn source_allowed(&self, _: u8, _: Origin) -> bool {
        self.source.get()
    }
}
struct Timer(Cell<u64>);
impl Clock for Timer {
    fn now(&self) -> Result<ClockReading, Failure> {
        Ok(ClockReading {
            monotonic_us: self.0.get(),
            timestamp: Timestamp::new(10_000, true).unwrap(),
        })
    }
}
#[cfg(feature = "uv")]
struct UvPort {
    engine: UvEngine<4>,
    generation: u64,
    identity: UvIdentityPolicy,
    revision: u64,
    active: Option<u32>,
    cancelled: Option<u32>,
    applied: Vec<u32>,
    checks: Cell<u32>,
    origin: UvValueOrigin,
    cancel_outcome: Option<Result<(), Failure>>,
    preview_failure: Option<Failure>,
}
#[cfg(feature = "uv")]
impl UvPort {
    fn new(report: bool) -> Self {
        let mut engine = UvEngine::new(
            UvPolicy {
                report_enabled: report,
                alarm_enabled: true,
                threshold_x100: 300,
                interval_s: 1,
            },
            1,
        )
        .unwrap();
        engine
            .ingest(UvSample {
                sample_id: 1,
                monotonic_ms: 1000,
                timestamp: Timestamp::new(1000, true).unwrap(),
                source: UvSource::DeviceSampling,
                reading: UvReading {
                    index_x100: Some(500),
                    intensity_uw_cm2: Some(20),
                },
            })
            .unwrap();
        let active = engine.pending_alarm().map(|alarm| alarm.event_id);
        Self {
            engine,
            generation: 77,
            identity: UvIdentityPolicy::PersistentNonReuse,
            revision: 1,
            active,
            cancelled: None,
            applied: Vec::new(),
            checks: Cell::new(0),
            origin: UvValueOrigin::DerivedEstimate,
            cancel_outcome: None,
            preview_failure: None,
        }
    }
    fn drained_alarm() -> Self {
        let mut port = Self::new(false);
        let alarm = port.engine.pending_alarm().unwrap();
        port.engine
            .confirm_alarm(alarm.event_id, alarm.transition)
            .unwrap();
        port
    }
}
#[cfg(feature = "uv")]
impl UvControlPort for UvPort {
    fn identity_policy(&self) -> UvIdentityPolicy {
        self.identity
    }
    fn generation(&self) -> u64 {
        self.generation
    }
    fn provenance(&self) -> UvProvenance<'_> {
        UvProvenance {
            origin: self.origin,
            provider_id: "actual-test-fixture",
            provider_version: "1",
        }
    }
    fn state(&self) -> &dyn UvState {
        &self.engine
    }
    fn validate_cancel(&self, id: u32) -> Result<CancelTicket, Failure> {
        self.checks.set(self.checks.get() + 1);
        if let Some(failure) = self.preview_failure {
            return Err(failure);
        }
        if id == 0 || self.active != Some(id) && self.cancelled != Some(id) {
            return Err(Failure::NotFound);
        }
        if self.engine.pending_alarm().is_some() && self.cancelled != Some(id) {
            return Err(Failure::Busy);
        }
        Ok(CancelTicket {
            generation: self.generation,
            revision: self.revision,
            event_id: id,
            already_cancelled: self.cancelled == Some(id),
        })
    }
    fn ticket_current(&self, ticket: CancelTicket) -> bool {
        ticket.generation == self.generation
            && ticket.revision == self.revision
            && (self.active == Some(ticket.event_id) || self.cancelled == Some(ticket.event_id))
            && ticket.already_cancelled == (self.cancelled == Some(ticket.event_id))
    }
    fn cancel_atomic(
        &mut self,
        _connection: u64,
        ticket: CancelTicket,
        when: ClockReading,
        access: &dyn Access,
    ) -> Result<(), Failure> {
        if let Some(outcome) = self.cancel_outcome {
            return outcome;
        }
        if !access.allowed(0x2e, Operation::Cancel, Resource::Alarm(ticket.event_id)) {
            return Err(Failure::Denied);
        }
        if !self.ticket_current(ticket) {
            return Err(Failure::Conflict);
        }
        if self
            .engine
            .request_cancel(ticket.event_id, UvSource::AppRequest, when.timestamp)
            .map_err(|_| Failure::Backend)?
        {
            self.applied.push(ticket.event_id);
            self.cancelled = Some(ticket.event_id);
            self.revision += 1;
        }
        self.engine
            .confirm_cancel_response(ticket.event_id)
            .map_err(|_| Failure::Backend)
    }
    fn event_admitted(&mut self, event: UvEvent) -> Result<(), Failure> {
        match event {
            UvEvent::Report(value) => self.engine.confirm_report(value.event_id),
            UvEvent::Alarm(value) => self.engine.confirm_alarm(value.event_id, value.transition),
        }
        .map_err(|_| Failure::Backend)
    }
}
struct Ports {
    selected: bool,
    status: ModuleStatus,
    detaches: u32,
    detach_failure: Option<Failure>,
    #[cfg(feature = "rri")]
    tracker: RriTracker,
    #[cfg(feature = "rri")]
    rri_generation: u64,
    #[cfg(feature = "rri")]
    view_reads: Cell<u32>,
    #[cfg(feature = "rri")]
    fail_second_view: bool,
    #[cfg(feature = "rri")]
    provider: &'static str,
    #[cfg(feature = "rri")]
    origin: RriOrigin,
    #[cfg(feature = "rri")]
    acknowledged: Vec<(u64, u32)>,
    #[cfg(feature = "uv")]
    uv: UvPort,
    #[cfg(feature = "womens-health")]
    config: Option<WomensConfig>,
}
impl Ports {
    fn new() -> Self {
        Self {
            selected: true,
            status: ModuleStatus {
                state: 1,
                reason: 0,
            },
            detaches: 0,
            detach_failure: None,
            #[cfg(feature = "rri")]
            tracker: RriTracker::new(MEASUREMENT, Some(PARENT), 0).unwrap(),
            #[cfg(feature = "rri")]
            rri_generation: 55,
            #[cfg(feature = "rri")]
            view_reads: Cell::new(0),
            #[cfg(feature = "rri")]
            fail_second_view: false,
            #[cfg(feature = "rri")]
            provider: "actual-test-beats",
            #[cfg(feature = "rri")]
            origin: RriOrigin::DeviceAlgorithm,
            #[cfg(feature = "rri")]
            acknowledged: Vec::new(),
            #[cfg(feature = "uv")]
            uv: UvPort::drained_alarm(),
            #[cfg(feature = "womens-health")]
            config: Some(WomensConfig {
                enabled: true,
                mode: WomensMode::Cycle {
                    anchor: DayNumber::new(20_000),
                    duration_days: 5,
                    cycle_days: 28,
                },
            }),
        }
    }
}
#[cfg(feature = "womens-health")]
impl WomensBackend for Ports {
    fn available(&self) -> bool {
        self.selected
    }
    fn load(&self) -> Result<Option<WomensConfig>, BackendError> {
        Ok(self.config)
    }
    fn store_atomic(&mut self, value: WomensConfig) -> Result<(), BackendError> {
        self.config = Some(value);
        Ok(())
    }
}
impl Backend for Ports {
    fn selected(&self, _: u8) -> bool {
        self.selected
    }
    fn status(&self, _: u8) -> Result<ModuleStatus, Failure> {
        Ok(self.status)
    }
    #[cfg(feature = "rri")]
    fn rri_view(&self, _connection: u64) -> Result<RriView<'_>, Failure> {
        self.view_reads.set(self.view_reads.get() + 1);
        if self.fail_second_view && self.view_reads.get() > 1 {
            return Err(Failure::Conflict);
        }
        Ok(RriView {
            generation: self.rri_generation,
            tracker: &self.tracker,
            timestamp: Timestamp::new(1000, true).unwrap(),
            origin: self.origin,
            provider_id: self.provider,
            provider_version: "1",
        })
    }
    #[cfg(feature = "rri")]
    fn rri_event_admitted(
        &mut self,
        _connection: u64,
        generation: u64,
        measurement: u64,
        index: u32,
    ) -> Result<(), Failure> {
        if generation != self.rri_generation {
            return Err(Failure::Conflict);
        }
        self.tracker
            .confirm(measurement, index)
            .map_err(|_| Failure::Contract)?;
        self.acknowledged.push((measurement, index));
        Ok(())
    }
    #[cfg(feature = "uv")]
    fn uv(&self) -> Result<&dyn UvControlPort, Failure> {
        Ok(&self.uv)
    }
    #[cfg(feature = "uv")]
    fn uv_mut(&mut self) -> Result<&mut dyn UvControlPort, Failure> {
        Ok(&mut self.uv)
    }
    #[cfg(feature = "womens-health")]
    fn womens(&self) -> Result<&dyn WomensBackend, Failure> {
        Ok(self)
    }
    fn detach(&mut self, _connection: u64) -> Result<(), Failure> {
        self.detaches += 1;
        if let Some(error) = self.detach_failure {
            Err(error)
        } else {
            Ok(())
        }
    }
}
fn entry(module: u8) -> ModuleEntry {
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
fn req(module: u8, opcode: u8, bytes: &[u8]) -> Frame<'_> {
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
type Handler<'a> = WellnessHandler<'a, Ports>;
fn run(handler: &mut Handler<'_>, module: u8, opcode: u8, bytes: &[u8]) -> Vec<u8> {
    let mut out = [0xa5; 128];
    let length = handler
        .dispatch(req(module, opcode, bytes), &mut out)
        .unwrap();
    out[..length].to_vec()
}
#[test]
fn only_business1_actual_ports_can_be_registered() {
    let owner = WellnessOwner::new(Ports::new());
    let access = Roles::new();
    let clock = Timer(Cell::new(2_000_000));
    let module = if cfg!(feature = "rri") {
        0x2c
    } else if cfg!(feature = "uv") {
        0x2e
    } else {
        0x2d
    };
    let mut wrong = entry(module);
    wrong.minor = 2;
    assert_eq!(
        Handler::new(&owner, 1, &[wrong], &access, &clock).err(),
        Some(Failure::Invalid)
    );
    owner
        .with_backend(|ports| {
            ports.selected = false;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        Handler::new(&owner, 1, &[entry(module)], &access, &clock).err(),
        Some(Failure::Unsupported)
    );
}
#[test]
fn actual_status_and_strict_empty_body_never_predict_a_measurement() {
    let owner = WellnessOwner::new(Ports::new());
    let access = Roles::new();
    let clock = Timer(Cell::new(2_000_000));
    let module = if cfg!(feature = "rri") {
        0x2c
    } else if cfg!(feature = "uv") {
        0x2e
    } else {
        0x2d
    };
    let entries = [entry(module)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    assert_eq!(run(&mut handler, module, 1, &[]), [0, 0, 0, 0, 1, 0, 0]);
    assert_eq!(run(&mut handler, module, 1, &[0]), [1, 0, 0, 0]);
}
#[cfg(feature = "rri")]
#[test]
fn latest_reads_actual_device_tracker_full_parent_and_no_new_acquisition() {
    let mut ports = Ports::new();
    ports
        .tracker
        .accept(
            RriObservation {
                event: RriEvent {
                    measurement: MEASUREMENT,
                    index: 0,
                    cumulative_us: 1_000_000,
                    interval_us: None,
                },
                class: BeatClass::Baseline,
            },
            1_000_000,
        )
        .unwrap();
    ports.tracker.confirm(MEASUREMENT, 0).unwrap();
    ports
        .tracker
        .accept(
            RriObservation {
                event: RriEvent {
                    measurement: MEASUREMENT,
                    index: 1,
                    cumulative_us: 1_800_000,
                    interval_us: Some(800_000),
                },
                class: BeatClass::Interval,
            },
            1_800_000,
        )
        .unwrap();
    let owner = WellnessOwner::new(ports);
    let access = Roles::new();
    let clock = Timer(Cell::new(2_000_000));
    let entries = [entry(0x2c)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    let out = run(&mut handler, 0x2c, 2, &[]);
    assert_eq!(out.len(), 35);
    assert_eq!(&out[4..8], &[1, 0x2c, 1, 0]);
    assert_eq!(&out[19..27], &MEASUREMENT.to_le_bytes());
    assert_eq!(&out[27..31], &2000u32.to_le_bytes());
    assert_eq!(&out[31..35], &1u32.to_le_bytes());
    handler.connection_closed().unwrap();
    owner
        .with_backend(|ports| {
            assert_eq!(ports.tracker.parent_sport(), Some(PARENT));
            assert_eq!(
                ports.tracker.summary_at(3_000_000).unwrap().duration_ms,
                3000
            );
            assert_eq!(ports.detaches, 1);
            Ok(())
        })
        .unwrap();
}
#[cfg(feature = "rri")]
#[test]
fn rri_event_is_exact25_bytes_and_retry_ack_is_not_a_second_candidate() {
    let mut ports = Ports::new();
    ports
        .tracker
        .accept(
            RriObservation {
                event: RriEvent {
                    measurement: MEASUREMENT,
                    index: 0,
                    cumulative_us: 1_000_000,
                    interval_us: None,
                },
                class: BeatClass::Baseline,
            },
            1_000_000,
        )
        .unwrap();
    let owner = WellnessOwner::new(ports);
    let access = Roles::new();
    let clock = Timer(Cell::new(2_000_000));
    let entries = [entry(0x2c)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    handler.subscribe(0x2c).unwrap();
    let mut out = [0xa5; 128];
    let event = handler.next_event(&mut out).unwrap().unwrap();
    assert_eq!((event.module, event.opcode, event.length), (0x2c, 0x80, 25));
    assert_eq!(&out[..8], &MEASUREMENT.to_le_bytes());
    assert_eq!(&out[20..25], &[0; 5]);
    assert!(RriEvent::decode(&out[..25]).is_ok());
    assert!(RriEvent::decode(&out[..29]).is_err());
    let first = out;
    assert_eq!(handler.next_event(&mut out), Ok(Some(event)));
    assert_eq!(out, first);
    handler.event_admitted(event.token).unwrap();
    handler.event_admitted(event.token).unwrap();
    owner
        .with_backend(|ports| {
            assert_eq!(ports.acknowledged, [(MEASUREMENT, 0)]);
            Ok(())
        })
        .unwrap();
}
#[cfg(feature = "rri")]
#[test]
fn missing_source_metadata_or_import_permission_cannot_masquerade_as_sensor() {
    let mut ports = Ports::new();
    ports.provider = "";
    let owner = WellnessOwner::new(ports);
    let access = Roles::new();
    let clock = Timer(Cell::new(2_000_000));
    let entries = [entry(0x2c)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    assert_eq!(run(&mut handler, 0x2c, 2, &[]), [17, 0, 0, 0]);
    owner
        .with_backend(|ports| {
            ports.provider = "actual-importer";
            ports.origin = RriOrigin::PhoneImport;
            Ok(())
        })
        .unwrap();
    access.source.set(false);
    assert_eq!(run(&mut handler, 0x2c, 2, &[]), [5, 0, 3, 255]);
}
#[cfg(feature = "uv")]
#[test]
fn cancel_dispatch_is_pure_and_unadmitted_disconnect_has_no_cancel_tombstone() {
    let owner = WellnessOwner::new(Ports::new());
    let access = Roles::new();
    let clock = Timer(Cell::new(2_000_000));
    let entries = [entry(0x2e)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    assert_eq!(run(&mut handler, 0x2e, 0x30, &1u32.to_le_bytes()), [0; 4]);
    owner
        .with_backend(|ports| {
            assert!(ports.uv.applied.is_empty());
            assert_eq!(ports.uv.cancelled, None);
            assert_eq!(ports.uv.engine.pending_alarm(), None);
            Ok(())
        })
        .unwrap();
    handler.connection_closed().unwrap();
    owner
        .with_backend(|ports| {
            assert!(ports.uv.applied.is_empty());
            assert_eq!(ports.uv.cancelled, None);
            assert!(ports.uv.validate_cancel(1).is_ok());
            Ok(())
        })
        .unwrap();
}
#[cfg(feature = "uv")]
#[test]
fn exact_response_admission_applies_cancel_once_then_emits_plain22byte_e81() {
    let owner = WellnessOwner::new(Ports::new());
    let access = Roles::new();
    let clock = Timer(Cell::new(2_000_000));
    let entries = [entry(0x2e)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    let body = 1u32.to_le_bytes();
    let response = run(&mut handler, 0x2e, 0x30, &body);
    let mut out = [0xa5; 128];
    assert_eq!(handler.next_event(&mut out), Ok(None));
    let mut wrong = req(0x2e, 0x30, &body);
    wrong.header.message_id = 2;
    handler.response_admitted(wrong, &response).unwrap();
    owner
        .with_backend(|ports| {
            assert!(ports.uv.applied.is_empty());
            Ok(())
        })
        .unwrap();
    handler
        .response_admitted(req(0x2e, 0x30, &body), &response)
        .unwrap();
    handler
        .response_admitted(req(0x2e, 0x30, &body), &response)
        .unwrap();
    let event = handler.next_event(&mut out).unwrap().unwrap();
    assert_eq!((event.module, event.opcode, event.length), (0x2e, 0x81, 22));
    assert_eq!(&out[..6], &[1, 0, 0, 0, 3, 3]);
    assert!(UvAlarmEvent::decode(&out[..22]).is_ok());
    assert!(UvAlarmEvent::decode(&out[..26]).is_err());
    handler.event_admitted(event.token).unwrap();
    let response = run(&mut handler, 0x2e, 0x30, &body);
    handler
        .response_admitted(req(0x2e, 0x30, &body), &response)
        .unwrap();
    assert_eq!(handler.next_event(&mut out), Ok(None));
    owner
        .with_backend(|ports| {
            assert_eq!(ports.uv.applied, [1]);
            Ok(())
        })
        .unwrap();
}
#[cfg(feature = "uv")]
#[test]
fn uv_reports_and_trigger_transitions_are_exact21_and22_without_crc() {
    let mut ports = Ports::new();
    ports.uv = UvPort::new(true);
    let owner = WellnessOwner::new(ports);
    let access = Roles::new();
    let clock = Timer(Cell::new(2_000_000));
    let entries = [entry(0x2e)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    let latest = run(&mut handler, 0x2e, 2, &[]);
    assert_eq!(latest.len(), 26);
    assert_eq!(&latest[4..8], &[1, 0x2e, 1, 0]);
    let mut out = [0xa5; 128];
    let mut seen = Vec::new();
    while let Some(event) = handler.next_event(&mut out).unwrap() {
        seen.push((event.opcode, event.length));
        if event.opcode == 0x80 {
            assert!(UvReportEvent::decode(&out[..21]).is_ok());
        } else {
            assert!(UvAlarmEvent::decode(&out[..22]).is_ok());
        }
        handler.event_admitted(event.token).unwrap();
    }
    assert_eq!(seen, [(0x80, 21), (0x81, 22)]);
}
#[cfg(feature = "uv")]
#[test]
fn stale_cancel_receipt_and_revocation_never_silence_actual_alarm() {
    let owner = WellnessOwner::new(Ports::new());
    let access = Roles::new();
    let clock = Timer(Cell::new(2_000_000));
    let entries = [entry(0x2e)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    let body = 1u32.to_le_bytes();
    let response = run(&mut handler, 0x2e, 0x30, &body);
    owner
        .with_backend(|ports| {
            ports.uv.revision += 1;
            Ok(())
        })
        .unwrap();
    assert!(!handler.replay_valid(req(0x2e, 0x30, &body), &response));
    assert_eq!(
        handler.response_admitted(req(0x2e, 0x30, &body), &response),
        Err(SessionError::InvalidHandlerResponse)
    );
    owner
        .with_backend(|ports| {
            assert!(ports.uv.applied.is_empty());
            assert_eq!(ports.uv.cancelled, None);
            Ok(())
        })
        .unwrap();
}
#[cfg(feature = "womens-health")]
#[test]
fn womens_only_status_reads_actual_config_and_never_registers_latest_or_prediction() {
    let owner = WellnessOwner::new(Ports::new());
    let access = Roles::new();
    let clock = Timer(Cell::new(2_000_000));
    let entries = [entry(0x2d)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    assert_eq!(handler.required_features(&entries[0], 1), Some(0));
    assert_eq!(handler.required_features(&entries[0], 2), None);
    assert_eq!(run(&mut handler, 0x2d, 1, &[]), [0, 0, 0, 0, 1, 0, 0]);
    assert_eq!(run(&mut handler, 0x2d, 2, &[]), [3, 0, 0, 0]);
    owner
        .with_backend(|ports| {
            ports.config = None;
            Ok(())
        })
        .unwrap();
    assert_eq!(run(&mut handler, 0x2d, 1, &[]), [8, 0, 0, 0]);
}

#[test]
fn clock_regression_clears_response_and_cannot_reopen_old_connection() {
    let owner = WellnessOwner::new(Ports::new());
    let access = Roles::new();
    let clock = Timer(Cell::new(2_000_000));
    let module = if cfg!(feature = "rri") {
        0x2c
    } else if cfg!(feature = "uv") {
        0x2e
    } else {
        0x2d
    };
    let entries = [entry(module)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    clock.0.set(1_000_000);
    let mut out = [0xa5; 128];
    assert_eq!(
        handler.dispatch(req(module, 1, &[]), &mut out),
        Err(SessionError::AuthorizationRevoked)
    );
    assert_eq!(out, [0; 128]);
    handler.connection_closed().unwrap();
    assert_eq!(
        Handler::new(&owner, 1, &entries, &access, &clock).err(),
        Some(Failure::Invalid)
    );
    assert!(Handler::new(&owner, 2, &entries, &access, &clock).is_ok());
}
#[test]
fn failed_detach_stays_closed_and_blocks_replacement_until_actual_cleanup() {
    let mut ports = Ports::new();
    ports.detach_failure = Some(Failure::Backend);
    let owner = WellnessOwner::new(ports);
    let access = Roles::new();
    let clock = Timer(Cell::new(2_000_000));
    let module = if cfg!(feature = "rri") {
        0x2c
    } else if cfg!(feature = "uv") {
        0x2e
    } else {
        0x2d
    };
    let entries = [entry(module)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
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
        .with_backend(|ports| {
            ports.detach_failure = None;
            Ok(())
        })
        .unwrap();
    handler.connection_closed().unwrap();
    assert!(Handler::new(&owner, 2, &entries, &access, &clock).is_ok());
}
#[cfg(feature = "uv")]
#[test]
fn untrusted_success_without_actual_engine_cancel_is_local_failure() {
    let mut ports = Ports::new();
    ports.uv.cancel_outcome = Some(Ok(()));
    let owner = WellnessOwner::new(ports);
    let access = Roles::new();
    let clock = Timer(Cell::new(2_000_000));
    let entries = [entry(0x2e)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    let body = 1u32.to_le_bytes();
    let response = run(&mut handler, 0x2e, 0x30, &body);
    assert_eq!(
        handler.response_admitted(req(0x2e, 0x30, &body), &response),
        Err(SessionError::InvalidHandlerResponse)
    );
    assert!(handler.must_end_session());
    owner
        .with_backend(|ports| {
            assert_eq!(ports.uv.cancelled, None);
            assert!(ports.uv.applied.is_empty());
            Ok(())
        })
        .unwrap();
}
#[cfg(feature = "uv")]
#[test]
fn queued_cancel_revocation_clears_ticket_without_cancel_or_alarm_silence() {
    let owner = WellnessOwner::new(Ports::new());
    let access = Roles::new();
    let clock = Timer(Cell::new(2_000_000));
    let entries = [entry(0x2e)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    let body = 1u32.to_le_bytes();
    let response = run(&mut handler, 0x2e, 0x30, &body);
    access.allow.set(false);
    assert!(!handler.replay_valid(req(0x2e, 0x30, &body), &response));
    assert_eq!(
        handler.response_admitted(req(0x2e, 0x30, &body), &response),
        Err(SessionError::AuthorizationRevoked)
    );
    let mut out = [0xa5; 128];
    assert_eq!(handler.next_event(&mut out), Err(Failure::Closed));
    assert_eq!(out, [0; 128]);
    handler.connection_closed().unwrap();
    owner
        .with_backend(|ports| {
            assert_eq!(ports.uv.cancelled, None);
            assert!(ports.uv.applied.is_empty());
            Ok(())
        })
        .unwrap();
}
#[cfg(feature = "uv")]
#[test]
fn no_sample_is_not_found_but_actual_zero_index_is_a_valid_latest() {
    let mut ports = Ports::new();
    ports.uv.engine = UvEngine::new(
        UvPolicy {
            report_enabled: false,
            alarm_enabled: false,
            threshold_x100: 0,
            interval_s: 0,
        },
        1,
    )
    .unwrap();
    let owner = WellnessOwner::new(ports);
    let access = Roles::new();
    let clock = Timer(Cell::new(2_000_000));
    let entries = [entry(0x2e)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    assert_eq!(run(&mut handler, 0x2e, 2, &[]), [8, 0, 0, 0]);
    owner
        .with_backend(|ports| {
            ports
                .uv
                .engine
                .ingest(UvSample {
                    sample_id: 1,
                    monotonic_ms: 1000,
                    timestamp: Timestamp::unknown(),
                    source: UvSource::DeviceSampling,
                    reading: UvReading {
                        index_x100: Some(0),
                        intensity_uw_cm2: None,
                    },
                })
                .unwrap();
            Ok(())
        })
        .unwrap();
    let out = run(&mut handler, 0x2e, 2, &[]);
    assert_eq!(&out[8..17], &[0; 9]);
    assert_eq!(&out[19..26], &[0, 0, 0, 0, 0, 0, 1]);
}
#[cfg(feature = "uv")]
#[test]
fn malformed_or_short_cancel_never_performs_a_pure_preflight_or_effect() {
    let owner = WellnessOwner::new(Ports::new());
    let access = Roles::new();
    let clock = Timer(Cell::new(2_000_000));
    let entries = [entry(0x2e)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    assert_eq!(run(&mut handler, 0x2e, 0x30, &[1, 0, 0]), [1, 0, 0, 0]);
    assert_eq!(run(&mut handler, 0x2e, 0x30, &[0; 4]), [1, 0, 0, 0]);
    assert_eq!(
        handler.dispatch(req(0x2e, 0x30, &[1, 0, 0, 0]), &mut [0; 3]),
        Err(SessionError::Capacity)
    );
    owner
        .with_backend(|ports| {
            assert_eq!(ports.uv.checks.get(), 0);
            assert!(ports.uv.applied.is_empty());
            Ok(())
        })
        .unwrap();
    assert_eq!(run(&mut handler, 0x2e, 0x30, &[99, 0, 0, 0]), [8, 0, 0, 0]);
}

#[cfg(feature = "uv")]
#[test]
fn pure_event_route_peeks_before_first_queue_reservation_without_preparing_or_consuming() {
    let mut ports = Ports::new();
    ports.uv = UvPort::new(true);
    let owner = WellnessOwner::new(ports);
    let access = Roles::new();
    let clock = Timer(Cell::new(2_000_000));
    let entries = [entry(0x2e)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    handler.subscribe(0x2e).unwrap();
    let expected = Some(EventRoute {
        module: 0x2e,
        opcode: 0x80,
        length: 21,
    });
    assert_eq!(handler.event_route(), Ok(expected));
    assert_eq!(handler.event_route(), Ok(expected));
    owner
        .with_backend(|ports| {
            assert!(ports.uv.engine.pending_report().is_some());
            assert!(ports.uv.engine.pending_alarm().is_some());
            assert!(ports.uv.applied.is_empty());
            Ok(())
        })
        .unwrap();
    let event = handler.next_event(&mut [0; 128]).unwrap().unwrap();
    assert_eq!(handler.event_route(), Ok(expected));
    handler.event_admitted(event.token).unwrap();
    assert_eq!(
        handler.event_route(),
        Ok(Some(EventRoute {
            module: 0x2e,
            opcode: 0x81,
            length: 22
        }))
    );
}

#[cfg(feature = "rri")]
#[test]
fn rri_source_revocation_discards_app_bytes_without_stopping_device_tracker() {
    let mut ports = Ports::new();
    ports
        .tracker
        .accept(
            RriObservation {
                event: RriEvent {
                    measurement: MEASUREMENT,
                    index: 0,
                    cumulative_us: 1_000_000,
                    interval_us: None,
                },
                class: BeatClass::Baseline,
            },
            1_000_000,
        )
        .unwrap();
    let owner = WellnessOwner::new(ports);
    let access = Roles::new();
    let clock = Timer(Cell::new(2_000_000));
    let entries = [entry(0x2c)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    handler.subscribe(0x2c).unwrap();
    let mut out = [0xa5; 128];
    let event = handler.next_event(&mut out).unwrap().unwrap();
    access.source.set(false);
    assert!(!handler.event_valid(event.token));
    assert!(handler.must_end_session());
    assert_eq!(handler.next_event(&mut out), Err(Failure::Closed));
    assert_eq!(out, [0; 128]);
    handler.connection_closed().unwrap();
    owner
        .with_backend(|ports| {
            assert_eq!(ports.tracker.pending().unwrap().measurement, MEASUREMENT);
            assert_eq!(
                ports.tracker.summary_at(3_000_000).unwrap().duration_ms,
                3000
            );
            assert!(ports.acknowledged.is_empty());
            Ok(())
        })
        .unwrap();
}
#[cfg(feature = "uv")]
#[test]
fn real_session_cancel_replay_never_applies_twice_and_previous_boot_success_is_rejected() {
    use device_protocol::ProtocolLimits;
    use device_services::{BuildProfile, Config, DeviceInfo, Session};
    let owner = WellnessOwner::new(Ports::new());
    let access = Roles::new();
    let clock = Timer(Cell::new(2_000_000));
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
        entry(0x2e),
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
    let mut out = [0xa5; 128];
    session
        .dispatch_with_handler(
            req(0, 1, &[1, 2, 128, 0, 0, 0, 3, 0]),
            &mut handler,
            &mut out,
        )
        .unwrap();
    let body = 1u32.to_le_bytes();
    let mut request = req(0x2e, 0x30, &body);
    request.header.message_id = 2;
    assert_eq!(
        session.dispatch_with_handler(request, &mut handler, &mut out),
        Ok(4)
    );
    assert_eq!(
        session.dispatch_with_handler(request, &mut handler, &mut out),
        Ok(4)
    );
    owner
        .with_backend(|ports| {
            assert!(ports.uv.applied.is_empty());
            Ok(())
        })
        .unwrap();
    handler.response_admitted(request, &out[..4]).unwrap();
    assert_eq!(
        session.dispatch_with_handler(request, &mut handler, &mut out),
        Ok(4)
    );
    owner
        .with_backend(|ports| {
            assert_eq!(ports.uv.applied, [1]);
            ports.uv.generation += 1;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        session.dispatch_with_handler(request, &mut handler, &mut out),
        Err(SessionError::AuthorizationRevoked)
    );
    assert_eq!(out, [0; 128]);
    handler.connection_closed().unwrap();
}
#[test]
fn debug_retains_only_lengths_and_never_exposes_clock_or_identity_payloads() {
    let clock = format!(
        "{:?}",
        ClockReading {
            monotonic_us: 123_456_789,
            timestamp: Timestamp::new(987_654_321, true).unwrap()
        }
    );
    assert!(clock.contains("encoded_length"));
    assert!(!clock.contains("123456789"));
    assert!(!clock.contains("987654321"));
    let resource = format!(
        "{:?}",
        Resource::Measurement {
            id: 123_456_789,
            parent: Some(987_654_321)
        }
    );
    assert!(!resource.contains("123456789"));
    assert!(!resource.contains("987654321"));
}

#[cfg(feature = "rri")]
#[test]
fn event_preparation_reads_one_consistent_validated_tracker_view() {
    let mut ports = Ports::new();
    ports
        .tracker
        .accept(
            RriObservation {
                event: RriEvent {
                    measurement: MEASUREMENT,
                    index: 0,
                    cumulative_us: 1_000_000,
                    interval_us: None,
                },
                class: BeatClass::Baseline,
            },
            1_000_000,
        )
        .unwrap();
    let owner = WellnessOwner::new(ports);
    let access = Roles::new();
    let clock = Timer(Cell::new(2_000_000));
    let entries = [entry(0x2c)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    handler.subscribe(0x2c).unwrap();
    owner
        .with_backend(|ports| {
            ports.view_reads.set(0);
            ports.fail_second_view = true;
            Ok(())
        })
        .unwrap();
    assert!(handler.next_event(&mut [0; 128]).unwrap().is_some());
    owner
        .with_backend(|ports| {
            assert_eq!(ports.view_reads.get(), 1);
            Ok(())
        })
        .unwrap();
}

#[cfg(feature = "uv")]
#[test]
fn uv_registration_requires_actual_persistent_nonreuse_policy_and_source_generation_is_insufficient()
 {
    let mut ports = Ports::new();
    ports.uv.identity = UvIdentityPolicy::Unverified;
    ports.uv.generation = 99_999;
    let owner = WellnessOwner::new(ports);
    let access = Roles::new();
    let clock = Timer(Cell::new(2_000_000));
    let entries = [entry(0x2e)];
    assert_eq!(
        Handler::new(&owner, 1, &entries, &access, &clock).err(),
        Some(Failure::Unavailable)
    );
}

#[cfg(feature = "uv")]
#[test]
fn reserved_identity_highwater_survives_simulated_reboot_and_old_numeric_alarm_is_not_found() {
    struct RetainedIds(u32);
    impl RetainedIds {
        fn reserve(&mut self) -> Result<u32, Failure> {
            self.0 = self.0.checked_add(1).ok_or(Failure::Capacity)?;
            Ok(self.0)
        }
    }
    fn boot(ids: &mut RetainedIds, generation: u64) -> UvPort {
        let first = ids.reserve().unwrap();
        let mut port = UvPort::drained_alarm();
        port.engine = UvEngine::new(
            UvPolicy {
                report_enabled: false,
                alarm_enabled: true,
                threshold_x100: 300,
                interval_s: 0,
            },
            first,
        )
        .unwrap();
        port.engine
            .ingest(UvSample {
                sample_id: 1,
                monotonic_ms: 1000,
                timestamp: Timestamp::unknown(),
                source: UvSource::DeviceSampling,
                reading: UvReading {
                    index_x100: Some(500),
                    intensity_uw_cm2: None,
                },
            })
            .unwrap();
        let alarm = port.engine.pending_alarm().unwrap();
        port.active = Some(alarm.event_id);
        port.engine
            .confirm_alarm(alarm.event_id, alarm.transition)
            .unwrap();
        port.generation = generation;
        port.revision = 1;
        port.cancelled = None;
        port
    }
    let mut ledger = RetainedIds(0);
    let first = boot(&mut ledger, 1);
    let old = first.active.unwrap();
    assert_eq!(old, 1);
    let next = boot(&mut ledger, 2);
    assert_eq!(next.active, Some(2));
    assert_eq!(ledger.0, 2);
    let mut ports = Ports::new();
    ports.uv = next;
    let owner = WellnessOwner::new(ports);
    let access = Roles::new();
    let clock = Timer(Cell::new(2_000_000));
    let entries = [entry(0x2e)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    assert_eq!(
        run(&mut handler, 0x2e, 0x30, &old.to_le_bytes()),
        [8, 0, 0, 0]
    );
    assert_eq!(run(&mut handler, 0x2e, 0x30, &2u32.to_le_bytes()), [0; 4]);
    assert_eq!(RetainedIds(u32::MAX).reserve(), Err(Failure::Capacity));
}
#[cfg(feature = "uv")]
#[test]
fn withdrawn_identity_guarantee_invalidates_retained_uv_reply_and_events() {
    let owner = WellnessOwner::new(Ports::new());
    let access = Roles::new();
    let clock = Timer(Cell::new(2_000_000));
    let entries = [entry(0x2e)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    let response = run(&mut handler, 0x2e, 2, &[]);
    owner
        .with_backend(|ports| {
            ports.uv.identity = UvIdentityPolicy::Unverified;
            Ok(())
        })
        .unwrap();
    assert!(!handler.replay_valid(req(0x2e, 2, &[]), &response));
    assert!(handler.must_end_session());
    let mut out = [0xa5; 128];
    assert_eq!(handler.next_event(&mut out), Err(Failure::Closed));
    assert_eq!(out, [0; 128]);
}

#[cfg(feature = "uv")]
#[test]
fn identity_policy_loss_before_first_read_cannot_still_report_uv_ready() {
    let owner = WellnessOwner::new(Ports::new());
    let access = Roles::new();
    let clock = Timer(Cell::new(2_000_000));
    let entries = [entry(0x2e)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    owner
        .with_backend(|ports| {
            ports.uv.identity = UvIdentityPolicy::Unverified;
            Ok(())
        })
        .unwrap();
    let mut out = [0xa5; 128];
    assert_eq!(
        handler.dispatch(req(0x2e, 1, &[]), &mut out),
        Err(SessionError::AuthorizationRevoked)
    );
    assert_eq!(out, [0; 128]);
    assert!(handler.must_end_session());
}

#[cfg(feature = "uv")]
#[test]
fn review_should_encode_actual_cancel_capacity_as_limit_exceeded() {
    let mut ports = Ports::new();
    ports.uv.preview_failure = Some(Failure::Capacity);
    let owner = WellnessOwner::new(ports);
    let access = Roles::new();
    let clock = Timer(Cell::new(2_000_000));
    let entries = [entry(0x2e)];
    let mut handler = Handler::new(&owner, 1, &entries, &access, &clock).unwrap();
    let mut out = [0xa5; 128];
    let result = handler.dispatch(req(0x2e, 0x30, &1u32.to_le_bytes()), &mut out);
    eprintln!(
        "actual pure-preview capacity with128B output: {result:?}, prefix={:?}",
        &out[..4]
    );
    assert_eq!(result, Ok(4));
    assert_eq!(&out[..4], &[7, 0, 0, 0]);
    owner
        .with_backend(|ports| {
            assert!(ports.uv.applied.is_empty());
            assert_eq!(ports.uv.cancelled, None);
            Ok(())
        })
        .unwrap();
}
