//! Actual selected validators, with explicit test-only capabilities and defaults.
#![allow(
    clippy::expect_used,
    reason = "Test fixture construction and assertions deliberately panic on failure."
)]
#![cfg(all(
    feature = "time",
    feature = "device",
    feature = "profile",
    feature = "settings",
    feature = "reminders",
    feature = "find",
    feature = "heart-rate",
    feature = "sleep",
    feature = "sport",
    feature = "activity",
    feature = "temperature",
    feature = "notifications",
    feature = "watchface"
))]
mod support;
use device_configuration::{HealthCapability, ModuleLimits, ProductCapabilities, Range};
use device_property_store::{
    CodecSchemas, DefaultValue, ModuleCapabilities, ModuleCatalogue, PropertySchema, PropertyStore,
};
use device_protocol::{Frame, Header, Kind};
use device_services::{
    PropertyAccess, PropertyBackend, PropertyOperation, PropertyService, Status,
};
use device_storage::persistence::{AtomicSnapshot, Layout};
fn frame(module: u8, opcode: u8, payload: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module,
            opcode,
            message_id: 1,
            offset: 0,
            total_length: u16::try_from(payload.len()).expect("bounded"),
        },
        payload,
    }
}
#[derive(Debug)]
struct Allow;
impl PropertyAccess for Allow {
    fn allowed(&self, _: u8, _: PropertyOperation, _: Option<u16>) -> bool {
        true
    }
}
fn capabilities() -> ProductCapabilities<'static> {
    ProductCapabilities {
        limits: Some(ModuleLimits {
            max_objects: 8,
            max_object_bytes: 512,
            max_string_bytes: 32,
        }),
        languages: Some(&["en", "zh"]),
        saver_levels: Some(&[0, 250, 1000]),
        normal_levels: Some(&[0, 500, 1000]),
        display_ms: Some(Range {
            min: 10,
            max: 10000,
        }),
        health: Some(&[HealthCapability {
            module: 0x28,
            interval_s: Range { min: 1, max: 1000 },
            temperature_modes: 15,
        }]),
        automatic_sports: Some(&[0, 1]),
        auto_sport_delay_s: Some(Range { min: 1, max: 120 }),
        activity_segment_s: Some(Range { min: 1, max: 3600 }),
        ..ProductCapabilities::NONE
    }
}
fn manifest() -> device_content::Manifest<'static> {
    device_content::Manifest {
        string_max: Some(32),
        property_max: Some(128),
        count_max: Some(8),
        find_delay_s: Some(device_content::Range { min: 1, max: 60 }),
        watchface: Some(device_content::WatchfaceManifest {
            revision: 1,
            width: 240,
            height: 240,
            dials: &[7, 9],
            groups: &[],
            elements: &[],
            styles: &[],
            verified_resources: false,
        }),
        ..Default::default()
    }
}
fn values() -> Vec<(u8, u16, Vec<u8>)> {
    vec![
        (1, 1, vec![0xe0, 1, 0x10, 0xff, 3, 0, b'L', b'o', b'n']),
        (2, 5, vec![4, 0, b't', b'e', b's', b't']),
        (3, 1, vec![0; 14]),
        (3, 2, vec![0; 18]),
        (4, 1, vec![1]),
        (4, 2, vec![0, 1]),
        (4, 3, vec![1]),
        (4, 4, vec![1, 127, 0, 0, 0x9f, 5]),
        (4, 5, vec![0]),
        (4, 6, vec![10, 0, 0, 0, 20, 0, 0, 0]),
        (4, 7, vec![2, 0, b'e', b'n']),
        (4, 8, vec![0, 127, 1, 0, 2, 0]),
        (4, 9, vec![250, 0]),
        (4, 10, vec![0xf4, 1]),
        (5, 1, vec![0, 0, 0, 0, 0, 0]),
        (6, 1, vec![1, 5, 0]),
        (0x20, 2, vec![1, 100, 0]),
        (0x21, 1, vec![1, 127, 0, 0, 0, 0]),
        (0x22, 1, vec![1, 10, 0, 0, 0, 1, 0, 1]),
        (0x23, 1, vec![60, 0, 0, 0]),
        (0x28, 1, vec![1, 10, 0, 0, 0, 1]),
        (0x28, 2, vec![1, 0x10, 0x0e, 1]),
        (0x28, 3, vec![0, 0x10, 0x0e]),
        (0x30, 1, vec![1, 0, 0, 1]),
        (0x30, 2, vec![1]),
        (0x30, 3, vec![1, 0, 1, 0, b'x']),
        (0x30, 4, vec![1, 1, 0, b'x', 1]),
        (0x31, 1, vec![7, 0, 0, 0]),
    ]
}
fn exercise_wire<
    S: device_storage::persistence::SnapshotStore,
    C: PropertySchema,
    const N: usize,
>(
    owner: &PropertyStore<'_, S, C, N>,
    values: &[(u8, u16, Vec<u8>)],
    memory: &support::Memory,
) {
    for (module, key, value) in values {
        let mut adapter = owner.adapter(*module).expect("module");
        let mut service =
            PropertyService::new(*module, &mut adapter, 128).expect("actual wire service");
        let mut out = [0; 128];
        let n = service
            .dispatch(frame(*module, 0x10, &key.to_le_bytes()), &Allow, &mut out)
            .expect("GET actual schema");
        assert_eq!(&out[4..8], &1u32.to_le_bytes());
        assert_eq!(&out[12..n], value);
        let mut payload = Vec::new();
        payload.extend_from_slice(&key.to_le_bytes());
        payload.extend_from_slice(&1u32.to_le_bytes());
        payload.extend_from_slice(&u16::try_from(value.len()).expect("length").to_le_bytes());
        payload.extend_from_slice(value);
        let n = service
            .dispatch(frame(*module, 0x11, &payload), &Allow, &mut out)
            .expect("SET actual schema");
        assert_eq!(&out[..n], &[0, 0, 0, 0, 2, 0, 0, 0]);
        payload[6..8].copy_from_slice(
            &u16::try_from(value.len() + 1)
                .expect("length")
                .to_le_bytes(),
        );
        payload.push(0);
        let before = memory.mutations();
        let n = service
            .dispatch(frame(*module, 0x11, &payload), &Allow, &mut out)
            .expect("invalid actual value");
        assert_eq!(n, 4);
        assert_eq!(&out[..2], &(Status::InvalidArgument as u16).to_le_bytes());
        assert_eq!(memory.mutations(), before);
    }
}
fn reject_changed_capabilities(
    caps: &[ModuleCapabilities<'_>],
    catalogue: &[ModuleCatalogue<'_>],
    memory: &support::Memory,
) {
    let mut changed = caps.to_vec();
    changed
        .iter_mut()
        .find(|c| c.module == 4)
        .expect("settings")
        .capabilities
        .languages = Some(&["zh"]);
    let snapshot = AtomicSnapshot::<_, 2048>::open(
        memory.clone(),
        Layout {
            start: 0,
            bank_bytes: 4096,
        },
    )
    .expect("snapshot");
    assert!(matches!(
        PropertyStore::<_, _, 2048>::open(
            snapshot,
            catalogue,
            CodecSchemas::new(&changed).with_content(manifest())
        ),
        Err(device_property_store::Error::Integrity)
    ));
}
#[test]
fn test_should_validate_and_durably_reopen_all_28_selected_actual_codecs() {
    let values = values();
    assert_eq!(values.len(), 28);
    let mut caps = Vec::new();
    for &(module, _, _) in &values {
        if caps
            .last()
            .is_none_or(|c: &ModuleCapabilities<'_>| c.module != module)
        {
            caps.push(ModuleCapabilities {
                module,
                capabilities: capabilities(),
            });
        }
    }
    let schemas = CodecSchemas::new(&caps).with_content(manifest());
    let mut grouped: Vec<(u8, Vec<device_services::PropertyDescriptor>)> = Vec::new();
    for (module, key, value) in &values {
        let mut descriptor = schemas
            .descriptor(*module, *key)
            .expect("actual compiled codec");
        descriptor.value_max = u16::try_from(value.len() + 8)
            .expect("bounded")
            .min(descriptor.value_max);
        schemas
            .validate(*module, *key, value)
            .expect("complete actual value");
        let mut trailing = value.clone();
        trailing.push(0);
        assert!(
            schemas.validate(*module, *key, &trailing).is_err(),
            "strict tail {module:02x}.{key}"
        );
        if grouped.last().is_none_or(|g| g.0 != *module) {
            grouped.push((*module, vec![]));
        }
        grouped.last_mut().expect("group").1.push(descriptor);
    }
    let catalogue: Vec<_> = grouped
        .iter()
        .map(|(module, properties)| ModuleCatalogue {
            module: *module,
            properties,
        })
        .collect();
    let defaults: Vec<_> = values
        .iter()
        .map(|(module, key, value)| DefaultValue {
            module: *module,
            key: *key,
            value,
        })
        .collect();
    let memory = support::Memory::new();
    let snapshot = AtomicSnapshot::<_, 2048>::open(
        memory.clone(),
        Layout {
            start: 0,
            bank_bytes: 4096,
        },
    )
    .expect("snapshot");
    let owner = PropertyStore::<_, _, 2048>::open(snapshot, &catalogue, schemas).expect("owner");
    owner.initialize(&defaults).expect("all explicit defaults");
    exercise_wire(&owner, &values, &memory);
    drop(owner.into_store());
    let snapshot = AtomicSnapshot::<_, 2048>::open(
        memory.clone(),
        Layout {
            start: 0,
            bank_bytes: 4096,
        },
    )
    .expect("restart snapshot");
    let owner = PropertyStore::<_, _, 2048>::open(
        snapshot,
        &catalogue,
        CodecSchemas::new(&caps).with_content(manifest()),
    )
    .expect("restart owner");
    for (module, key, value) in &values {
        let mut out = [0; 512];
        let p = owner
            .adapter(*module)
            .expect("module")
            .read(*key, &mut out)
            .expect("durable readback");
        assert_eq!(p.revision, 2);
        assert_eq!(&out[..p.length], value);
    }
    assert_eq!(owner.revision(), Ok(29));
    drop(owner.into_store());
    reject_changed_capabilities(&caps, &catalogue, &memory);
}
#[test]
fn test_should_reject_actual_product_range_dictionary_and_dynamic_routes() {
    let modules = [
        ModuleCapabilities {
            module: 4,
            capabilities: capabilities(),
        },
        ModuleCapabilities {
            module: 6,
            capabilities: capabilities(),
        },
        ModuleCapabilities {
            module: 0x30,
            capabilities: capabilities(),
        },
        ModuleCapabilities {
            module: 0x31,
            capabilities: capabilities(),
        },
    ];
    let schemas = CodecSchemas::new(&modules).with_content(manifest());
    for (m, k, b) in [
        (4, 7, &[2, 0, b'f', b'r'][..]),
        (4, 9, &[251, 0][..]),
        (6, 1, &[1, 61, 0][..]),
        (0x30, 4, &[2, 1, 0, b'x', 1, 1, 0, b'x', 0][..]),
        (0x31, 1, &[8, 0, 0, 0][..]),
    ] {
        assert!(schemas.validate(m, k, b).is_err());
    }
    for (m, k) in [
        (0x20, 1),
        (0x20, 3),
        (0x22, 2),
        (0x2a, 1),
        (7, 2),
        (7, 3),
        (0x2a, 3),
        (2, 1),
        (4, 11),
        (0x21, 2),
        (4, 0xf000),
        (0x40, 1),
    ] {
        assert!(schemas.descriptor(m, k).is_err());
    }
}
