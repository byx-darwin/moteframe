//! Portable application adapters. Real persistence, authorization and physical backends remain explicit ports.
#![no_std]
#![forbid(unsafe_code)]

pub mod actions;
#[cfg(feature = "configuration")]
mod schema;
#[cfg(feature = "configuration")]
pub use schema::{SchemaContext, SchemaObjects, SchemaProperties};

mod shared;
pub use shared::{CellBusy, ServiceCell, ServiceHandle};

mod resources;
pub use resources::{CreditPort, DatasetPort, ResourceRouter};

#[cfg(any(
    feature = "phone",
    feature = "companion-radio",
    feature = "find",
    feature = "notifications",
    feature = "watchface",
    feature = "weather",
    feature = "contacts",
    feature = "wallet",
    feature = "memo"
))]
mod content;
#[cfg(any(
    feature = "phone",
    feature = "companion-radio",
    feature = "find",
    feature = "notifications",
    feature = "watchface",
    feature = "weather",
    feature = "contacts",
    feature = "wallet",
    feature = "memo"
))]
pub use content::ContentSchemas;

#[cfg(feature = "history")]
mod history;
#[cfg(feature = "history")]
pub use history::{HistoryEventClock, HistoryEventToken, HistoryEvents, HistoryPort};

#[cfg(feature = "system")]
mod system;
#[cfg(feature = "system")]
pub use system::{SystemAccess, SystemFailure, SystemHandler, SystemPort};

#[cfg(feature = "device")]
pub use device_models::BatteryState;

mod events;
pub use events::{Change, ControlEvent};

#[cfg(any(
    feature = "phone",
    feature = "media",
    feature = "gimbal",
    feature = "companion-radio",
    feature = "find",
    feature = "notifications",
    feature = "watchface",
    feature = "weather",
    feature = "contacts",
    feature = "wallet",
    feature = "memo"
))]
pub use device_content as content_protocol;
#[cfg(any(
    feature = "heart-rate",
    feature = "blood-pressure",
    feature = "oxygen",
    feature = "glucose",
    feature = "temperature",
    feature = "ecg",
    feature = "wellbeing",
    feature = "microcirculation"
))]
pub use device_health as health;

mod catalogue;
pub use catalogue::{COMPILED_MODULES, ModuleVersion};

#[cfg(feature = "configuration")]
pub use device_configuration as configuration;

pub use device_connection::{
    EventOffer, EventPlane, EventPump, EventRoute, EventSource, PumpError, PumpFailure,
    SourceFailure,
};

/// Optional typed business handlers; each leaf feature remains independently selected.
#[cfg(feature = "service-routing")]
pub mod handlers {
    pub use device_content_service as content;
    pub use device_health_service as health;
    pub use device_maintenance_service as maintenance;
    pub use device_motion_service as motion;
    pub use device_wellness_service as wellness;
}
/// Commercial persistent device job protocol; no engineering/raw/self-test dependency.
#[cfg(feature = "device-jobs")]
pub use device_maintenance_service as device_jobs;
/// Common object transaction handler. Real repository and schema ports are mandatory.
#[cfg(feature = "objects")]
pub use device_object_service as objects;

mod event_pair;
pub use event_pair::{EventPair, EventPairToken};

mod common_events;
pub use common_events::{CommonEventPort, CommonEvents};
