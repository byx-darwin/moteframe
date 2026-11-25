//! Actual `AtomicSnapshot` + selected configuration codecs; no service/store doubles.
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "runtime_support/medium.rs"]
mod medium;
use device_configuration::ProductCapabilities;
use device_property_store::{
    CodecSchemas, DefaultValue, ModuleCapabilities, ModuleCatalogue, PropertyStore,
};
use device_services::{PropertyAccess, PropertyBackend, PropertyDescriptor, PropertyOperation};
use device_storage::persistence::{AtomicSnapshot, Layout};
use wattch5_parity_ui_demo::{Appearance, Field, TimeFormat};
use wattch5_product::runtime::{Dictionary, Settings};
const LEVELS: [u16; 5] = [100, 200, 400, 700, 1000];
const LANGUAGES: [&str; 2] = ["zh-CN", "en-US"];
const CAPS: [ModuleCapabilities<'static>; 1] = [ModuleCapabilities {
    module: 4,
    capabilities: ProductCapabilities {
        languages: Some(&LANGUAGES),
        normal_levels: Some(&LEVELS),
        ..ProductCapabilities::NONE
    },
}];
const CATALOGUE: [ModuleCatalogue<'static>; 1] = [ModuleCatalogue {
    module: 4,
    properties: &[
        PropertyDescriptor {
            key: 3,
            access: 11,
            schema: 1,
            value_max: 1,
        },
        PropertyDescriptor {
            key: 7,
            access: 11,
            schema: 1,
            value_max: 512,
        },
        PropertyDescriptor {
            key: 10,
            access: 11,
            schema: 1,
            value_max: 2,
        },
    ],
}];
struct Access(bool);
impl PropertyAccess for Access {
    fn allowed(&self, _: u8, _: PropertyOperation, _: Option<u16>) -> bool {
        self.0
    }
}
type Owner =
    PropertyStore<'static, AtomicSnapshot<medium::Memory, 1024>, CodecSchemas<'static>, 1024>;
fn open(memory: medium::Memory) -> Owner {
    PropertyStore::open(
        AtomicSnapshot::open(
            memory,
            Layout {
                start: 0,
                bank_bytes: 2048,
            },
        )
        .unwrap(),
        &CATALOGUE,
        CodecSchemas::new(&CAPS),
    )
    .unwrap()
}
fn initialized(memory: medium::Memory) -> Owner {
    let owner = open(memory);
    owner
        .initialize(&[
            DefaultValue {
                module: 4,
                key: 3,
                value: &[0],
            },
            DefaultValue {
                module: 4,
                key: 7,
                value: &[5, 0, b'z', b'h', b'-', b'C', b'N'],
            },
            DefaultValue {
                module: 4,
                key: 10,
                value: &[0x90, 1],
            },
        ])
        .unwrap();
    owner
}
#[test]
fn test_should_commit_real_format_codec_and_reload_reconstructed_owner() {
    // A missing durable commit or wrong 04.0003 encoding must fail this flow.
    let memory = medium::Memory::new();
    let owner = initialized(memory.clone());
    let settings = Settings::new(
        &owner,
        Dictionary {
            normal_levels: LEVELS,
            languages: LANGUAGES,
        },
    )
    .unwrap();
    settings.commit(Field::Format, 1, &Access(true)).unwrap();
    assert_eq!(
        settings
            .load(&Access(true), Appearance::default())
            .unwrap()
            .format,
        TimeFormat::Hour12
    );
    drop(owner);
    let owner = open(memory);
    let settings = Settings::new(
        &owner,
        Dictionary {
            normal_levels: LEVELS,
            languages: LANGUAGES,
        },
    )
    .unwrap();
    assert_eq!(
        settings
            .load(&Access(true), Appearance::default())
            .unwrap()
            .format,
        TimeFormat::Hour12
    );
}
#[test]
fn test_should_reject_denied_local_commit_and_keep_confirmed_value() {
    // Bypassing independent UI authority would change the confirmed revision.
    let owner = initialized(medium::Memory::new());
    let settings = Settings::new(
        &owner,
        Dictionary {
            normal_levels: LEVELS,
            languages: LANGUAGES,
        },
    )
    .unwrap();
    assert_eq!(
        settings.commit(Field::Format, 1, &Access(false)),
        Err(device_runtime::PortError::Unavailable)
    );
    assert_eq!(owner.revision(), Ok(1));
    assert_eq!(
        settings
            .load(&Access(true), Appearance::default())
            .unwrap()
            .format,
        TimeFormat::Hour24
    );
}
#[test]
fn test_should_surface_uncertain_commit_until_explicit_owner_recovery() {
    // Advancing confirmed UI state or silently retrying uncertain storage is a bug.
    let memory = medium::Memory::new();
    let owner = initialized(memory.clone());
    let settings = Settings::new(
        &owner,
        Dictionary {
            normal_levels: LEVELS,
            languages: LANGUAGES,
        },
    )
    .unwrap();
    memory.arm_sync(2);
    assert!(settings.commit(Field::Format, 1, &Access(true)).is_err());
    assert!(settings.load(&Access(true), Appearance::default()).is_err());
    memory.disarm();
    owner.recover_and_reload().unwrap();
    assert_eq!(
        settings
            .load(&Access(true), Appearance::default())
            .unwrap()
            .format,
        TimeFormat::Hour12
    );
}

use device_runtime::{PortError, Work};
use device_services::{ClockSourceError, TimeAccess, UtcMapping};
use device_time_store::{ClockReading, TimeStore, TrustedClock};
use std::cell::Cell;
use wattch5_parity_ui_demo::{Event, Model, Screen, Snapshot, Weekday};
use wattch5_product::runtime::{
    Calendar, CalendarDate, NoActions, Observation, ObservationSource, ProductUi, RamFields,
    TouchSource, UiAuthority, UiPorts,
};
#[derive(Debug)]
struct Clock(Cell<u64>);
impl TrustedClock for Clock {
    fn reading(&self) -> Result<ClockReading, ClockSourceError> {
        Ok(ClockReading {
            source_id: 7,
            epoch: 11,
            mono_us: self.0.get(),
        })
    }
}
struct TimeScope;
impl TimeAccess for TimeScope {
    fn allowed(&self, _: bool) -> bool {
        true
    }
}
type TimeOwner = TimeStore<AtomicSnapshot<medium::Memory, 128>, Clock>;
fn time_owner() -> TimeOwner {
    let owner = TimeStore::open(
        AtomicSnapshot::open(
            medium::Memory::new(),
            Layout {
                start: 0,
                bank_bytes: 1024,
            },
        )
        .unwrap(),
        Clock(Cell::new(0)),
        20,
    )
    .unwrap();
    owner.synchronize(1_761_696_000_000, 4, &TimeScope).unwrap();
    owner
}
#[derive(Debug)]
struct Touch(std::collections::VecDeque<Event>);
impl TouchSource for Touch {
    fn next_event(&mut self) -> Result<Option<Event>, PortError> {
        Ok(self.0.pop_front())
    }
}
#[derive(Debug)]
struct Observations {
    unavailable: bool,
    generation: u64,
}
impl ObservationSource for Observations {
    fn capture(&mut self) -> Result<Observation<'_>, PortError> {
        if self.unavailable {
            return Err(PortError::Unavailable);
        }
        // Full fixture, independent literal observations; no demo Snapshot::default.
        Ok(Observation {
            generation: self.generation,
            snapshot: Snapshot {
                hour: 0,
                minute: 0,
                month: 1,
                day: 1,
                weekday: Weekday::Mon,
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
#[derive(Debug)]
struct Dates;
impl Calendar for Dates {
    fn convert(&mut self, m: UtcMapping) -> Result<CalendarDate, PortError> {
        // This test adapter accepts only its actual synchronized UTC fixture.
        if !m.valid || m.utc_ms != 1_761_696_000_000 {
            return Err(PortError::Unavailable);
        }
        Ok(CalendarDate {
            hour: 8,
            minute: 0,
            month: 10,
            day: 29,
            weekday: Weekday::Wed,
        })
    }
}
#[derive(Debug, Default)]
struct Display {
    rows: Vec<device_core::Rect>,
    fail: bool,
    pixels: Vec<device_core::Rgb565>,
}
impl device_core::Display for Display {
    type Error = ();
    fn dimensions(&self) -> device_core::Dimensions {
        device_core::Dimensions::new(128, 128)
    }
    fn write(&mut self, r: device_core::Rect, p: &[device_core::Rgb565]) -> Result<(), ()> {
        assert_eq!(r.height, 1);
        assert_eq!(p.len(), usize::from(r.width));
        if self.fail {
            return Err(());
        }
        if self.pixels.is_empty() {
            self.pixels.resize(128 * 128, device_core::Rgb565(0));
        }
        let start = usize::from(r.y) * 128 + usize::from(r.x);
        self.pixels[start..start + p.len()].copy_from_slice(p);
        self.rows.push(r);
        Ok(())
    }
}
fn ports() -> UiPorts<Touch, Observations, Dates, Display, NoActions> {
    UiPorts {
        touch: Touch(std::collections::VecDeque::new()),
        observations: Observations {
            unavailable: false,
            generation: 1,
        },
        calendar: Dates,
        display: Display::default(),
        actions: NoActions,
    }
}
#[test]
fn test_should_stage_appearance_until_actual_commit_and_keep_failed_display_damage() {
    let memory = medium::Memory::new();
    let owner = initialized(memory.clone());
    let time = time_owner();
    let settings = Settings::new(
        &owner,
        Dictionary {
            normal_levels: LEVELS,
            languages: LANGUAGES,
        },
    )
    .unwrap();
    let mut model = Model::default();
    let mut observed = Observations {
        unavailable: false,
        generation: 1,
    };
    let snapshot = observed.capture().unwrap().snapshot;
    model.preview(Screen::Format, &snapshot).unwrap();
    let access = Access(true);
    let mut ui = ProductUi::new(
        settings,
        &time,
        UiAuthority {
            properties: &access,
            time: &TimeScope,
        },
        model,
        RamFields::RamOnly,
        ports(),
    )
    .unwrap();
    ui.ports_mut().touch.0.push_back(Event::Up);
    ui.step(0).unwrap();
    assert_eq!(ui.model().appearance().format, TimeFormat::Hour24);
    ui.ports_mut().touch.0.push_back(Event::Select);
    memory.arm_sync(2);
    assert_eq!(ui.step(1), Err(PortError::Uncertain));
    assert_eq!(ui.model().appearance().format, TimeFormat::Hour24);
    memory.disarm();
    owner.recover_and_reload().unwrap();
    ui.ports_mut().display.fail = true;
    assert_eq!(ui.step(2), Err(PortError::Fault));
    assert_eq!(ui.pending_row(), Some(1));
    ui.ports_mut().display.fail = false;
    ui.step(3).unwrap();
    assert_eq!(ui.pending_row(), Some(2));
    assert_eq!(ui.model().appearance().format, TimeFormat::Hour12);
}
#[test]
fn test_should_report_missing_observations_and_unbound_measurement_actions() {
    let owner = initialized(medium::Memory::new());
    let time = time_owner();
    let access = Access(true);
    let settings = Settings::new(
        &owner,
        Dictionary {
            normal_levels: LEVELS,
            languages: LANGUAGES,
        },
    )
    .unwrap();
    let mut model = Model::default();
    let mut observations = Observations {
        unavailable: false,
        generation: 1,
    };
    model
        .preview(Screen::Heart, &observations.capture().unwrap().snapshot)
        .unwrap();
    let mut ui = ProductUi::new(
        settings,
        &time,
        UiAuthority {
            properties: &access,
            time: &TimeScope,
        },
        model,
        RamFields::Unavailable,
        ports(),
    )
    .unwrap();
    ui.ports_mut().observations.unavailable = true;
    assert_eq!(ui.step(0), Err(PortError::Unavailable));
    ui.ports_mut().observations.unavailable = false;
    ui.ports_mut().touch.0.push_back(Event::Select);
    assert_eq!(ui.step(1), Err(PortError::Unavailable));
    assert_eq!(ui.model().screen(), Screen::Heart);
}
#[path = "runtime_support/wire.rs"]
mod wire;
use device_protocol::{ModuleEntry, ProtocolLimits};
use device_services::{BuildProfile, Config, DeviceInfo};
use wattch5_product::runtime::{BleAuthority, NoExtensions, ProductFactory};
const MODULES: [ModuleEntry; 4] = [
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
        module: 1,
        major: 1,
        minor: 0,
        availability: 1,
        features: 0,
        input_max: 12,
        output_max: 25,
    },
    ModuleEntry {
        module: 4,
        major: 1,
        minor: 0,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    },
    ModuleEntry {
        module: 0x20,
        major: 1,
        minor: 0,
        availability: 0,
        features: 0,
        input_max: 0,
        output_max: 0,
    },
];
fn config() -> Config<'static> {
    Config {
        session: 1,
        capability_rev: 1,
        profile: BuildProfile::Commercial,
        info: DeviceInfo {
            product_id: 5,
            fw_major: 1,
            fw_minor: 0,
            fw_patch: 0,
            build_id: 19,
        },
        modules: &MODULES,
        limits: ProtocolLimits {
            rx_frame_max: 128,
            tx_frame_max: 128,
            control_max: 128,
            data_max: 0,
            idle_ms: 2000,
            fragment_ms: 100,
            response_ms: 10000,
        },
    }
}
#[test]
fn test_should_share_actual_touch_wire_and_time_owners_through_five_cccd_sessions() {
    // A second settings owner, missing handler wiring, stale actor or false module availability fails here.
    let memory = medium::Memory::new();
    let owner = initialized(memory.clone());
    let time = time_owner();
    let ble = Access(true);
    let local = Access(true);
    let factory = ProductFactory::new(
        &owner,
        &time,
        BleAuthority {
            properties: &ble,
            time: &TimeScope,
        },
        config(),
        &NoExtensions,
    )
    .unwrap();
    let mut model = Model::default();
    let mut observed = Observations {
        unavailable: false,
        generation: 1,
    };
    model
        .preview(Screen::Format, &observed.capture().unwrap().snapshot)
        .unwrap();
    let settings = Settings::new(
        &owner,
        Dictionary {
            normal_levels: LEVELS,
            languages: LANGUAGES,
        },
    )
    .unwrap();
    let mut ui = ProductUi::new(
        settings,
        &time,
        UiAuthority {
            properties: &local,
            time: &TimeScope,
        },
        model,
        RamFields::RamOnly,
        ports(),
    )
    .unwrap();
    let mut runtime = factory.runtime(device_runtime::Budgets::new(2, 2, 1).unwrap());
    let mut transport = wire::Wire::new();
    let mut background = wire::Background::default();
    let mut now = 0;
    exercise_cccd_sessions(
        &mut runtime,
        &mut transport,
        &mut ui,
        &mut background,
        &time,
        &mut now,
    );
    runtime.shutdown(&mut transport).unwrap();
    assert!(background.0 >= 17);
    drop(runtime);
    drop(ui);
    drop(owner);
    let (snapshot, clock) = time.into_parts();
    let reopened_time = TimeStore::open(snapshot, clock, 20).unwrap();
    assert_eq!(reopened_time.revision(), Ok(2));
    assert_eq!(
        reopened_time.read(&TimeScope).unwrap().utc_ms,
        1_761_696_000_000
    );
    let reopened = open(memory);
    let settings = Settings::new(
        &reopened,
        Dictionary {
            normal_levels: LEVELS,
            languages: LANGUAGES,
        },
    )
    .unwrap();
    assert_eq!(
        settings.load(&local, Appearance::default()).unwrap().format,
        TimeFormat::Hour12
    );
}
#[test]
fn test_should_roundtrip_only_actual_locale_and_brightness_dictionaries() {
    // Wrong schema keys/encodings or accepting an undeclared level/locale fails here.
    let owner = initialized(medium::Memory::new());
    let settings = Settings::new(
        &owner,
        Dictionary {
            normal_levels: LEVELS,
            languages: LANGUAGES,
        },
    )
    .unwrap();
    settings.commit(Field::Language, 1, &Access(true)).unwrap();
    settings
        .commit(Field::Brightness, 4, &Access(true))
        .unwrap();
    let confirmed = settings.load(&Access(true), Appearance::default()).unwrap();
    assert_eq!(confirmed.locale, wattch5_parity_ui_demo::Locale::En);
    assert_eq!(confirmed.brightness, 4);
    let adapter = owner.adapter(4).unwrap();
    let mut bytes = [0; 16];
    let n = adapter.read(7, &mut bytes).unwrap().length;
    assert_eq!(&bytes[..n], &[5, 0, b'e', b'n', b'-', b'U', b'S']);
    let n = adapter.read(10, &mut bytes).unwrap().length;
    assert_eq!(&bytes[..n], &[0xe8, 3]);
    assert!(settings.commit(Field::Language, 2, &Access(true)).is_err());
    assert!(
        settings
            .commit(Field::Brightness, 5, &Access(true))
            .is_err()
    );
    let revision = owner.revision().unwrap();
    assert!(
        Settings::new(
            &owner,
            Dictionary {
                normal_levels: [100, 200, 400, 700, 900],
                languages: LANGUAGES
            }
        )
        .is_err()
    );
    assert_eq!(owner.revision(), Ok(revision));
    assert!(
        settings
            .load(&Access(false), Appearance::default())
            .is_err()
    );
}
#[derive(Debug)]
struct ChangingDate(Cell<u8>);
impl Calendar for ChangingDate {
    fn convert(&mut self, m: UtcMapping) -> Result<CalendarDate, PortError> {
        if !m.valid {
            return Err(PortError::Unavailable);
        }
        Ok(CalendarDate {
            hour: 8,
            minute: self.0.get(),
            month: 10,
            day: 29,
            weekday: Weekday::Wed,
        })
    }
}
#[test]
fn test_should_invalidate_on_calendar_change_without_observation_revision_and_reject_blank_time() {
    let owner = initialized(medium::Memory::new());
    let time = time_owner();
    let access = Access(true);
    let settings = Settings::new(
        &owner,
        Dictionary {
            normal_levels: LEVELS,
            languages: LANGUAGES,
        },
    )
    .unwrap();
    let old = ports();
    let ports = UiPorts {
        touch: old.touch,
        observations: old.observations,
        calendar: ChangingDate(Cell::new(0)),
        display: old.display,
        actions: old.actions,
    };
    let mut ui = ProductUi::new(
        settings,
        &time,
        UiAuthority {
            properties: &access,
            time: &TimeScope,
        },
        Model::default(),
        RamFields::RamOnly,
        ports,
    )
    .unwrap();
    ui.step(0).unwrap();
    ui.step(1).unwrap();
    assert_eq!(ui.pending_row(), Some(2));
    ui.ports_mut().calendar.0.set(1);
    ui.step(2).unwrap();
    assert_eq!(ui.pending_row(), Some(3));
    let blank = TimeStore::open(
        AtomicSnapshot::<_, 128>::open(
            medium::Memory::new(),
            Layout {
                start: 0,
                bank_bytes: 1024,
            },
        )
        .unwrap(),
        Clock(Cell::new(0)),
        20,
    )
    .unwrap();
    let settings = Settings::new(
        &owner,
        Dictionary {
            normal_levels: LEVELS,
            languages: LANGUAGES,
        },
    )
    .unwrap();
    let mut ui = ProductUi::new(
        settings,
        &blank,
        UiAuthority {
            properties: &access,
            time: &TimeScope,
        },
        Model::default(),
        RamFields::RamOnly,
        self::ports(),
    )
    .unwrap();
    assert_eq!(ui.step(0), Err(PortError::Unavailable));
    assert!(ui.ports_mut().display.rows.is_empty());
}
#[test]
fn test_should_reject_selected_module_without_actual_extension_owner() {
    let owner = initialized(medium::Memory::new());
    let time = time_owner();
    let ble = Access(true);
    let mut modules = MODULES;
    modules[3].availability = 1;
    modules[3].input_max = 128;
    modules[3].output_max = 128;
    let mut cfg = config();
    cfg.modules = &modules;
    let factory = ProductFactory::new(
        &owner,
        &time,
        BleAuthority {
            properties: &ble,
            time: &TimeScope,
        },
        cfg,
        &NoExtensions,
    )
    .unwrap();
    let mut runtime = factory.runtime(device_runtime::Budgets::new(1, 1, 1).unwrap());
    let mut wire = wire::Wire::new();
    wire.request(0, 1, 1, &[1, 1, 128, 0, 0, 0, 3, 0]);
    let mut ui = wire::Background::default();
    let mut background = wire::Background::default();
    assert!(
        runtime
            .step(0, &mut wire, &mut ui, &mut background)
            .connection
            .is_err()
    );
    assert!(wire.sent.is_empty());
}
#[derive(Debug, Default)]
struct ExtensionState {
    module: Cell<u8>,
    opens: Cell<u32>,
    advances: Cell<u32>,
    ready: Cell<u32>,
    dispatches: Cell<u32>,
    replays: Cell<u32>,
    admitted: Cell<u32>,
    closed: Cell<u32>,
    information: Cell<u32>,
    cleanup_fault: Cell<bool>,
    end: Cell<bool>,
}
struct ExtensionActor<'a>(&'a ExtensionState);
impl wattch5_product::runtime::Extensions for ExtensionState {
    type Handler<'a> = ExtensionActor<'a>;
    fn open(&self, _: device_runtime::Scope) -> Result<Self::Handler<'_>, PortError> {
        self.opens.set(self.opens.get() + 1);
        Ok(ExtensionActor(self))
    }
}
impl device_services::ServiceHandler for ExtensionActor<'_> {
    fn advance(&mut self, _: u64) -> Result<(), device_services::SessionError> {
        self.0.advances.set(self.0.advances.get() + 1);
        Ok(())
    }
    fn informational_access(&self) -> Option<device_services::AccessSnapshot> {
        self.0.information.set(self.0.information.get() + 1);
        None
    }
    fn response_ready(
        &mut self,
        _: device_protocol::Frame<'_>,
    ) -> Result<bool, device_services::SessionError> {
        self.0.ready.set(self.0.ready.get() + 1);
        Ok(true)
    }
    fn replay_valid(&self, _: device_protocol::Frame<'_>, _: &[u8]) -> bool {
        self.0.replays.set(self.0.replays.get() + 1);
        true
    }
    fn response_admitted(
        &mut self,
        _: device_protocol::Frame<'_>,
        _: &[u8],
    ) -> Result<(), device_services::SessionError> {
        self.0.admitted.set(self.0.admitted.get() + 1);
        Ok(())
    }
    fn connection_closed(&mut self) -> Result<(), device_services::SessionError> {
        self.0.closed.set(self.0.closed.get() + 1);
        if self.0.cleanup_fault.get() {
            Err(device_services::SessionError::InvalidHandlerResponse)
        } else {
            Ok(())
        }
    }
    fn must_end_session(&self) -> bool {
        self.0.end.get()
    }
    fn required_features(&self, e: &ModuleEntry, op: u8) -> Option<u32> {
        (e.module == self.0.module.get() && op == if e.module == 4 { 0x10 } else { 0x30 })
            .then_some(0)
    }
    fn authorized(&self, _: device_protocol::Frame<'_>) -> bool {
        !self.0.end.get()
    }
    fn dispatch(
        &mut self,
        _: device_protocol::Frame<'_>,
        out: &mut [u8],
    ) -> Result<usize, device_services::SessionError> {
        self.0.dispatches.set(self.0.dispatches.get() + 1);
        out[..4].fill(0);
        Ok(4)
    }
}
#[test]
fn test_should_preserve_extension_lifecycle_replay_and_failed_cleanup_ownership() {
    // Dropping a wrapper hook or treating failed cleanup as success breaks this contract fixture.
    let owner = initialized(medium::Memory::new());
    let time = time_owner();
    let access = Access(true);
    let extension = ExtensionState::default();
    extension.module.set(2);
    let modules = [
        MODULES[0],
        MODULES[1],
        ModuleEntry {
            module: 2,
            major: 1,
            minor: 0,
            availability: 1,
            features: 0,
            input_max: 128,
            output_max: 128,
        },
        MODULES[2],
    ];
    let mut cfg = config();
    cfg.modules = &modules;
    let factory = ProductFactory::new(
        &owner,
        &time,
        BleAuthority {
            properties: &access,
            time: &TimeScope,
        },
        cfg,
        &extension,
    )
    .unwrap();
    let mut runtime = factory.runtime(device_runtime::Budgets::new(1, 1, 1).unwrap());
    let mut wire = wire::Wire::new();
    let mut ui = wire::Background::default();
    let mut bg = wire::Background::default();
    wire.request(0, 1, 1, &[1, 1, 128, 0, 0, 0, 3, 0]);
    assert!(
        runtime
            .step(0, &mut wire, &mut ui, &mut bg)
            .connection
            .is_ok()
    );
    wire.request(2, 0x30, 2, &[]);
    assert!(
        runtime
            .step(1, &mut wire, &mut ui, &mut bg)
            .connection
            .is_ok()
    );
    wire.request(2, 0x30, 2, &[]);
    assert!(
        runtime
            .step(2, &mut wire, &mut ui, &mut bg)
            .connection
            .is_ok()
    );
    assert_eq!(extension.opens.get(), 1);
    assert_eq!(extension.dispatches.get(), 1);
    assert_eq!(extension.admitted.get(), 1);
    assert!(
        extension.ready.get() > 0 && extension.replays.get() > 0 && extension.advances.get() > 0
    );
    extension.cleanup_fault.set(true);
    assert!(runtime.shutdown(&mut wire).is_err());
    assert_eq!(extension.closed.get(), 1);
    wire.scope = Some(device_runtime::Scope::new(2, 128).unwrap());
    assert!(
        runtime
            .step(3, &mut wire, &mut ui, &mut bg)
            .connection
            .is_err()
    );
    assert_eq!(extension.opens.get(), 1);
    extension.cleanup_fault.set(false);
    runtime.retry_cleanup(&mut wire).unwrap();
    assert_eq!(extension.closed.get(), 2);
    assert!(
        runtime
            .step(4, &mut wire, &mut ui, &mut bg)
            .connection
            .is_ok()
    );
    assert_eq!(extension.opens.get(), 2);
    runtime.shutdown(&mut wire).unwrap();
}
#[test]
fn test_should_reject_duplicate_extension_route_before_hello_and_clean_up_actor() {
    let owner = initialized(medium::Memory::new());
    let time = time_owner();
    let access = Access(true);
    let extension = ExtensionState::default();
    extension.module.set(4);
    let factory = ProductFactory::new(
        &owner,
        &time,
        BleAuthority {
            properties: &access,
            time: &TimeScope,
        },
        config(),
        &extension,
    )
    .unwrap();
    let mut runtime = factory.runtime(device_runtime::Budgets::new(1, 1, 1).unwrap());
    let mut wire = wire::Wire::new();
    let mut ui = wire::Background::default();
    let mut bg = wire::Background::default();
    wire.request(0, 1, 1, &[1, 1, 128, 0, 0, 0, 3, 0]);
    assert!(
        runtime
            .step(0, &mut wire, &mut ui, &mut bg)
            .connection
            .is_err()
    );
    assert!(wire.sent.is_empty());
    assert_eq!(extension.closed.get(), 1);
    assert_eq!(extension.dispatches.get(), 0);
}
struct RevokingAccess(Cell<u32>);
impl PropertyAccess for RevokingAccess {
    fn allowed(&self, _: u8, _: PropertyOperation, _: Option<u16>) -> bool {
        let n = self.0.get();
        self.0.set(n + 1);
        n < 4
    }
}
#[test]
fn test_should_not_report_commit_success_after_local_authority_revocation() {
    let owner = initialized(medium::Memory::new());
    let settings = Settings::new(
        &owner,
        Dictionary {
            normal_levels: LEVELS,
            languages: LANGUAGES,
        },
    )
    .unwrap();
    assert!(
        settings
            .commit(Field::Format, 1, &RevokingAccess(Cell::new(0)))
            .is_err()
    );
    assert_eq!(
        settings
            .load(&Access(true), Appearance::default())
            .unwrap()
            .format,
        TimeFormat::Hour12
    );
}
#[test]
fn test_should_preserve_product_frame_ceiling_when_actual_mtu_is_larger() {
    // Actual CONTROL HELLO/limits/fragments must keep the product's 20-byte ceiling under a 64-byte carrier.
    let owner = initialized(medium::Memory::new());
    let time = time_owner();
    let access = Access(true);
    let mut cfg = config();
    cfg.limits.rx_frame_max = 20;
    cfg.limits.tx_frame_max = 20;
    let factory = ProductFactory::new(
        &owner,
        &time,
        BleAuthority {
            properties: &access,
            time: &TimeScope,
        },
        cfg,
        &NoExtensions,
    )
    .unwrap();
    let mut runtime = factory.runtime(device_runtime::Budgets::new(1, 4, 1).unwrap());
    let mut wire = wire::Wire::new();
    wire.scope = Some(device_runtime::Scope::new(1, 64).unwrap());
    let mut ui = wire::Background::default();
    let mut bg = wire::Background::default();
    wire.request(0, 1, 1, &[1, 1, 128, 0, 0, 0, 3, 0]);
    assert!(
        runtime
            .step(0, &mut wire, &mut ui, &mut bg)
            .connection
            .is_ok()
    );
    assert!(wire.sent.iter().all(|frame| frame.len() <= 20));
    let mut peer = device_protocol::Reassembler::<128>::new(20, 128).unwrap();
    for frame in &wire.sent {
        peer.push(device_protocol::Frame::decode(frame, 20, 128).unwrap(), 0)
            .unwrap();
    }
    assert_eq!(
        device_protocol::HelloResponse::decode(&peer.message().unwrap().payload[4..])
            .unwrap()
            .session,
        1
    );
    wire.sent.clear();
    wire.request(0, 4, 2, &[]);
    assert!(
        runtime
            .step(1, &mut wire, &mut ui, &mut bg)
            .connection
            .is_ok()
    );
    peer.reset();
    for frame in &wire.sent {
        peer.push(device_protocol::Frame::decode(frame, 20, 128).unwrap(), 1)
            .unwrap();
    }
    let offered =
        device_protocol::ProtocolLimits::decode(&peer.message().unwrap().payload[4..]).unwrap();
    assert_eq!((offered.rx_frame_max, offered.tx_frame_max), (20, 20));
    runtime.shutdown(&mut wire).unwrap();
}

#[test]
fn test_should_match_existing_full_scene_after_clipped_row_failure_and_retry() {
    // Incorrect clipping/origin/color conversion or advancing a failed row changes the final image.
    let owner = initialized(medium::Memory::new());
    let time = time_owner();
    let access = Access(true);
    let settings = Settings::new(
        &owner,
        Dictionary {
            normal_levels: LEVELS,
            languages: LANGUAGES,
        },
    )
    .unwrap();
    let mut ui = ProductUi::new(
        settings,
        &time,
        UiAuthority {
            properties: &access,
            time: &TimeScope,
        },
        Model::default(),
        RamFields::RamOnly,
        ports(),
    )
    .unwrap();
    for n in 0..7 {
        ui.step(n).unwrap();
    }
    ui.ports_mut().display.fail = true;
    assert_eq!(ui.step(7), Err(PortError::Fault));
    assert_eq!(ui.pending_row(), Some(7));
    ui.ports_mut().display.fail = false;
    for n in 8..129 {
        ui.step(n).unwrap();
    }
    assert_eq!(ui.pending_row(), None);
    assert_eq!(ui.ports_mut().display.rows.len(), 128);
    assert!(
        ui.ports_mut()
            .display
            .rows
            .iter()
            .all(|r| r.x == 0 && r.width == 128 && r.height == 1)
    );
    let mut observations = Observations {
        unavailable: false,
        generation: 1,
    };
    let mut snapshot = observations.capture().unwrap().snapshot;
    snapshot.hour = 8;
    snapshot.minute = 0;
    snapshot.month = 10;
    snapshot.day = 29;
    snapshot.weekday = Weekday::Wed;
    let mut expected = Display::default();
    ui.model()
        .scene(&snapshot)
        .unwrap()
        .draw(&mut device_ui::StreamTarget::new(&mut expected))
        .unwrap();
    assert_eq!(ui.ports_mut().display.pixels, expected.pixels);
}

type TestUi<'a> = ProductUi<
    'a,
    'static,
    AtomicSnapshot<medium::Memory, 1024>,
    AtomicSnapshot<medium::Memory, 128>,
    Clock,
    Touch,
    Observations,
    Dates,
    Display,
    NoActions,
    1024,
>;
fn exercise_cccd_sessions<F: device_runtime::SessionFactory>(
    runtime: &mut device_runtime::Runtime<'_, F>,
    transport: &mut wire::Wire,
    ui: &mut TestUi<'_>,
    background: &mut wire::Background,
    time: &TimeOwner,
    now: &mut u64,
) {
    let hello = [1, 1, 128, 0, 0, 0, 3, 0];
    for cycle in 1..=5 {
        transport.scope = Some(device_runtime::Scope::new(cycle, 128).unwrap());
        transport.request(0, 1, 1, &hello);
        let report = runtime.step(*now, transport, ui, background);
        assert!(report.connection.is_ok());
        assert!(report.ui.is_ok());
        *now += 1;
        assert_eq!(&transport.response().payload[..4], &[0, 0, 0, 0]);
        assert_eq!(
            u32::from_le_bytes(transport.response().payload[6..10].try_into().unwrap()),
            u32::try_from(cycle).unwrap()
        );
        if cycle == 1 {
            ui.ports_mut().touch.0.push_back(Event::Up);
            runtime.step(*now, transport, ui, background);
            *now += 1;
            ui.ports_mut().touch.0.push_back(Event::Select);
            assert!(runtime.step(*now, transport, ui, background).ui.is_ok());
            *now += 1;
        }
        transport.request(4, 0x10, 2, &[3, 0]);
        assert!(
            runtime
                .step(*now, transport, ui, background)
                .connection
                .is_ok()
        );
        *now += 1;
        assert_eq!(transport.response().payload.last(), Some(&1));
        if cycle == 1 {
            exercise_wire_updates(runtime, transport, ui, background, time, now);
        }
        transport.scope = None;
        assert!(
            runtime
                .step(*now, transport, ui, background)
                .connection
                .is_ok()
        );
        *now += 1;
    }
}
fn exercise_wire_updates<F: device_runtime::SessionFactory>(
    runtime: &mut device_runtime::Runtime<'_, F>,
    transport: &mut wire::Wire,
    ui: &mut TestUi<'_>,
    background: &mut wire::Background,
    time: &TimeOwner,
    now: &mut u64,
) {
    transport.request(4, 0x11, 3, &[3, 0, 0, 0, 0, 0, 1, 0, 0]);
    runtime.step(*now, transport, ui, background);
    *now += 1;
    assert_eq!(ui.model().appearance().format, TimeFormat::Hour24);
    transport.request(4, 0x11, 4, &[3, 0, 0, 0, 0, 0, 1, 0, 1]);
    runtime.step(*now, transport, ui, background);
    *now += 1;
    transport.request(1, 1, 5, &[]);
    runtime.step(*now, transport, ui, background);
    *now += 1;
    assert_eq!(transport.response().payload.len(), 25);
    assert_eq!(transport.response().payload[24], 1);
    let mut sync = [0; 12];
    sync[..8].copy_from_slice(&1_761_696_000_000_i64.to_le_bytes());
    sync[8..].copy_from_slice(&7_u32.to_le_bytes());
    transport.request(1, 2, 6, &sync);
    assert!(
        runtime
            .step(*now, transport, ui, background)
            .connection
            .is_ok()
    );
    *now += 1;
    assert_eq!(transport.response().payload.len(), 25);
    assert_eq!(time.evidence().unwrap().unwrap().uncertainty_ms, 7);
    transport.request(0x20, 1, 7, &[]);
    runtime.step(*now, transport, ui, background);
    *now += 1;
    assert_eq!(
        &transport.response().payload[..2],
        &(device_services::Status::InvalidState as u16).to_le_bytes()
    );
}

#[derive(Debug)]
struct LiveObservations {
    base: Observations,
    battery: u8,
}
impl ObservationSource for LiveObservations {
    fn capture(&mut self) -> Result<Observation<'_>, PortError> {
        let mut observed = self.base.capture()?;
        observed.snapshot.battery = self.battery;
        Ok(observed)
    }
}
#[test]
fn test_should_finish_rows_during_live_updates_then_converge_to_latest_scene() {
    // Restarting every change starves the bottom; missing the followup leaves old pixels after updates stop.
    let owner = initialized(medium::Memory::new());
    let time = time_owner();
    let access = Access(true);
    let settings = Settings::new(
        &owner,
        Dictionary {
            normal_levels: LEVELS,
            languages: LANGUAGES,
        },
    )
    .unwrap();
    let p = ports();
    let mut ui = ProductUi::new(
        settings,
        &time,
        UiAuthority {
            properties: &access,
            time: &TimeScope,
        },
        Model::default(),
        RamFields::RamOnly,
        UiPorts {
            touch: p.touch,
            observations: LiveObservations {
                base: p.observations,
                battery: 37,
            },
            calendar: p.calendar,
            display: p.display,
            actions: p.actions,
        },
    )
    .unwrap();
    for n in 0..512 {
        ui.ports_mut().observations.base.generation = n + 1;
        ui.ports_mut().observations.battery = u8::try_from(n % 101).unwrap();
        if n == 5 {
            ui.ports_mut().display.fail = true;
            assert_eq!(ui.step(n), Err(PortError::Fault));
            assert_eq!(ui.pending_row(), Some(5));
            ui.ports_mut().display.fail = false;
        }
        ui.step(n).unwrap();
    }
    assert!(ui.ports_mut().display.rows.iter().any(|r| r.y == 127));
    for n in 512..768 {
        if ui.pending_row().is_none() {
            break;
        }
        ui.step(n).unwrap();
    }
    assert_eq!(ui.pending_row(), None);
    let mut observations = LiveObservations {
        base: Observations {
            unavailable: false,
            generation: 512,
        },
        battery: 6,
    };
    let mut snapshot = observations.capture().unwrap().snapshot;
    snapshot.hour = 8;
    snapshot.minute = 0;
    snapshot.month = 10;
    snapshot.day = 29;
    snapshot.weekday = Weekday::Wed;
    let mut expected = Display::default();
    ui.model()
        .scene(&snapshot)
        .unwrap()
        .draw(&mut device_ui::StreamTarget::new(&mut expected))
        .unwrap();
    assert_eq!(ui.ports_mut().display.pixels, expected.pixels);
}
