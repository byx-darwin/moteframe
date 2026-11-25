//! Exact private durable synchronization image.
use crate::{ClockReading, Error, SyncEvidence};
use device_storage::persistence::SnapshotStore;
const LENGTH: usize = 64;
const MAGIC: &[u8; 8] = b"DTIME001";
pub(super) fn encode(item: SyncEvidence) -> [u8; LENGTH] {
    let mut bytes = [0; LENGTH];
    bytes[..8].copy_from_slice(MAGIC);
    bytes[8..16].copy_from_slice(&item.revision.to_le_bytes());
    bytes[16..24].copy_from_slice(&item.reading.source_id.to_le_bytes());
    bytes[24..32].copy_from_slice(&item.reading.epoch.to_le_bytes());
    bytes[32..40].copy_from_slice(&item.reading.mono_us.to_le_bytes());
    bytes[40..48].copy_from_slice(&item.utc_ms.to_le_bytes());
    bytes[48..52].copy_from_slice(&item.uncertainty_ms.to_le_bytes());
    bytes[52..56].copy_from_slice(&item.max_drift_ppm.to_le_bytes());
    bytes
}
pub(super) fn load<S: SnapshotStore>(store: &S) -> Result<Option<SyncEvidence>, Error> {
    let mut bytes = [0; LENGTH];
    let Some(snapshot) = store.load(&mut bytes).map_err(Error::Storage)? else {
        return Ok(None);
    };
    if snapshot.length != LENGTH
        || snapshot.revision == 0
        || &bytes[..8] != MAGIC
        || bytes[56..] != [0; 8]
    {
        return Err(Error::Integrity);
    }
    let revision = u64_at(&bytes, 8);
    let source_id = u64_at(&bytes, 16);
    let epoch = u64_at(&bytes, 24);
    if revision != snapshot.revision || source_id == 0 || epoch == 0 {
        return Err(Error::Integrity);
    }
    Ok(Some(SyncEvidence {
        revision,
        reading: ClockReading {
            source_id,
            epoch,
            mono_us: u64_at(&bytes, 32),
        },
        utc_ms: i64::from_le_bytes(bytes[40..48].try_into().map_err(|_| Error::Integrity)?),
        uncertainty_ms: u32::from_le_bytes(bytes[48..52].try_into().map_err(|_| Error::Integrity)?),
        max_drift_ppm: u32::from_le_bytes(bytes[52..56].try_into().map_err(|_| Error::Integrity)?),
    }))
}
fn u64_at(bytes: &[u8; LENGTH], at: usize) -> u64 {
    bytes[at..at + 8]
        .iter()
        .enumerate()
        .fold(0, |value, (shift, byte)| {
            value | (u64::from(*byte) << (shift * 8))
        })
}
