//! Optional typed configuration codecs. Schema presence does not advertise a real device backend.
#![no_std]
#![forbid(unsafe_code)]
#![allow(clippy::too_many_lines)]
use core::fmt;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Unknown,
    Invalid,
    Readonly,
    Unavailable,
    Unsupported,
    Capacity,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub min: u32,
    pub max: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WindowCapability {
    pub id: u16,
    pub parent: u16,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HidBinding {
    pub platform: u8,
    pub gesture: u8,
    pub action: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HealthCapability {
    pub module: u8,
    pub interval_s: Range,
    pub temperature_modes: u8,
}
#[derive(Clone, Copy, Debug)]
pub struct DiagnosticCapability {
    pub id: u16,
    pub reason: u16,
    pub parameter_length: u16,
    pub revision: u16,
    pub validate_parameters: fn(&[u8]) -> bool,
}
#[derive(Clone, Copy)]
pub struct ProductCapabilities<'a> {
    /// Per-module F000 limits for this validation call; absent means wire-only size bounds.
    pub limits: Option<ModuleLimits>,
    pub languages: Option<&'a [&'a str]>,
    pub saver_levels: Option<&'a [u16]>,
    pub normal_levels: Option<&'a [u16]>,
    pub display_ms: Option<Range>,
    pub windows: Option<&'a [WindowCapability]>,
    pub health: Option<&'a [HealthCapability]>,
    pub mood_values: Option<&'a [i16]>,
    pub hid: Option<&'a [HidBinding]>,
    pub antplus_backend: bool,
    /// True only after firmware authentication/profile checks and real engineering backend selection.
    pub engineering: bool,
    pub amplitude: Option<Range>,
    pub tap_interval_ms: Option<Range>,
    pub wear_counts: Option<Range>,
    pub diagnostics: Option<&'a [DiagnosticCapability]>,
    /// Current HR policy and selected RMSSD backend, checked atomically by the caller.
    pub rmssd_allowed: bool,
    pub automatic_sports: Option<&'a [u16]>,
    pub auto_sport_delay_s: Option<Range>,
    pub activity_segment_s: Option<Range>,
}
impl ProductCapabilities<'_> {
    pub const NONE: Self = Self {
        limits: None,
        languages: None,
        saver_levels: None,
        normal_levels: None,
        display_ms: None,
        windows: None,
        health: None,
        mood_values: None,
        hid: None,
        antplus_backend: false,
        engineering: false,
        amplitude: None,
        tap_interval_ms: None,
        wear_counts: None,
        diagnostics: None,
        rmssd_allowed: false,
        automatic_sports: None,
        auto_sport_delay_s: None,
        activity_segment_s: None,
    };
}
impl fmt::Debug for ProductCapabilities<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProductCapabilities")
            .finish_non_exhaustive()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    ReadOnly,
    ReadWrite,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SchemaType {
    Boolean,
    Struct,
    Utf8,
    Object,
    Directory,
    Extension,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SchemaDescriptor {
    pub module: u8,
    pub key: u16,
    pub schema_type: SchemaType,
    pub revision: u16,
    pub max_length: u64,
    pub access: Access,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Timezones<'a> {
    pub offset_min: i16,
    pub second_offset_min: i16,
    pub second_city: &'a str,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Identity<'a> {
    pub product: u32,
    pub subtype: u16,
    pub group: u16,
    pub serial: &'a str,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Value<'a> {
    ModuleLimits(ModuleLimits),
    Timezones(Timezones<'a>),
    Identity(Identity<'a>),
    Text(&'a str),
    Boolean(bool),
    Battery(Battery),
    Ui(Ui<'a>),
    Hardware(Hardware<'a>),
    Reset(Reset),
    Address(Address),
    Units {
        distance: u8,
        temperature: u8,
    },
    HourFormat(u8),
    DisplayTimeouts {
        main_ms: u32,
        other_ms: u32,
    },
    Brightness(u16),
    BrightnessLimits(BrightnessLimits<'a>),
    #[cfg(any(
        feature = "settings",
        feature = "oxygen",
        feature = "reminders",
        feature = "sleep"
    ))]
    Window(device_models::TimeWindow),
    WindowObject(WindowObject),
    Find {
        enabled: bool,
        delay_s: u16,
    },
    TelemetryDisabled,
    HeartPolicy(HeartPolicy),
    Threshold {
        enabled: bool,
        value: u16,
    },
    BloodPressureReference {
        enabled: bool,
        high: u16,
        low: u16,
    },
    /// Actual BP algorithm/module version, never a firmware or host fallback.
    #[cfg(feature = "blood-pressure")]
    BloodPressureVersion(&'a str),
    #[cfg(feature = "oxygen")]
    OxygenPolicy {
        enabled: bool,
        interval_s: u32,
        window: device_models::TimeWindow,
    },
    Zones(Zones),
    Glucose(Glucose),
    TemperaturePolicy {
        enabled: bool,
        interval_s: u32,
        mode: u8,
    },
    TemperatureAlarm {
        enabled: bool,
        centi_c: i16,
        mode: u8,
    },
    TemperatureReference {
        enabled: bool,
        centi_c: i16,
    },
    WellbeingImport(WellbeingImport),
    AlgorithmDirectory(AlgorithmDirectory<'a>),
    Hid(HidMappings<'a>),
    AntState(u8),
    LogPolicy {
        enabled: bool,
        read_policy: u8,
    },
    Tap {
        amplitude: u32,
        minimum_interval_ms: u32,
    },
    WearThreshold(u16),
    DiagnosticLog(DiagnosticLog<'a>),
    #[cfg(feature = "profile")]
    UserProfile(device_models::UserProfile),
    #[cfg(feature = "profile")]
    ActivityGoals(device_models::ActivityGoals),
    #[cfg(feature = "reminders")]
    Reminder(device_models::Reminder<'a>),
    #[cfg(feature = "womens-health")]
    Womens(device_wellness::WomensConfig),
    #[cfg(feature = "uv")]
    UvPolicy(device_wellness::UvPolicy),
    #[cfg(feature = "sleep")]
    SleepDetection(device_motion::SleepDetection),
    SportAuto {
        enabled: bool,
        delay_s: u32,
        sport_type: u16,
        trigger_kind: u8,
    },
    ActivitySegment(u32),
    RadiationThreshold(u16),
    Extension(&'a [u8]),
}
impl fmt::Debug for Value<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Value { contents: redacted }")
    }
}

mod dto;
#[allow(dead_code)]
mod wire;
pub use dto::*;
mod registry;
pub use registry::*;

mod catalog;
pub use catalog::*;
