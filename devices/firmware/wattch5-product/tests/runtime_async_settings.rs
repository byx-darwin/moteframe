//! Real async settings integration with offline physical-input fixtures.
#![cfg(feature = "runtime")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod async_support;
#[path = "runtime_support/wire.rs"]
mod wire;
use async_support::{Access, Nor, config, dictionary, initialized};
use device_runtime::{Budgets, PortError, Scope, Work};
use device_services::PropertyBackend;
use std::cell::Cell;
use wattch5_parity_ui_demo::{Event, Field, Model, Screen, TimeFormat};
use wattch5_product::runtime::{
    AsyncProductUi, AsyncSettings, RamFields, SettingsFactory, SettingsWork, UiAuthority,
};

#[test]
fn test_should_defer_actual_runtime_set_until_background_and_replay_once() {
    // Publishing a receipt before the owner's final readback breaks this test.
    let nor = Nor::new();
    let blocked = nor.blocked.clone();
    let actions = nor.actions.clone();
    let owner = initialized(nor);
    let access = Access(Cell::new(true));
    let factory = SettingsFactory::new(&owner, &access, config()).unwrap();
    let mut runtime = factory.runtime(Budgets::new(1, 1, 1).unwrap());
    let mut transport = wire::Wire::new();
    let mut ui = wire::Background::default();
    let mut background = SettingsWork::new(&owner);
    transport.request(0, 1, 1, &[1, 1, 128, 0, 0, 0, 3, 0]);
    runtime
        .step(0, &mut transport, &mut ui, &mut background)
        .connection
        .unwrap();
    transport.sent.clear();
    blocked.set(true);
    transport.request(4, 0x11, 2, &[3, 0, 0, 0, 0, 0, 1, 0, 1]);
    for now in 1..20 {
        let before = actions.get();
        let r = runtime.step(now, &mut transport, &mut ui, &mut background);
        r.connection.unwrap();
        r.background.unwrap();
        assert!(actions.get() - before <= 1);
        assert!(transport.sent.is_empty());
        assert_eq!(owner.revision(), Ok(1));
    }
    blocked.set(false);
    for now in 20..2000 {
        let r = runtime.step(now, &mut transport, &mut ui, &mut background);
        r.connection.unwrap();
        r.background.unwrap();
        if !transport.sent.is_empty() {
            break;
        }
    }
    assert_eq!(transport.response().payload, &[0, 0, 0, 0, 2, 0, 0, 0]);
    assert_eq!(owner.revision(), Ok(2));
    transport.sent.clear();
    transport.request(4, 0x11, 2, &[3, 0, 0, 0, 0, 0, 1, 0, 1]);
    runtime
        .step(2000, &mut transport, &mut ui, &mut background)
        .connection
        .unwrap();
    assert_eq!(transport.response().payload, &[0, 0, 0, 0, 2, 0, 0, 0]);
    assert_eq!(owner.revision(), Ok(2));
    runtime.shutdown(&mut transport).unwrap();
}

#[test]
fn test_should_continue_detached_runtime_write_and_open_fresh_settings_only_session() {
    let nor = Nor::new();
    let blocked = nor.blocked.clone();
    let memory = nor.memory.clone();
    let owner = initialized(nor);
    let access = Access(Cell::new(true));
    let factory = SettingsFactory::new(&owner, &access, config()).unwrap();
    let mut runtime = factory.runtime(Budgets::new(1, 1, 1).unwrap());
    let mut transport = wire::Wire::new();
    let mut ui = wire::Background::default();
    let mut bg = SettingsWork::new(&owner);
    transport.request(0, 1, 1, &[1, 1, 128, 0, 0, 0, 3, 0]);
    runtime
        .step(0, &mut transport, &mut ui, &mut bg)
        .connection
        .unwrap();
    transport.sent.clear();
    blocked.set(true);
    transport.request(4, 0x11, 2, &[3, 0, 0, 0, 0, 0, 1, 0, 1]);
    runtime
        .step(1, &mut transport, &mut ui, &mut bg)
        .connection
        .unwrap();
    transport.scope = None;
    runtime
        .step(2, &mut transport, &mut ui, &mut bg)
        .connection
        .unwrap();
    blocked.set(false);
    for now in 3..1500 {
        runtime
            .step(now, &mut transport, &mut ui, &mut bg)
            .background
            .unwrap();
        if owner.revision() == Ok(2) {
            break;
        }
    }
    assert_eq!(owner.revision(), Ok(2));
    assert!(transport.sent.is_empty());
    transport.scope = Some(Scope::new(2, 128).unwrap());
    transport.request(0, 1, 1, &[1, 1, 128, 0, 0, 0, 3, 0]);
    runtime
        .step(1500, &mut transport, &mut ui, &mut bg)
        .connection
        .unwrap();
    assert_eq!(
        device_protocol::HelloResponse::decode(&transport.response().payload[4..])
            .unwrap()
            .session,
        2
    );
    transport.request(4, 0x10, 2, &[3, 0]);
    runtime
        .step(1501, &mut transport, &mut ui, &mut bg)
        .connection
        .unwrap();
    assert_eq!(transport.response().payload.last(), Some(&1));
    runtime.shutdown(&mut transport).unwrap();
    drop(runtime);
    drop(owner);
    let mut restored = Nor::new();
    restored.memory = memory;
    let owner = async_support::open(restored);
    let mut value = [0; 1];
    assert_eq!(
        owner
            .adapter(4)
            .unwrap()
            .read(3, &mut value)
            .unwrap()
            .revision,
        2
    );
    assert_eq!(value, [1]);
}

#[test]
fn test_should_reject_unbound_time_module_and_inconsistent_settings_capacity() {
    let owner = initialized(Nor::new());
    let access = Access(Cell::new(true));
    let mut modules = async_support::MODULES;
    modules[1].module = 1;
    let mut c = config();
    c.modules = &modules;
    assert!(matches!(
        SettingsFactory::new(&owner, &access, c),
        Err(PortError::InvalidInput)
    ));
    let mut c = config();
    c.limits.control_max = 512;
    assert!(matches!(
        SettingsFactory::new(&owner, &access, c),
        Err(PortError::InvalidInput)
    ));
}

fn editor() -> Model {
    use wattch5_product::runtime::ObservationSource;
    let mut model = Model::default();
    let mut observations = async_support::ui::ports().observations;
    model
        .preview(Screen::Format, &observations.capture().unwrap().snapshot)
        .unwrap();
    model
}
#[test]
fn test_should_keep_touch_editor_and_confirmed_appearance_until_exact_durability() {
    // Publishing the event's candidate before final readback exits Format too early.
    let nor = Nor::new();
    let blocked = nor.blocked.clone();
    let owner = initialized(nor);
    let time = async_support::time::time_owner();
    let access = Access(Cell::new(true));
    let mut ui = AsyncProductUi::new(
        AsyncSettings::new(&owner, dictionary()).unwrap(),
        &time,
        UiAuthority {
            properties: &access,
            time: &async_support::time::TimeScope,
        },
        editor(),
        RamFields::Unavailable,
        async_support::ui::ports(),
    )
    .unwrap();
    ui.ports_mut().touch.0.push_back(Event::Up);
    ui.step(0).unwrap();
    ui.ports_mut().touch.0.push_back(Event::Select);
    blocked.set(true);
    ui.step(1).unwrap();
    assert!(ui.save_pending());
    assert_eq!(ui.model().screen(), Screen::Format);
    assert_eq!(ui.model().appearance().format, TimeFormat::Hour24);
    assert!(
        AsyncSettings::new(&owner, dictionary())
            .unwrap()
            .begin(Field::Format, 0, &access)
            .is_err()
    );
    for now in 2..20 {
        ui.ports_mut().touch.0.push_back(Event::Right);
        ui.step(now).unwrap();
        owner.step().unwrap();
        assert!(ui.ports_mut().touch.0.is_empty());
        assert_eq!(ui.model().screen(), Screen::Format);
        assert_eq!(owner.revision(), Ok(1));
    }
    blocked.set(false);
    for now in 20..2000 {
        owner.step().unwrap();
        ui.step(now).unwrap();
        if !ui.save_pending() {
            break;
        }
    }
    assert!(!ui.save_pending());
    assert_eq!(ui.model().appearance().format, TimeFormat::Hour12);
    assert_ne!(ui.model().screen(), Screen::Format);
    assert_eq!(owner.revision(), Ok(2));
    ui.step(2001).unwrap();
    assert_eq!(owner.revision(), Ok(2));
}
#[test]
fn test_should_detach_revoked_touch_save_even_when_observations_unavailable() {
    // Checking observations before pending authority can strand the accepted token.
    let nor = Nor::new();
    let owner = initialized(nor);
    let time = async_support::time::time_owner();
    let access = Access(Cell::new(true));
    let mut ui = AsyncProductUi::new(
        AsyncSettings::new(&owner, dictionary()).unwrap(),
        &time,
        UiAuthority {
            properties: &access,
            time: &async_support::time::TimeScope,
        },
        editor(),
        RamFields::Unavailable,
        async_support::ui::ports(),
    )
    .unwrap();
    ui.ports_mut().touch.0.push_back(Event::Up);
    ui.step(0).unwrap();
    ui.ports_mut().touch.0.push_back(Event::Select);
    ui.step(1).unwrap();
    access.0.set(false);
    ui.ports_mut().observations.unavailable = true;
    assert_eq!(ui.step(2), Err(PortError::Unavailable));
    assert!(!ui.save_pending());
    assert_eq!(ui.model().screen(), Screen::Format);
    for _ in 0..2000 {
        owner.step().unwrap();
        if owner.revision() == Ok(2) {
            break;
        }
    }
    assert_eq!(owner.revision(), Ok(2));
    access.0.set(true);
    assert!(owner.begin_set(4, 3, 0, &[0], &access).is_ok());
}
#[test]
fn test_should_keep_editor_on_async_final_sync_failure() {
    let nor = Nor::new();
    let memory = nor.memory.clone();
    let owner = initialized(nor);
    let time = async_support::time::time_owner();
    let access = Access(Cell::new(true));
    let mut ui = AsyncProductUi::new(
        AsyncSettings::new(&owner, dictionary()).unwrap(),
        &time,
        UiAuthority {
            properties: &access,
            time: &async_support::time::TimeScope,
        },
        editor(),
        RamFields::Unavailable,
        async_support::ui::ports(),
    )
    .unwrap();
    ui.ports_mut().touch.0.push_back(Event::Up);
    ui.step(0).unwrap();
    ui.ports_mut().touch.0.push_back(Event::Select);
    ui.step(1).unwrap();
    memory.arm_sync(2);
    let mut failed = false;
    for now in 2..2000 {
        let _ = owner.step();
        if ui.step(now).is_err() {
            failed = true;
            break;
        }
    }
    assert!(failed);
    assert!(!ui.save_pending());
    assert_eq!(ui.model().screen(), Screen::Format);
    assert_eq!(ui.model().appearance().format, TimeFormat::Hour24);
    assert!(owner.revision().is_err());
}

#[test]
fn test_should_serve_confirmed_get_and_busy_ble_set_during_local_async_save() {
    // An independent candidate image in either route would leak or overwrite local work.
    let nor = Nor::new();
    let blocked = nor.blocked.clone();
    let owner = initialized(nor);
    let access = Access(Cell::new(true));
    let factory = SettingsFactory::new(&owner, &access, config()).unwrap();
    let mut runtime = factory.runtime(Budgets::new(1, 1, 1).unwrap());
    let mut wire = wire::Wire::new();
    let mut ui = wire::Background::default();
    let mut bg = SettingsWork::new(&owner);
    wire.request(0, 1, 1, &[1, 1, 128, 0, 0, 0, 3, 0]);
    runtime
        .step(0, &mut wire, &mut ui, &mut bg)
        .connection
        .unwrap();
    let mut local = AsyncSettings::new(&owner, dictionary()).unwrap();
    blocked.set(true);
    local.begin(Field::Format, 1, &access).unwrap();
    wire.request(4, 0x10, 2, &[3, 0]);
    runtime
        .step(1, &mut wire, &mut ui, &mut bg)
        .connection
        .unwrap();
    assert_eq!(wire.response().payload.last(), Some(&0));
    wire.request(4, 0x11, 3, &[3, 0, 0, 0, 0, 0, 1, 0, 0]);
    runtime
        .step(2, &mut wire, &mut ui, &mut bg)
        .connection
        .unwrap();
    assert_eq!(
        &wire.response().payload[..2],
        &(device_services::Status::Busy as u16).to_le_bytes()
    );
    assert_eq!(owner.revision(), Ok(1));
    blocked.set(false);
    for now in 3..2000 {
        runtime
            .step(now, &mut wire, &mut ui, &mut bg)
            .background
            .unwrap();
        if local.poll(&access).unwrap() {
            break;
        }
    }
    assert_eq!(
        local
            .load(&access, wattch5_parity_ui_demo::Appearance::default())
            .unwrap()
            .format,
        TimeFormat::Hour12
    );
    local.finish().unwrap();
    assert_eq!(owner.revision(), Ok(2));
    runtime.shutdown(&mut wire).unwrap();
}
#[test]
fn test_should_refresh_async_ui_from_ble_confirmed_owner_and_reject_missing_time() {
    let nor = Nor::new();
    let owner = initialized(nor);
    let time = async_support::time::time_owner();
    let access = Access(Cell::new(true));
    let mut ui = AsyncProductUi::new(
        AsyncSettings::new(&owner, dictionary()).unwrap(),
        &time,
        UiAuthority {
            properties: &access,
            time: &async_support::time::TimeScope,
        },
        editor(),
        RamFields::Unavailable,
        async_support::ui::ports(),
    )
    .unwrap();
    let token = owner.begin_set(4, 3, 0, &[1], &access).unwrap();
    ui.step(0).unwrap();
    assert_eq!(ui.model().appearance().format, TimeFormat::Hour24);
    for _ in 0..2000 {
        owner.step().unwrap();
        if owner.result(token).unwrap().is_ready() {
            break;
        }
    }
    owner.take_result(token).unwrap().unwrap();
    ui.step(1).unwrap();
    assert_eq!(ui.model().appearance().format, TimeFormat::Hour12);
    ui.ports_mut().observations.unavailable = true;
    assert_eq!(ui.step(2), Err(PortError::Unavailable));
    let blank = device_time_store::TimeStore::open(
        device_storage::persistence::AtomicSnapshot::<_, 128>::open(
            async_support::medium::Memory::new(),
            device_storage::persistence::Layout {
                start: 0,
                bank_bytes: 1024,
            },
        )
        .unwrap(),
        NoClock,
        20,
    )
    .unwrap();
    let mut ui = AsyncProductUi::new(
        AsyncSettings::new(&owner, dictionary()).unwrap(),
        &blank,
        UiAuthority {
            properties: &access,
            time: &async_support::time::TimeScope,
        },
        editor(),
        RamFields::Unavailable,
        async_support::ui::ports(),
    )
    .unwrap();
    assert_eq!(ui.step(3), Err(PortError::Unavailable));
    assert!(ui.ports_mut().display.rows.is_empty());
}
#[derive(Debug)]
struct NoClock;
impl device_time_store::TrustedClock for NoClock {
    fn reading(
        &self,
    ) -> Result<device_time_store::ClockReading, device_services::ClockSourceError> {
        Err(device_services::ClockSourceError::Unavailable)
    }
}
#[test]
fn test_should_complete_revoked_runtime_write_without_delivering_response() {
    let nor = Nor::new();
    let blocked = nor.blocked.clone();
    let owner = initialized(nor);
    let access = Access(Cell::new(true));
    let factory = SettingsFactory::new(&owner, &access, config()).unwrap();
    let mut runtime = factory.runtime(Budgets::new(1, 1, 1).unwrap());
    let mut wire = wire::Wire::new();
    let mut ui = wire::Background::default();
    let mut bg = SettingsWork::new(&owner);
    wire.request(0, 1, 1, &[1, 1, 128, 0, 0, 0, 3, 0]);
    runtime
        .step(0, &mut wire, &mut ui, &mut bg)
        .connection
        .unwrap();
    wire.sent.clear();
    blocked.set(true);
    wire.request(4, 0x11, 2, &[3, 0, 0, 0, 0, 0, 1, 0, 1]);
    runtime
        .step(1, &mut wire, &mut ui, &mut bg)
        .connection
        .unwrap();
    access.0.set(false);
    assert!(
        runtime
            .step(2, &mut wire, &mut ui, &mut bg)
            .connection
            .is_err()
    );
    assert!(wire.sent.is_empty());
    assert_eq!(wire.scope, None);
    blocked.set(false);
    for now in 3..2000 {
        runtime
            .step(now, &mut wire, &mut ui, &mut bg)
            .background
            .unwrap();
        if owner.revision() == Ok(2) {
            break;
        }
    }
    assert_eq!(owner.revision(), Ok(2));
    assert!(wire.sent.is_empty());
}

struct SetAccess(Cell<bool>);
impl device_services::PropertyAccess for SetAccess {
    fn allowed(
        &self,
        _: u8,
        operation: device_services::PropertyOperation,
        _: Option<u16>,
    ) -> bool {
        operation != device_services::PropertyOperation::Set || self.0.get()
    }
}
#[derive(Debug)]
struct RevokingObservation<'a> {
    inner: async_support::ui::Observations,
    set: &'a Cell<bool>,
    revoke: bool,
}
impl wattch5_product::runtime::ObservationSource for RevokingObservation<'_> {
    fn capture(&mut self) -> Result<wattch5_product::runtime::Observation<'_>, PortError> {
        if self.revoke {
            self.set.set(false);
        }
        self.inner.capture()
    }
}
#[test]
fn test_should_recheck_local_set_scope_after_observation_before_navigation_publication() {
    // GET may remain allowed while the local save scope revokes during a fallible provider call.
    let owner = initialized(Nor::new());
    let time = async_support::time::time_owner();
    let access = SetAccess(Cell::new(true));
    let ports = async_support::ui::ports();
    let mut ui = AsyncProductUi::new(
        AsyncSettings::new(&owner, dictionary()).unwrap(),
        &time,
        UiAuthority {
            properties: &access,
            time: &async_support::time::TimeScope,
        },
        editor(),
        RamFields::Unavailable,
        wattch5_product::runtime::UiPorts {
            touch: ports.touch,
            observations: RevokingObservation {
                inner: ports.observations,
                set: &access.0,
                revoke: false,
            },
            calendar: ports.calendar,
            display: ports.display,
            actions: ports.actions,
        },
    )
    .unwrap();
    ui.ports_mut().touch.0.push_back(Event::Up);
    ui.step(0).unwrap();
    ui.ports_mut().touch.0.push_back(Event::Select);
    ui.step(1).unwrap();
    for _ in 0..2000 {
        owner.step().unwrap();
        if owner.revision() == Ok(2) {
            break;
        }
    }
    ui.ports_mut().observations.revoke = true;
    assert_eq!(ui.step(2), Err(PortError::Unavailable));
    assert!(!ui.save_pending());
    assert_eq!(ui.model().screen(), Screen::Format);
    assert_eq!(ui.model().appearance().format, TimeFormat::Hour24);
    assert_eq!(owner.revision(), Ok(2));
}

#[test]
fn test_should_reject_early_finish_and_continue_after_local_controller_drop() {
    // An early finish or lost Drop detach would release too early or strand the slot.
    let owner = initialized(Nor::new());
    let access = Access(Cell::new(true));
    let mut local = AsyncSettings::new(&owner, dictionary()).unwrap();
    local.begin(Field::Format, 1, &access).unwrap();
    assert_eq!(local.finish(), Err(PortError::Fault));
    assert!(local.pending());
    assert!(!local.poll(&access).unwrap());
    drop(local);
    for _ in 0..2000 {
        owner.step().unwrap();
        if owner.revision() == Ok(2) {
            break;
        }
    }
    assert_eq!(owner.revision(), Ok(2));
    let mut local = AsyncSettings::new(&owner, dictionary()).unwrap();
    assert_eq!(
        local
            .load(&access, wattch5_parity_ui_demo::Appearance::default())
            .unwrap()
            .format,
        TimeFormat::Hour12
    );
    local.begin(Field::Format, 0, &access).unwrap();
}
#[test]
fn test_should_keep_editor_and_report_uncertainty_after_failed_exact_readback() {
    // Sync success alone must never publish the staged navigation candidate.
    let nor = Nor::new();
    let memory = nor.memory.clone();
    let owner = initialized(nor);
    let time = async_support::time::time_owner();
    let access = Access(Cell::new(true));
    let mut ui = AsyncProductUi::new(
        AsyncSettings::new(&owner, dictionary()).unwrap(),
        &time,
        UiAuthority {
            properties: &access,
            time: &async_support::time::TimeScope,
        },
        editor(),
        RamFields::Unavailable,
        async_support::ui::ports(),
    )
    .unwrap();
    ui.ports_mut().touch.0.push_back(Event::Up);
    ui.step(0).unwrap();
    ui.ports_mut().touch.0.push_back(Event::Select);
    ui.step(1).unwrap();
    memory.fail_reads(true);
    let mut error = None;
    for now in 2..2000 {
        let _ = owner.step();
        if let Err(e) = ui.step(now) {
            error = Some(e);
            break;
        }
    }
    assert_eq!(error, Some(PortError::Uncertain));
    assert!(!ui.save_pending());
    assert_eq!(ui.model().screen(), Screen::Format);
    assert_eq!(ui.model().appearance().format, TimeFormat::Hour24);
    assert!(owner.revision().is_err());
}
