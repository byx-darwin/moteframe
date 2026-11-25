//! Real durable product owners with test-only storage/authority/clock inputs.
#![allow(clippy::unwrap_used)]
use super::medium;
use device_configuration::ProductCapabilities;
use device_property_store::{
    CodecSchemas, DefaultValue, ModuleCapabilities, ModuleCatalogue, PropertyStore,
};
use device_protocol::{ModuleEntry, ProtocolLimits};
use device_services::{
    BuildProfile, ClockSourceError, Config, DeviceInfo, PropertyAccess, PropertyDescriptor,
    PropertyOperation, TimeAccess,
};
use device_storage::persistence::{AtomicSnapshot, Layout};
use device_time_store::{ClockReading, TimeStore, TrustedClock};
use std::cell::Cell;
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
pub struct Access(pub bool);
impl PropertyAccess for Access {
    fn allowed(&self, _: u8, _: PropertyOperation, _: Option<u16>) -> bool {
        self.0
    }
}
pub type Owner =
    PropertyStore<'static, AtomicSnapshot<medium::Memory, 1024>, CodecSchemas<'static>, 1024>;
pub fn open(memory: medium::Memory) -> Owner {
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
pub fn initialized(memory: medium::Memory) -> Owner {
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
#[derive(Debug)]
pub struct Clock(Cell<u64>);
impl TrustedClock for Clock {
    fn reading(&self) -> Result<ClockReading, ClockSourceError> {
        Ok(ClockReading {
            source_id: 7,
            epoch: 11,
            mono_us: self.0.get(),
        })
    }
}
pub struct TimeScope;
impl TimeAccess for TimeScope {
    fn allowed(&self, _: bool) -> bool {
        true
    }
}
pub type TimeOwner = TimeStore<AtomicSnapshot<medium::Memory, 128>, Clock>;
pub fn time_owner() -> TimeOwner {
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
pub fn config() -> Config<'static> {
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
