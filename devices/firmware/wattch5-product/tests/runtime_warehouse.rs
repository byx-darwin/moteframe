//! Six actual durable owner families and real two-bank persistence recovery.
#![cfg(feature = "runtime")]
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "runtime_support/medium.rs"]
mod medium;
use device_history_service::{HistoryAccess, HistoryFailure, HistoryOperation, RecordStore};
use device_history_store::{AppendRecord, DomainConfig, HistoryClock, HistorySchema, HistoryStore};
use device_job_store::{JobStore, ResetIsolation, TrustedBoot, TrustedJobPolicy};
use device_maintenance_service::Error as JobError;
use device_maintenance_service::{DeviceJobAccess, DeviceJobLedger, DeviceJobRecord, NamedRequest};
use device_property_store::{
    CodecSchemas, DefaultValue, ModuleCapabilities, ModuleCatalogue, PropertyStore,
};
use device_services::{ClockSourceError, PropertyDescriptor, TimeAccess};
use device_storage::persistence::{AtomicSnapshot, Layout};
use device_storage::{
    Authorization, Begin, Commit, ObjectSchema, ObjectStore, StoreError, StoredRecord,
};
use device_time_store::{ClockReading, TimeStore, TrustedClock};
use wattch5_product::runtime::{BlankSettings, SelectedWarehouse, initialize_blank_settings};
fn snap<const N: usize>(memory: medium::Memory) -> AtomicSnapshot<medium::Memory, N> {
    AtomicSnapshot::open(
        memory,
        Layout {
            start: 0,
            bank_bytes: 4096,
        },
    )
    .unwrap()
}
#[derive(Debug)]
struct Schema;
impl ObjectSchema for Schema {
    fn validate(&self, m: u8, k: u16, b: &[u8]) -> Result<(), StoreError> {
        if m == 1 && k == 7 && b == [42] {
            Ok(())
        } else {
            Err(StoreError::Schema)
        }
    }
}
impl HistorySchema for Schema {
    fn validate(&self, r: StoredRecord<'_>, parent: u64) -> Result<(), HistoryFailure> {
        if r.schema == 1 && r.payload == [1, 11] && parent == 1 {
            Ok(())
        } else {
            Err(HistoryFailure::Invalid)
        }
    }
}
#[derive(Debug)]
struct Clock;
impl HistoryClock for Clock {
    fn now_ms(&self) -> Result<u64, HistoryFailure> {
        Ok(100)
    }
}
impl TrustedClock for Clock {
    fn reading(&self) -> Result<ClockReading, ClockSourceError> {
        Ok(ClockReading {
            source_id: 7,
            epoch: 2,
            mono_us: 0,
        })
    }
}
struct Access;
impl Authorization for Access {
    fn write_allowed(&self, m: u8) -> bool {
        m == 1
    }
}
impl DeviceJobAccess for Access {
    fn identity(&self) -> Option<[u8; 16]> {
        Some([1; 16])
    }
    fn start_allowed(&self, op: u16) -> bool {
        matches!(op, 1 | 2)
    }
    fn dangerous_allowed(&self) -> bool {
        true
    }
    fn read_scope(&self) -> bool {
        true
    }
    fn may_read(&self, r: DeviceJobRecord) -> bool {
        r.principal == [1; 16]
    }
}
impl TimeAccess for Access {
    fn allowed(&self, _: bool) -> bool {
        true
    }
}
impl HistoryAccess for Access {
    fn module_allowed(&self) -> bool {
        true
    }
    fn allowed(&self, _: [u8; 16], _: Option<u8>, _: HistoryOperation) -> bool {
        true
    }
}
#[derive(Debug)]
struct Boot;
impl TrustedBoot for Boot {
    fn current_boot(&self) -> Result<u64, JobError> {
        Ok(1)
    }
}
#[derive(Debug)]
struct Policy;
impl TrustedJobPolicy for Policy {
    fn supports(&self, op: u16) -> bool {
        matches!(op, 1 | 2)
    }
    fn current_revision(&self) -> Result<u16, JobError> {
        Ok(1)
    }
    fn permits(&self, p: [u8; 16], op: u16, r: u16) -> bool {
        p == [1; 16] && matches!(op, 1 | 2) && r == 1
    }
}
#[derive(Debug)]
struct Isolation;
impl ResetIsolation for Isolation {
    fn preserves_ledger(&self, op: u16) -> bool {
        matches!(op, 1 | 2)
    }
}
const CATALOGUE: [ModuleCatalogue<'static>; 1] = [ModuleCatalogue {
    module: 4,
    properties: &[PropertyDescriptor {
        key: 3,
        access: 11,
        schema: 1,
        value_max: 1,
    }],
}];
const CAPS: [ModuleCapabilities<'static>; 1] = [ModuleCapabilities {
    module: 4,
    capabilities: device_configuration::ProductCapabilities::NONE,
}];
#[cfg(feature = "heart-rate")]
const HEALTH: [device_configuration::HealthCapability; 1] =
    [device_configuration::HealthCapability {
        module: 0x20,
        interval_s: device_configuration::Range { min: 5, max: 600 },
        temperature_modes: 0,
    }];
fn shared_caps() -> device_configuration::ProductCapabilities<'static> {
    #[cfg(feature = "heart-rate")]
    return device_configuration::ProductCapabilities {
        health: Some(&HEALTH),
        ..device_configuration::ProductCapabilities::NONE
    };
    #[cfg(not(feature = "heart-rate"))]
    device_configuration::ProductCapabilities::NONE
}
fn shared_defaults() -> device_config_store::Defaults<'static> {
    #[cfg(feature = "heart-rate")]
    let heart = Some(device_configuration::HeartPolicy {
        enabled: true,
        mode: 1,
        interval_s: 30,
        rmssd_enabled: false,
    });
    #[cfg(not(feature = "heart-rate"))]
    let heart = None;
    #[cfg(all(feature = "heart-rate", feature = "sport"))]
    let zones = &[device_configuration::Zones {
        sport_type: 1,
        zone_type: 1,
        resting: 60,
        maximum: 190,
        thresholds: [100, 120, 140, 160, 180],
    }][..];
    #[cfg(not(all(feature = "heart-rate", feature = "sport")))]
    let zones = &[];
    device_config_store::Defaults { heart, zones }
}
fn filter() -> device_protocol::HistoryFilter {
    device_protocol::HistoryFilter {
        domain: 1,
        filter_flags: 0,
        start_utc_ms: 0,
        end_utc_ms: 0,
        record_type: 0,
        parent_session: 0,
    }
}
#[test]
fn test_should_recover_all_six_real_owner_families_independently_and_preserve_history_ack() {
    // Skipping any actual recovery, formatting on failure or losing ACK makes this fail.
    let media: [medium::Memory; 6] = core::array::from_fn(|_| medium::Memory::new());
    let mut config =
        device_config_store::ConfigStore::open(snap::<1024>(media[0].clone()), shared_caps())
            .unwrap();
    config.initialize(shared_defaults()).unwrap();
    let mut properties = PropertyStore::<_, _, 256>::open(
        snap::<256>(media[1].clone()),
        &CATALOGUE,
        CodecSchemas::new(&CAPS),
    )
    .unwrap();
    initialize_blank_settings(
        &properties,
        BlankSettings::Initialize(&[DefaultValue {
            module: 4,
            key: 3,
            value: &[0],
        }]),
    )
    .unwrap();
    let (mut objects, begin) = initialized_objects(media[2].clone());
    let mut history = HistoryStore::<_, _, _, 1, 4, 16, 1024>::initialize(
        snap::<1024>(media[3].clone()),
        Schema,
        Clock,
        [7; 16],
        &[DomainConfig {
            domain: 1,
            epoch: 3,
        }],
        10,
    )
    .unwrap();
    history
        .append(AppendRecord {
            domain: 1,
            record_type: 2,
            schema: 1,
            utc_ms: 100,
            flags: 1,
            parent_session: 1,
            payload: &[1, 11],
        })
        .unwrap();
    let lease = RecordStore::open(&mut history, 1, filter(), 100, &Access).unwrap();
    history.record_admitted(lease, 1, &Access).unwrap();
    history
        .ack_atomic(lease, filter(), 3, 1, 100, &Access)
        .unwrap();
    let mut time = TimeStore::open(snap::<128>(media[4].clone()), Clock, 20).unwrap();
    time.synchronize(1000, 2, &Access).unwrap();
    let mut jobs = JobStore::<_, _, _, _, 32, 2112>::open(
        snap::<2304>(media[5].clone()),
        Boot,
        Policy,
        Isolation,
    )
    .unwrap();
    jobs.initialize().unwrap();
    let claim = jobs
        .claim_atomic(
            NamedRequest {
                operation_id: [9; 16],
                operation: 1,
                params: &[1, 0],
            },
            &Access,
        )
        .unwrap();
    exercise_owner_recovery(
        &media,
        (
            &mut config,
            &mut properties,
            &mut objects,
            &mut history,
            &mut time,
            &mut jobs,
        ),
        begin,
        claim.record,
    );
    assert_eq!(config.revision(), Ok(1));
    assert_eq!(objects.revision(), Ok(3));
    assert_eq!(jobs.revision(), Ok(2));
}
#[test]
fn test_should_initialize_only_explicit_confirmed_blank_properties_and_never_format_corruption() {
    let memory = medium::Memory::new();
    let properties = PropertyStore::<_, _, 256>::open(
        snap::<256>(memory.clone()),
        &CATALOGUE,
        CodecSchemas::new(&CAPS),
    )
    .unwrap();
    initialize_blank_settings(&properties, BlankSettings::KeepBlank).unwrap();
    assert_eq!(memory.mutations(), 0);
    initialize_blank_settings(
        &properties,
        BlankSettings::Initialize(&[DefaultValue {
            module: 4,
            key: 3,
            value: &[0],
        }]),
    )
    .unwrap();
    let before = memory.mutations();
    assert!(
        initialize_blank_settings(
            &properties,
            BlankSettings::Initialize(&[DefaultValue {
                module: 4,
                key: 3,
                value: &[1]
            }])
        )
        .is_err()
    );
    assert_eq!(memory.mutations(), before);
    drop(properties);
    memory.corrupt(0);
    let before = memory.mutations();
    assert!(
        AtomicSnapshot::<_, 256>::open(
            memory.clone(),
            Layout {
                start: 0,
                bank_bytes: 4096
            }
        )
        .is_err()
    );
    assert_eq!(memory.mutations(), before);
}

type SharedConfig = device_config_store::ConfigStore<'static, AtomicSnapshot<medium::Memory, 1024>>;
type Properties =
    PropertyStore<'static, AtomicSnapshot<medium::Memory, 256>, CodecSchemas<'static>, 256>;
type Objects =
    device_object_store::DurableObjects<AtomicSnapshot<medium::Memory, 512>, Schema, 1, 4, 16, 512>;
type History = HistoryStore<AtomicSnapshot<medium::Memory, 1024>, Schema, Clock, 1, 4, 16, 1024>;
type Time = TimeStore<AtomicSnapshot<medium::Memory, 128>, Clock>;
type Jobs = JobStore<AtomicSnapshot<medium::Memory, 2304>, Boot, Policy, Isolation, 32, 2112>;
fn exercise_owner_recovery(
    media: &[medium::Memory; 6],
    owners: (
        &mut SharedConfig,
        &mut Properties,
        &mut Objects,
        &mut History,
        &mut Time,
        &mut Jobs,
    ),
    begin: Begin,
    claim: DeviceJobRecord,
) {
    let (config, properties, objects, history, time, jobs) = owners;
    for failed in 0..6 {
        let mutations: [usize; 6] = core::array::from_fn(|i| media[i].mutations());
        media[failed].fail_reads(true);
        let report = SelectedWarehouse::new((
            &mut *config,
            &mut *properties,
            &mut *objects,
            &mut *history,
            &mut *time,
            &mut *jobs,
        ))
        .recover_after_shutdown();
        let outcomes = [
            report.shared_config,
            report.properties,
            report.objects,
            report.history_ack,
            report.time,
            report.jobs,
        ];
        for (i, outcome) in outcomes.into_iter().enumerate() {
            assert_eq!(outcome.is_err(), i == failed, "owner {i}, failed {failed}");
            assert_eq!(media[i].mutations(), mutations[i]);
        }
        media[failed].disarm();
        let report = SelectedWarehouse::new((
            &mut *config,
            &mut *properties,
            &mut *objects,
            &mut *history,
            &mut *time,
            &mut *jobs,
        ))
        .boot_existing();
        assert!(report.all_recovered());
        #[cfg(feature = "heart-rate")]
        {
            use device_services::PropertyBackend;
            let mut bytes = [0; 32];
            let n = config
                .adapter(0x20, None)
                .unwrap()
                .read(1, &mut bytes)
                .unwrap()
                .length;
            assert_eq!(&bytes[..n], &[1, 1, 30, 0, 0, 0, 0]);
        }
        #[cfg(all(feature = "heart-rate", feature = "sport"))]
        {
            use device_services::PropertyBackend;
            let mut bytes = [0; 32];
            let n = config
                .adapter(0x22, Some(1))
                .unwrap()
                .read(2, &mut bytes)
                .unwrap()
                .length;
            assert_eq!(
                &bytes[..n],
                &[
                    1, 0, 1, 60, 0, 190, 0, 100, 0, 120, 0, 140, 0, 160, 0, 180, 0
                ]
            );
        }
        assert_eq!(objects.reserve(&begin, &Access), Err(StoreError::Conflict));
        assert_eq!(jobs.query([9; 16], &Access).unwrap(), claim);
        let lease = RecordStore::open(&mut *history, 2, filter(), 100, &Access).unwrap();
        assert_eq!(lease.committed_id, 1);
        history.close(lease).unwrap();
    }
}

fn initialized_objects(memory: medium::Memory) -> (Objects, Begin) {
    let mut objects = device_object_store::DurableObjects::<_, _, 1, 4, 16, 512>::open(
        snap::<512>(memory),
        Schema,
    )
    .unwrap();
    objects.initialize(&[1]).unwrap();
    let begin = Begin {
        module: 1,
        kind: 7,
        object_id: 3,
        expected_revision: 0,
        total: 1,
        create: true,
        creation_token: [9; 16],
    };
    objects.reserve(&begin, &Access).unwrap();
    objects
        .commit(
            &Commit {
                begin,
                crc: device_protocol::object_crc(1, 7, &[42]).unwrap(),
            },
            &[42],
            &Access,
        )
        .unwrap();

    (objects, begin)
}
