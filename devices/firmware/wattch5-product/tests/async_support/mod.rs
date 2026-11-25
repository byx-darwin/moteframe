//! Offline NOR/authority fixtures; real engine, owner, codecs and wire handlers.
#![allow(dead_code)]
#[path = "../runtime_support/medium.rs"]
pub mod medium;
pub mod time;
use core::task::Poll;
use device_configuration::ProductCapabilities;
use device_property_store::{
    AsyncPropertyStore, CodecSchemas, DefaultValue, ModuleCapabilities, ModuleCatalogue,
};
use device_protocol::ModuleEntry;
use device_services::{Config, PropertyAccess, PropertyDescriptor, PropertyOperation};
use device_storage::incremental::{
    AsyncMedium, Completion, IncrementalSnapshot, MediumOp, StartError,
};
use device_storage::persistence::{Error, Geometry, Layout, Medium};
use std::{cell::Cell, rc::Rc};
pub const LEVELS: [u16; 5] = [100, 200, 400, 700, 1000];
pub const LANGUAGES: [&str; 2] = ["zh-CN", "en-US"];
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
pub const MODULES: [ModuleEntry; 2] = [
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
        module: 4,
        major: 1,
        minor: 0,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    },
];
pub fn config() -> Config<'static> {
    Config {
        session: 1,
        capability_rev: 1,
        profile: device_services::BuildProfile::Commercial,
        info: device_services::DeviceInfo {
            product_id: 5,
            fw_major: 1,
            fw_minor: 0,
            fw_patch: 0,
            build_id: 19,
        },
        modules: &MODULES,
        limits: device_protocol::ProtocolLimits {
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
#[derive(Debug)]
pub struct Access(pub Cell<bool>);
impl PropertyAccess for Access {
    fn allowed(&self, _: u8, _: PropertyOperation, _: Option<u16>) -> bool {
        self.0.get()
    }
}
#[derive(Debug)]
enum Owned {
    Read(u32, usize),
    Program(u32, Vec<u8>),
    Erase(u32),
    Sync,
}
#[derive(Debug)]
pub struct Nor {
    pub memory: medium::Memory,
    pub blocked: Rc<Cell<bool>>,
    pub actions: Rc<Cell<usize>>,
    pending: Option<Owned>,
    waited: bool,
}
impl Nor {
    pub fn new() -> Self {
        Self {
            memory: medium::Memory::new(),
            blocked: Rc::new(Cell::new(false)),
            actions: Rc::new(Cell::new(0)),
            pending: None,
            waited: false,
        }
    }
}
impl AsyncMedium for Nor {
    fn geometry(&self) -> Geometry {
        self.memory.geometry()
    }
    fn begin(&mut self, op: MediumOp<'_>) -> Result<(), StartError> {
        self.actions.set(self.actions.get() + 1);
        if self.pending.is_some() {
            return Err(StartError::Busy);
        }
        self.pending = Some(match op {
            MediumOp::Read { offset, length } => {
                assert!(length <= 256);
                Owned::Read(offset, length)
            }
            MediumOp::Program { offset, bytes } => {
                assert!(bytes.len() <= 256);
                Owned::Program(offset, bytes.to_vec())
            }
            MediumOp::Erase { offset } => Owned::Erase(offset),
            MediumOp::Sync => Owned::Sync,
        });
        self.waited = false;
        Ok(())
    }
    fn poll(&mut self, out: &mut [u8]) -> Poll<Result<Completion, Error>> {
        self.actions.set(self.actions.get() + 1);
        if self.blocked.get() || !self.waited {
            self.waited = true;
            return Poll::Pending;
        }
        let Some(op) = self.pending.take() else {
            return Poll::Ready(Err(Error::Io));
        };
        Poll::Ready(match op {
            Owned::Read(offset, n) => self
                .memory
                .read(offset, &mut out[..n])
                .map(|()| Completion::Read(n)),
            Owned::Program(offset, bytes) => self
                .memory
                .program(offset, &bytes)
                .map(|()| Completion::Programmed),
            Owned::Erase(offset) => self
                .memory
                .erase(offset, self.memory.geometry().erase_size)
                .map(|()| Completion::Erased),
            Owned::Sync => self.memory.sync().map(|()| Completion::Synced),
        })
    }
}
pub type Owner = AsyncPropertyStore<'static, Nor, CodecSchemas<'static>, 128>;
pub fn open(nor: Nor) -> Owner {
    let mut engine = IncrementalSnapshot::new_unrecovered(
        nor,
        Layout {
            start: 0,
            bank_bytes: 1024,
        },
    )
    .unwrap();
    engine.begin_recover().unwrap();
    let mut done = false;
    for _ in 0..4000 {
        if let Poll::Ready(result) = engine.poll_recover() {
            result.unwrap();
            done = true;
            break;
        }
    }
    assert!(done);
    Owner::open(engine, &CATALOGUE, CodecSchemas::new(&CAPS)).unwrap()
}
pub fn initialized(nor: Nor) -> Owner {
    let owner = open(nor);
    let token = owner
        .begin_initialize(&[
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
    for _ in 0..4000 {
        owner.step().unwrap();
        if owner.result(token).unwrap().is_ready() {
            owner.take_result(token).unwrap().unwrap();
            return owner;
        }
    }
    panic!("initialization did not complete")
}
pub fn dictionary() -> wattch5_product::runtime::Dictionary<'static> {
    wattch5_product::runtime::Dictionary {
        normal_levels: LEVELS,
        languages: LANGUAGES,
    }
}

pub mod ui;
