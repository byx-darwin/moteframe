//! Actual source-sharing integration, offline NOR only; no hardware or demo production inputs.
#![cfg(feature = "native-health")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "../../../wattch5/crates/device-history-store/tests/async_paged_support/mod.rs"]
mod medium;
use core::{cell::Cell, task::Poll};
use device_core::{Dimensions, Display, Rect, Rgb565};
use device_health::{JobManager, Timestamp};
use device_health_service::{HealthHandler, ImportHealthHandler, LatestStore, SharedHealthOwner};
use device_history_store::{
    DomainConfig, HistoryClock,
    async_paged::{JobResult, JobToken},
    paged::LogConfig,
};
use device_protocol::{Frame, Header, Kind};
use device_runtime::{PortError, Work};
use device_services::ServiceHandler;
use device_storage::{incremental::IncrementalSnapshot, persistence::Layout};
use medium::{Clock, Control};
use wattch5_bringup::{Event, InputReport, InputSource, Readiness, ResetGate, TouchStatus};
use wattch5_candidate::ui::{CandidateUi, LocalHealth, UiPage};
use wattch5_parity_ui_demo::{MeasureKind, MeasureState, Reading};
use wattch5_product::runtime::legacy_health::{
    HealthHistory, HealthLog, HealthSchema, LegacyBackend, LegacyInputSource, LegacyInputs,
    LegacyProduct,
};
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
fn finish(h: &History, token: JobToken<'_>) -> JobResult {
    for _ in 0..100_000 {
        h.step().unwrap();
        if let Poll::Ready(result) = h.result(token).unwrap() {
            return result.unwrap();
        }
    }
    panic!("bounded NOR fixture stalled");
}
fn initialize(h: &History) {
    assert_eq!(
        finish(h, h.begin_recover().unwrap()),
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
                    },
                ]
            )
            .unwrap()
        ),
        JobResult::Initialized
    );
}
struct Inputs(bool);
impl LegacyInputSource for Inputs {
    fn inputs(&self, _: u8) -> Result<LegacyInputs, device_health::Error> {
        if !self.0 {
            return Err(device_health::Error::Unavailable);
        }
        // Explicit offline lifecycle fixture only; native production inputs remain unavailable.
        Ok(LegacyInputs {
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
struct Allowed;
impl device_health_service::HealthAccess for Allowed {
    fn allowed(&self, _: u8, _: device_health_service::Operation, _: Option<u32>) -> bool {
        true
    }
}
struct ServiceClock;
impl device_health_service::HealthClock for ServiceClock {
    fn now_ms(&self) -> Result<u64, device_health_service::Error> {
        Ok(100)
    }
}
struct Panel;
impl Display for Panel {
    type Error = ();
    fn dimensions(&self) -> Dimensions {
        Dimensions::new(128, 128)
    }
    fn write(&mut self, rect: Rect, pixels: &[Rgb565]) -> Result<(), ()> {
        assert_eq!(rect.height, 1);
        assert_eq!(pixels.len(), 128);
        Ok(())
    }
}
struct Input<'a>(&'a Cell<Option<Event>>);
impl InputSource for Input<'_> {
    fn sample(&mut self, _: u64) -> InputReport {
        InputReport {
            event: self.0.take(),
            touch: TouchStatus::Ready,
            fault: None,
        }
    }
}
fn frame(module: u8, opcode: u8, message_id: u16, payload: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module,
            opcode,
            message_id,
            offset: 0,
            total_length: u16::try_from(payload.len()).unwrap(),
        },
        payload,
    }
}

#[test]
fn actual_c33_commits_update_native_pages_and_latest_from_one_log_with_no_local_input_defaults() {
    let meta = Control::new(0x2000, 256, 4096);
    let nor = Control::new(0x62000, 256, 4096);
    let clock = Clock::new();
    let history = owner(&meta, &nor, clock.clone());
    initialize(&history);
    let log = HealthLog::restore(&history, || clock.now_ms(), || {}).unwrap();
    let inputs = Inputs(false);
    let native = LocalHealth::new(&inputs, &log);
    let backend = SharedHealthOwner::new(
        JobManager::<3>::new(1).unwrap(),
        LegacyBackend::new(&inputs),
    );
    let product = LegacyProduct::new(&inputs, &log);
    let mut latest = log.latest();
    let entries = [0x25, 0x26, 0x27].map(|module| device_protocol::ModuleEntry {
        module,
        major: 1,
        minor: 0,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    });
    let inner = HealthHandler::new(
        &backend,
        1,
        &entries,
        &Allowed,
        &ServiceClock,
        &product,
        &mut latest,
        None,
    )
    .unwrap();
    let mut service = ImportHealthHandler::new(inner, 1, &log, &Allowed).unwrap();
    let input = Cell::new(None);
    let readiness = Cell::new(Readiness::new());
    let gate = ResetGate::new();
    gate.completed();
    gate.arm_ui().unwrap();
    let mut ui = CandidateUi::new(Panel, Input(&input), |_| Ok(()), &readiness, &gate).unwrap();
    ui.bind_health(&native).unwrap();
    for (index, (module, page, kind, body, reading)) in [
        (
            0x25,
            UiPage::Pressure,
            MeasureKind::Pressure,
            vec![118, 0, 78, 0, 60, 0, 7, 0],
            Reading::Pressure {
                upper: 118,
                lower: 78,
            },
        ),
        (
            0x26,
            UiPage::Oxygen,
            MeasureKind::Oxygen,
            vec![0x48, 0x26, 0, 0, 1, 0],
            Reading::Number(98),
        ),
        (
            0x27,
            UiPage::Glucose,
            MeasureKind::Glucose,
            vec![0x58, 2, 1],
            Reading::Tenth(60),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        input.set(Some(Event::Left));
        ui.step(0).unwrap();
        assert_eq!(ui.view().page, page);
        input.set(Some(Event::Tap { x: 20, y: 96 }));
        assert_eq!(ui.step(1), Err(PortError::Unavailable));
        assert_eq!(ui.view().measurement.reading, Reading::Missing);
        let mut wire = vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        wire.extend(&body);
        let request = frame(module, 0x33, u16::try_from(index + 1).unwrap(), &wire);
        let calls = meta.calls.get() + nor.calls.get();
        assert!(!service.response_ready(request).unwrap());
        ui.step(2).unwrap();
        assert_eq!(
            calls,
            meta.calls.get() + nor.calls.get(),
            "UI and admission must not advance Flash"
        );
        assert_eq!(
            ui.view().measurement.reading,
            Reading::Missing,
            "pending import must not be displayed as committed"
        );
        for n in 3..10000 {
            log.step().unwrap();
            ui.step(n).unwrap();
            if service.response_ready(request).unwrap() {
                break;
            }
        }
        let mut response = [0; 128];
        let n = service.dispatch(request, &mut response).unwrap();
        assert_eq!(n, 12);
        assert_eq!(&response[..4], &[0; 4]);
        service.response_admitted(request, &response[..n]).unwrap();
        for n in 10000..10390 {
            ui.step(n).unwrap();
        }
        assert_eq!(ui.view().measurement, log.page(kind).unwrap());
        assert_eq!(ui.view().measurement.reading, reading);
        assert_eq!(ui.view().measurement.state, MeasureState::Result);
        assert!(!ui.view().health_error);
        let mut bytes = [0; 9];
        let saved = log
            .latest()
            .load(module, u16::from(module) * 256 + 1, &mut bytes)
            .unwrap();
        assert_eq!(&bytes[..saved.info.length - 1], &body);
        assert_eq!(bytes[saved.info.length - 1], 2);
        input.set(Some(Event::Tap { x: 20, y: 96 }));
        assert_eq!(ui.step(10400), Err(PortError::Unavailable));
        assert_eq!(ui.view().measurement.reading, reading);
        assert!(ui.view().health_error);
    }
    // A valid full-width wire BP beyond the three-digit native page becomes an
    // explicit page error, while its unchanged committed wire value remains available.
    input.set(Some(Event::Right));
    ui.step(10500).unwrap();
    input.set(Some(Event::Right));
    ui.step(10501).unwrap();
    assert_eq!(ui.view().page, UiPage::Pressure);
    let mut oversized = vec![1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
    oversized.extend([0xe8, 3, 78, 0, 60, 0, 7, 0]);
    let request = frame(0x25, 0x33, 10, &oversized);
    assert!(!service.response_ready(request).unwrap());
    for n in 10502..20502 {
        log.step().unwrap();
        ui.step(n).unwrap();
        if service.response_ready(request).unwrap() {
            break;
        }
    }
    assert_eq!(ui.view().measurement.state, MeasureState::Error);
    assert_eq!(ui.view().measurement.reading, Reading::Missing);
    let mut bytes = [0; 9];
    let saved = log.latest().load(0x25, 0x2501, &mut bytes).unwrap();
    assert_eq!(&bytes[..2], &[0xe8, 3]);
    assert_eq!(bytes[saved.info.length - 1], 2);
    // The real shared log restores the exact imported page snapshots after power-cycle recovery.
    drop(history);
    let recovered = owner(&meta, &nor, clock.clone());
    assert_eq!(
        finish(&recovered, recovered.begin_recover().unwrap()),
        JobResult::Recovered { initialized: true }
    );
    let restored = HealthLog::restore(&recovered, || clock.now_ms(), || {}).unwrap();
    assert_eq!(
        restored.page(MeasureKind::Oxygen).unwrap().reading,
        Reading::Number(98)
    );
    assert_eq!(
        restored.page(MeasureKind::Glucose).unwrap().reading,
        Reading::Tenth(60)
    );
}

#[test]
fn actual_native_back_stops_page_delivery_but_durable_owner_finishes_accepted_result() {
    let meta = Control::new(0x2000, 256, 4096);
    let nor = Control::new(0x62000, 256, 4096);
    let clock = Clock::new();
    let history = owner(&meta, &nor, clock.clone());
    initialize(&history);
    let log = HealthLog::restore(&history, || clock.now_ms(), || {}).unwrap();
    let inputs = Inputs(true);
    let native = LocalHealth::new(&inputs, &log);
    let input = Cell::new(Some(Event::Left));
    let readiness = Cell::new(Readiness::new());
    let gate = ResetGate::new();
    gate.completed();
    gate.arm_ui().unwrap();
    let mut ui = CandidateUi::new(Panel, Input(&input), |_| Ok(()), &readiness, &gate).unwrap();
    ui.bind_health(&native).unwrap();
    ui.step(0).unwrap();
    input.set(Some(Event::Tap { x: 20, y: 96 }));
    ui.step(1).unwrap();
    assert_eq!(ui.view().measurement.state, MeasureState::Pending);
    input.set(Some(Event::Tap { x: 20, y: 112 }));
    ui.step(2).unwrap();
    assert_eq!(ui.view().page, UiPage::Settings);
    let mut output = [0; 9];
    assert!(log.latest().load(0x25, 0x2501, &mut output).is_err());
    for _ in 0..10000 {
        log.step().unwrap();
        if log.latest().load(0x25, 0x2501, &mut output).is_ok() {
            break;
        }
    }
    let committed = log.latest().load(0x25, 0x2501, &mut output).unwrap();
    assert_eq!(output[committed.info.length - 1], 1);
    assert_eq!(committed.generation, 1);
    assert_eq!(
        log.page(MeasureKind::Pressure).unwrap().state,
        MeasureState::Idle
    );
    // Recovered history retains the accepted result even though page delivery was stopped.
    drop(history);
    let recovered = owner(&meta, &nor, clock.clone());
    finish(&recovered, recovered.begin_recover().unwrap());
    let restored = HealthLog::restore(&recovered, || clock.now_ms(), || {}).unwrap();
    assert_eq!(
        restored.page(MeasureKind::Pressure).unwrap().reading,
        Reading::Pressure {
            upper: 117,
            lower: 76
        }
    );
}
