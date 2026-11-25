//! Stable compiled-contract versions; backend/runtime registration remains a separate acceptance step.
/// One compiled source contract, without an availability or permission assertion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModuleVersion {
    /// Stable module number; omitted optional modules never renumber later modules.
    pub module: u8,
    /// Module contract major version, independent of core wire or record/value schema revision.
    pub major: u8,
    /// Module contract minor version.
    pub minor: u8,
}
/// Feature-selected source contracts, in stable numeric order. This is not a runtime MODULES reply.
/// Actual module registration still requires a bound backend, live scope and verified limits.
pub const COMPILED_MODULES: &[ModuleVersion] = &[
    ModuleVersion {
        module: 0,
        major: 1,
        minor: 2,
    },
    #[cfg(feature = "time")]
    ModuleVersion {
        module: 1,
        major: 1,
        minor: 0,
    },
    #[cfg(any(feature = "device", feature = "system", feature = "device-jobs"))]
    ModuleVersion {
        module: 2,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "profile")]
    ModuleVersion {
        module: 3,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "settings")]
    ModuleVersion {
        module: 4,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "reminders")]
    ModuleVersion {
        module: 5,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "find")]
    ModuleVersion {
        module: 6,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "phone")]
    ModuleVersion {
        module: 7,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "companion-radio")]
    ModuleVersion {
        module: 8,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "telemetry")]
    ModuleVersion {
        module: 9,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "history")]
    ModuleVersion {
        module: 16,
        major: 1,
        minor: 2,
    },
    #[cfg(feature = "extension")]
    ModuleVersion {
        module: 17,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "heart-rate")]
    ModuleVersion {
        module: 32,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "sleep")]
    ModuleVersion {
        module: 33,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "sport")]
    ModuleVersion {
        module: 34,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "activity")]
    ModuleVersion {
        module: 35,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "wear")]
    ModuleVersion {
        module: 36,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "blood-pressure")]
    ModuleVersion {
        module: 37,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "oxygen")]
    ModuleVersion {
        module: 38,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "glucose")]
    ModuleVersion {
        module: 39,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "temperature")]
    ModuleVersion {
        module: 40,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "ecg")]
    ModuleVersion {
        module: 41,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "wellbeing")]
    ModuleVersion {
        module: 42,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "microcirculation")]
    ModuleVersion {
        module: 43,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "rri")]
    ModuleVersion {
        module: 44,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "womens-health")]
    ModuleVersion {
        module: 45,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "uv")]
    ModuleVersion {
        module: 46,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "notifications")]
    ModuleVersion {
        module: 48,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "watchface")]
    ModuleVersion {
        module: 49,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "weather")]
    ModuleVersion {
        module: 50,
        major: 1,
        minor: 0,
    },
    #[cfg(any(feature = "media", feature = "gimbal"))]
    ModuleVersion {
        module: 51,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "contacts")]
    ModuleVersion {
        module: 52,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "wallet")]
    ModuleVersion {
        module: 53,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "memo")]
    ModuleVersion {
        module: 54,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "hid")]
    ModuleVersion {
        module: 55,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "antplus")]
    ModuleVersion {
        module: 56,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "radiation")]
    ModuleVersion {
        module: 57,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "raw-data")]
    ModuleVersion {
        module: 112,
        major: 1,
        minor: 2,
    },
    #[cfg(feature = "self-test")]
    ModuleVersion {
        module: 113,
        major: 1,
        minor: 1,
    },
    #[cfg(feature = "engineering")]
    ModuleVersion {
        module: 114,
        major: 1,
        minor: 0,
    },
    #[cfg(feature = "factory-config")]
    ModuleVersion {
        module: 115,
        major: 1,
        minor: 0,
    },
];
