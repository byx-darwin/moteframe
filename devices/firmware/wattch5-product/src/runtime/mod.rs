//! Callable CONTROL/UI composition over supplied real owners and platform ports.
//! See the adjacent README for startup, recovery, authority and unbound-port obligations.
mod settings;
pub use settings::{AsyncSettings, Dictionary, Settings, SettingsWork};
mod async_factory;
pub use async_factory::SettingsFactory;
mod ui;
pub use ui::{
    Actions, AsyncProductUi, Calendar, CalendarDate, NoActions, Observation, ObservationSource,
    ProductUi, RamFields, TouchSource, UiAuthority, UiPorts,
};
mod factory;
pub use factory::{
    BleAuthority, Extensions, NoExtensions, ProductFactory, ProductHandler, Unsupported,
};
mod warehouse;
pub use warehouse::{
    BlankSettings, HistoryRecovery, JobsRecovery, ObjectsRecovery, PropertiesRecovery,
    RecoverOwner, RecoveryReport, SelectedWarehouse, SharedConfigRecovery, TimeRecovery,
    initialize_blank_settings,
};
/// Actual pinned upstream CONTROL GATT transport; caller supplies platform bindings.
#[cfg(feature = "runtime-trouble")]
pub mod gatt;

/// Retained three-result product adapters; no hardware initialization or invented samples.
#[cfg(feature = "legacy-health")]
pub mod legacy_health;
