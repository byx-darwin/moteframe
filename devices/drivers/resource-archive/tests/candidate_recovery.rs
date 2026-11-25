//! Full generated-image interoperability with the actual asynchronous owner.
//!
//! Explicitly ignored in ordinary unit runs: requires the pinned, read-only
//! capture pair and caller's lab config under target/. Run with --ignored.
#![allow(
    clippy::unwrap_used,
    reason = "Fixture setup and assertions must fail loudly."
)]
use core::{cell::Cell, task::Poll};
use device_history_service::HistoryFailure;
use device_history_store::{
    HistoryClock, HistorySchema,
    async_paged::{AsyncPagedHistoryStore, JobResult},
    paged::LogConfig,
};
use device_storage::{
    StoredRecord,
    incremental::{AsyncMedium, Completion, IncrementalSnapshot, MediumOp, StartError},
    partitions::{AsyncPartitions, Range},
    persistence::{Error, Geometry, Layout},
};
use std::{path::PathBuf, process::Command, rc::Rc};

#[derive(Debug)]
enum ReadOp {
    Read { at: usize, length: usize },
    Sync,
}
#[derive(Debug)]
struct ReadModel {
    bytes: Vec<u8>,
    pending: Option<ReadOp>,
    delay: bool,
    calls: Rc<Cell<usize>>,
}
impl AsyncMedium for ReadModel {
    fn geometry(&self) -> Geometry {
        Geometry {
            capacity: u32::try_from(self.bytes.len()).unwrap(),
            write_size: 256,
            erase_size: 4096,
        }
    }
    fn begin(&mut self, operation: MediumOp<'_>) -> Result<(), StartError> {
        self.calls.set(self.calls.get() + 1);
        if self.pending.is_some() {
            return Err(StartError::Busy);
        }
        let pending = match operation {
            MediumOp::Read { offset, length } => {
                let at = usize::try_from(offset).map_err(|_| StartError::Io)?;
                assert!((1..=256).contains(&length));
                assert!((0x0020_0000..0x0038_7000).contains(&offset));
                if at
                    .checked_add(length)
                    .is_none_or(|end| end > self.bytes.len())
                {
                    return Err(StartError::Io);
                }
                ReadOp::Read { at, length }
            }
            MediumOp::Sync => ReadOp::Sync,
            MediumOp::Program { .. } | MediumOp::Erase { .. } => return Err(StartError::Io),
        };
        self.pending = Some(pending);
        self.delay = true;
        Ok(())
    }
    fn poll(&mut self, out: &mut [u8]) -> Poll<Result<Completion, Error>> {
        self.calls.set(self.calls.get() + 1);
        if self.delay {
            self.delay = false;
            return Poll::Pending;
        }
        let Some(operation) = self.pending.take() else {
            return Poll::Ready(Err(Error::Conflict));
        };
        Poll::Ready(match operation {
            ReadOp::Read { at, length } => {
                out[..length].copy_from_slice(&self.bytes[at..at + length]);
                Ok(Completion::Read(length))
            }
            ReadOp::Sync => Ok(Completion::Synced),
        })
    }
}
#[derive(Debug)]
struct NoObservation;
impl HistoryClock for NoObservation {
    fn now_ms(&self) -> Result<u64, HistoryFailure> {
        Err(HistoryFailure::Clock)
    }
}
#[derive(Debug)]
struct NoRecordSchema;
impl HistorySchema for NoRecordSchema {
    fn validate(&self, _record: StoredRecord<'_>, _parent: u64) -> Result<(), HistoryFailure> {
        Err(HistoryFailure::Invalid)
    }
}
fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}
fn namespace() -> [u8; 16] {
    let output = Command::new("python3")
        .args([
            "-c",
            "import json,sys;print(json.load(open(sys.argv[1]))['namespace'])",
        ])
        .arg(root().join("target/wattch5-integrated-candidate/candidate-lab-config.json"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = String::from_utf8(output.stdout).unwrap();
    let text = text.trim();
    assert_eq!(text.len(), 32);
    core::array::from_fn(|n| u8::from_str_radix(&text[n * 2..n * 2 + 2], 16).unwrap())
}
fn recover(image: Vec<u8>) -> Result<JobResult, HistoryFailure> {
    assert_eq!(image.len(), 0x0040_0000);
    let calls = Rc::new(Cell::new(0));
    let model = ReadModel {
        bytes: image,
        pending: None,
        delay: false,
        calls: calls.clone(),
    };
    let mut broker = AsyncPartitions::new_unrecovered(
        model,
        [
            Range {
                start: 0x0020_0000,
                length: 0x2000,
                erase: true,
            },
            Range {
                start: 0x0020_2000,
                length: 0x0018_5000,
                erase: true,
            },
        ],
    )
    .unwrap();
    let (mut coordinator, [metadata, log]) = broker.split().unwrap();
    loop {
        match coordinator.recover_step() {
            Poll::Pending => {}
            Poll::Ready(result) => {
                result.unwrap();
                break;
            }
        }
    }
    let snapshot = IncrementalSnapshot::new_unrecovered(
        metadata,
        Layout {
            start: 0,
            bank_bytes: 4096,
        },
    )
    .unwrap();
    let store: AsyncPagedHistoryStore<_, _, _, _, 6, 255, 474, 464> =
        AsyncPagedHistoryStore::new_unrecovered(
            snapshot,
            log,
            NoRecordSchema,
            NoObservation,
            LogConfig {
                namespace: namespace(),
                log_start: 0x0020_2000,
                slots_per_domain: 259,
            },
            60,
        )
        .unwrap();
    let token = store.begin_recover().unwrap();
    for step in 0..100_000 {
        if let Poll::Ready(result) = store.result(token).unwrap() {
            if result == Ok(JobResult::Recovered { initialized: true }) {
                let stats = store.stats().unwrap();
                assert_eq!(stats.retained, [0; 6]);
                assert_eq!(stats.consumed, [0; 6]);
                assert_eq!(stats.slots_per_domain, 259);
                assert_eq!(stats.slot_bytes, 1024);
                println!(
                    "actual AsyncPagedHistoryStore recovered initialized=true D6/R255/B474/N464; absolute_log_start=0x202000; log_capacity=0x185000; steps={step}; calls={}",
                    calls.get()
                );
            }
            return result;
        }
        let before = calls.get();
        store.step().unwrap();
        assert!(calls.get() - before <= 1, "unbounded step");
    }
    panic!("actual history recovery did not terminate");
}
#[test]
#[ignore = "requires pinned offline captures and explicit lab config; no device access"]
fn test_should_recover_generated_candidate_with_actual_async_owner() {
    let output = root()
        .join("target/wattch5-integrated-candidate/task3/cross-integration")
        .join(format!(
            "generated-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
    let result = Command::new("python3")
        .current_dir(root())
        .args([
            "scripts/candidate_external_image.py",
            "build",
            "--source",
            "target/resource-flash/capture-04/pass-1.bin",
            "--duplicate-source",
            "target/resource-flash/capture-04/pass-2.bin",
            "--config",
            "target/wattch5-integrated-candidate/candidate-lab-config.json",
            "--output",
        ])
        .arg(&output)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        recover(std::fs::read(output.join("external-candidate.bin")).unwrap()),
        Ok(JobResult::Recovered { initialized: true })
    );
}
#[test]
#[ignore = "requires preserved incompatible first candidate; verifies fail-closed behavior"]
fn test_should_reject_preserved_v1_zero_log_start_binding() {
    let image = root()
        .join("target/wattch5-integrated-candidate/external-candidate/external-candidate.bin");
    assert_eq!(
        recover(std::fs::read(image).unwrap()),
        Err(HistoryFailure::StoredIntegrity)
    );
}

#[test]
#[ignore = "requires generated v2 package and explicit lab config; no device access"]
fn test_should_recover_packaged_v2_with_actual_async_owner() {
    let image = root()
        .join("target/wattch5-integrated-candidate/external-candidate-v2/external-candidate.bin");
    assert_eq!(
        recover(std::fs::read(image).unwrap()),
        Ok(JobResult::Recovered { initialized: true })
    );
}
