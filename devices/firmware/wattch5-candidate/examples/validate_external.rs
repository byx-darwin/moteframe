//! Host-only offline recovery of the exact provisioned image through actual owners.
use device_configuration::ProductCapabilities;
use device_history_service::{HistoryFailure, RecordStore};
use device_history_store::{
    HistoryClock,
    async_paged::{AsyncPagedHistoryStore, JobResult},
    paged::LogConfig,
};
use device_property_store::{
    AsyncPropertyStore, CodecSchemas, ModuleCapabilities, ModuleCatalogue,
};
use device_services::PropertyDescriptor;
use device_storage::{
    incremental::{AsyncMedium, Completion, IncrementalSnapshot, MediumOp, StartError},
    partitions::{AsyncPartitions, Range},
    persistence::{Error as StorageError, Geometry, Layout},
};
use std::{cell::RefCell, error::Error, fmt::Debug, path::PathBuf, rc::Rc, task::Poll};
use wattch5_candidate::{boot, schema::StrictSchema};
#[derive(Debug)]
struct Disk {
    visible: Vec<u8>,
    durable: Vec<u8>,
    calls: u64,
}
#[derive(Debug)]
enum Operation {
    Read(u32, usize),
    Program(u32, Vec<u8>),
    Erase(u32),
    Sync,
}
#[derive(Debug)]
struct Medium {
    disk: Rc<RefCell<Disk>>,
    pending: Option<Operation>,
    delay: u8,
}
impl Medium {
    fn new(disk: &Rc<RefCell<Disk>>) -> Self {
        Self {
            disk: disk.clone(),
            pending: None,
            delay: 0,
        }
    }
}
impl AsyncMedium for Medium {
    fn geometry(&self) -> Geometry {
        Geometry {
            capacity: 0x400000,
            write_size: 256,
            erase_size: 4096,
        }
    }
    fn begin(&mut self, op: MediumOp<'_>) -> Result<(), StartError> {
        if self.pending.is_some() {
            return Err(StartError::Busy);
        }
        self.disk.borrow_mut().calls += 1;
        self.pending = Some(match op {
            MediumOp::Read { offset, length } => Operation::Read(offset, length),
            MediumOp::Program { offset, bytes } => Operation::Program(offset, bytes.to_vec()),
            MediumOp::Erase { offset } => Operation::Erase(offset),
            MediumOp::Sync => Operation::Sync,
        });
        self.delay = 2;
        Ok(())
    }
    fn poll(&mut self, out: &mut [u8]) -> Poll<Result<Completion, StorageError>> {
        self.disk.borrow_mut().calls += 1;
        if self.delay > 0 {
            self.delay -= 1;
            return Poll::Pending;
        }
        let Some(op) = self.pending.take() else {
            return Poll::Ready(Err(StorageError::Conflict));
        };
        let mut disk = self.disk.borrow_mut();
        let result = (|| match op {
            Operation::Read(at, n) => {
                if n == 0 || n > 256 || out.len() < n {
                    return Err(StorageError::Capacity);
                }
                let bytes = disk
                    .visible
                    .get(at as usize..at as usize + n)
                    .ok_or(StorageError::Io)?;
                out[..n].copy_from_slice(bytes);
                Ok(Completion::Read(n))
            }
            Operation::Program(at, bytes) => {
                if !at.is_multiple_of(256) || bytes.len() != 256 {
                    return Err(StorageError::InvalidLayout);
                }
                let dest = disk
                    .visible
                    .get_mut(at as usize..at as usize + bytes.len())
                    .ok_or(StorageError::Io)?;
                if dest.iter().zip(&bytes).any(|(old, new)| old & new != *new) {
                    return Err(StorageError::Io);
                }
                for (a, b) in dest.iter_mut().zip(bytes) {
                    *a &= b;
                }
                Ok(Completion::Programmed)
            }
            Operation::Erase(at) => {
                if !at.is_multiple_of(4096) {
                    return Err(StorageError::InvalidLayout);
                }
                disk.visible
                    .get_mut(at as usize..at as usize + 4096)
                    .ok_or(StorageError::Io)?
                    .fill(255);
                Ok(Completion::Erased)
            }
            Operation::Sync => {
                let visible = disk.visible.clone();
                disk.durable = visible;
                Ok(Completion::Synced)
            }
        })();
        Poll::Ready(result)
    }
}
#[derive(Debug)]
struct Clock;
impl HistoryClock for Clock {
    fn now_ms(&self) -> Result<u64, HistoryFailure> {
        Ok(100)
    }
}
fn checked<T, E: Debug>(value: Result<T, E>) -> Result<T, Box<dyn Error>> {
    value.map_err(|e| format!("{e:?}").into())
}
fn identity(raw: &str) -> Result<[u8; 16], Box<dyn Error>> {
    if raw.len() != 32 || !raw.is_ascii() {
        return Err("identity must have32hexbytes".into());
    }
    let mut out = [0; 16];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&raw[i * 2..i * 2 + 2], 16)?;
    }
    if out == [0; 16] {
        return Err("zero identity".into());
    }
    Ok(out)
}
fn recover<M: AsyncMedium, const N: usize>(
    snapshot: &mut IncrementalSnapshot<M, N>,
) -> Result<(), Box<dyn Error>> {
    checked(snapshot.begin_recover())?;
    for _ in 0..2_000_000 {
        if let Poll::Ready(r) = snapshot.poll_recover() {
            checked(r)?;
            return Ok(());
        }
    }
    Err("recovery did not terminate".into())
}
const LEVELS: [u16; 5] = [100, 200, 400, 700, 1000];
const LANGS: [&str; 2] = ["zh-CN", "en-US"];
const FACTS: [ModuleCapabilities<'static>; 1] = [ModuleCapabilities {
    module: 4,
    capabilities: ProductCapabilities {
        languages: Some(&LANGS),
        normal_levels: Some(&LEVELS),
        ..ProductCapabilities::NONE
    },
}];
const DESC: [PropertyDescriptor; 3] = [
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
        value_max: 7,
    },
    PropertyDescriptor {
        key: 10,
        access: 11,
        schema: 1,
        value_max: 2,
    },
];
const CATALOGUE: [ModuleCatalogue<'static>; 1] = [ModuleCatalogue {
    module: 4,
    properties: &DESC,
}];
fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len()
        != if cfg!(feature = "native-health") {
            6
        } else {
            5
        }
    {
        return Err(
            "usage: validate_external IMAGE NAMESPACE DATASET PROVISION EPOCHS_COMMA".into(),
        );
    }
    let path = PathBuf::from(&args[0]);
    let original = std::fs::read(path)?;
    if original.len() != 0x400000 {
        return Err("full4MiB image required".into());
    }
    let namespace = identity(&args[1])?;
    let dataset = identity(&args[2])?;
    let provision = identity(&args[3])?;
    let epochs: Vec<u32> = args[4]
        .split(',')
        .map(str::parse)
        .collect::<Result<_, _>>()?;
    if epochs.len() != 6 || epochs.contains(&0) {
        return Err("six nonzero epochs required".into());
    }
    #[cfg(feature = "native-health")]
    validate_health(&original, namespace, dataset, &args[5])?;
    let disk = Rc::new(RefCell::new(Disk {
        visible: original.clone(),
        durable: original.clone(),
        calls: 0,
    }));
    {
        let mut broker = checked(AsyncPartitions::new_unrecovered(
            Medium::new(&disk),
            [
                Range {
                    start: 0x200000,
                    length: 0x2000,
                    erase: true,
                },
                Range {
                    start: 0x202000,
                    length: 0x185000,
                    erase: false,
                },
                Range {
                    start: 0x389000,
                    length: 0x4000,
                    erase: true,
                },
                Range {
                    start: 0x399000,
                    length: 0x2000,
                    erase: true,
                },
            ],
        ))?;
        let (mut coordinator, [meta, log, properties, boot]) = checked(broker.split())?;
        for _ in 0..100 {
            if let Poll::Ready(result) = coordinator.recover_step() {
                checked(result)?;
                break;
            }
        }
        let mut boot = checked(IncrementalSnapshot::<_, 64>::new_unrecovered(
            boot,
            Layout {
                start: 0,
                bank_bytes: 4096,
            },
        ))?;
        recover(&mut boot)?;
        let mut properties = checked(IncrementalSnapshot::<_, 128>::new_unrecovered(
            properties,
            Layout {
                start: 0,
                bank_bytes: 8192,
            },
        ))?;
        recover(&mut properties)?;
        let properties = checked(AsyncPropertyStore::open(
            properties,
            &CATALOGUE,
            CodecSchemas::new(&FACTS),
        ))?;
        if checked(properties.revision())? != 1 {
            return Err("initial properties revision differs".into());
        }
        let history = checked(
            AsyncPagedHistoryStore::<_, _, _, _, 6, 255, 474, 464>::new_unrecovered(
                checked(IncrementalSnapshot::new_unrecovered(
                    meta,
                    Layout {
                        start: 0,
                        bank_bytes: 4096,
                    },
                ))?,
                log,
                StrictSchema,
                Clock,
                LogConfig {
                    namespace,
                    log_start: 0x202000,
                    slots_per_domain: 259,
                },
                300,
            ),
        )?;
        let token = checked(history.begin_recover())?;
        let mut complete = false;
        for _ in 0..2_000_000 {
            let before = disk.borrow().calls;
            checked(history.step())?;
            if disk.borrow().calls - before > 1 {
                return Err("history exceeded one medium operation/slice".into());
            }
            if let Poll::Ready(result) = checked(history.result(token))? {
                if checked(result)? != (JobResult::Recovered { initialized: true }) {
                    return Err("history not initialized".into());
                }
                complete = true;
                break;
            }
        }
        if !complete {
            return Err("history recovery stalled".into());
        }
        let adapter = checked(history.adapter())?;
        let directory = checked(adapter.directory())?;
        if directory.dataset != dataset || directory.domains.len() != 6 {
            return Err("foreign directory".into());
        }
        for (i, domain) in directory.domains.iter().enumerate() {
            if domain.domain != [0, 1, 2, 5, 6, 7][i]
                || domain.epoch != epochs[i]
                || domain.oldest_id != 0
                || domain.newest_id != 0
            {
                return Err("foreign/nonempty domain".into());
            }
        }
        if checked(history.stats())?.retained != [0; 6] {
            return Err("initial history not empty".into());
        }
        checked(coordinator.attest_normal())?;
        let mut payload = [0; 64];
        let snapshot = checked(boot.load_committed(&mut payload))?.ok_or("missing boot journal")?;
        let (next, counter) = checked(boot::advance(snapshot, &payload, provision, namespace))?;
        if counter != 1 {
            return Err("candidate must begin atcounter0".into());
        }
        let commit = checked(boot.begin_commit(snapshot.revision, &next))?;
        let mut complete = false;
        for _ in 0..100000 {
            if let Poll::Ready(result) = boot.poll_commit(commit) {
                let receipt = checked(result)?;
                if receipt.snapshot.revision != 2 || receipt.snapshot.length != 64 {
                    return Err("wrong boot receipt".into());
                }
                complete = true;
                break;
            }
        }
        if !complete {
            return Err("boot commit stalled".into());
        }
    }
    // All original owners/handles are out of scope before a new recovery owner is made.
    {
        let mut broker = checked(AsyncPartitions::new_unrecovered(
            Medium::new(&disk),
            [Range {
                start: 0x399000,
                length: 0x2000,
                erase: true,
            }],
        ))?;
        let (mut c, [boot]) = checked(broker.split())?;
        for _ in 0..100 {
            if let Poll::Ready(result) = c.recover_step() {
                checked(result)?;
                break;
            }
        }
        let mut boot = checked(IncrementalSnapshot::<_, 64>::new_unrecovered(
            boot,
            Layout {
                start: 0,
                bank_bytes: 4096,
            },
        ))?;
        recover(&mut boot)?;
        let mut payload = [0; 64];
        let snapshot = checked(boot.load_committed(&mut payload))?.ok_or("boot disappeared")?;
        if snapshot.revision != 2 || u64::from_le_bytes(payload[40..48].try_into()?) != 1 {
            return Err("boot notdurable".into());
        }
    }
    let disk = disk.borrow();
    if disk.visible != disk.durable
        || disk.durable[..0x399000] != original[..0x399000]
        || disk.durable[0x39b000..] != original[0x39b000..]
    {
        return Err("unexpected nonboot mutation".into());
    }
    println!(
        "{{\"model_only\":true,\"health_recovered\":{},\"owners_recovered\":[\"history\",\"properties\",\"boot_identity\"],\"boot_counter_before\":0,\"boot_counter_after\":1,\"fresh_recovery_confirmed\":true,\"lower_2mib_unchanged\":true,\"physical_qualification\":false}}",
        cfg!(feature = "native-health")
    );
    Ok(())
}

#[cfg(feature = "native-health")]
fn validate_health(
    image: &[u8],
    namespace: [u8; 16],
    dataset: [u8; 16],
    raw_epochs: &str,
) -> Result<(), Box<dyn Error>> {
    use wattch5_product::runtime::legacy_health::{HealthHistory, HealthLog, HealthSchema};
    let epochs: Vec<u32> = raw_epochs
        .split(',')
        .map(str::parse)
        .collect::<Result<_, _>>()?;
    if epochs.len() != 3 || epochs.contains(&0) {
        return Err("three nonzero health epochs required".into());
    }
    let disk = Rc::new(RefCell::new(Disk {
        visible: image.to_vec(),
        durable: image.to_vec(),
        calls: 0,
    }));
    let mut broker = checked(AsyncPartitions::new_unrecovered(
        Medium::new(&disk),
        [
            Range {
                start: 0x39b000,
                length: 8192,
                erase: true,
            },
            Range {
                start: 0x39d000,
                length: 0x62000,
                erase: false,
            },
        ],
    ))?;
    let (mut coordinator, [meta, log]) = checked(broker.split())?;
    loop {
        if let Poll::Ready(result) = coordinator.recover_step() {
            checked(result)?;
            break;
        }
    }
    let owner = checked(HealthHistory::new_unrecovered(
        checked(IncrementalSnapshot::new_unrecovered(
            meta,
            Layout {
                start: 0,
                bank_bytes: 4096,
            },
        ))?,
        log,
        HealthSchema,
        Clock,
        LogConfig {
            namespace,
            log_start: 0x39d000,
            slots_per_domain: 259,
        },
        300,
    ))?;
    let token = checked(owner.begin_recover())?;
    let mut recovered = false;
    for _ in 0..100_000 {
        checked(owner.step())?;
        if let Poll::Ready(result) = checked(owner.result(token))? {
            if checked(result)? != (JobResult::Recovered { initialized: true }) {
                return Err("health must already be initialized".into());
            }
            recovered = true;
            break;
        }
    }
    if !recovered {
        return Err("health recovery did not complete".into());
    }
    let reader = checked(owner.adapter())?;
    let directory = checked(reader.directory())?;
    if directory.dataset != dataset || directory.domains.len() != 3 {
        return Err("foreign health dataset".into());
    }
    for (n, d) in directory.domains.iter().enumerate() {
        if d.domain != [3, 4, 8][n] || d.epoch != epochs[n] {
            return Err("foreign health domain/epoch".into());
        }
    }
    checked(HealthLog::restore(&owner, || Ok(100), || {}))?;
    if disk.borrow().visible != image || disk.borrow().durable != image {
        return Err("health recovery mutated image".into());
    }
    Ok(())
}
