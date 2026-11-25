//! Canonical v1 bytes and semantic validation; no hardware or execution effects.
use device_maintenance_service::{
    DeviceJobRecord, DeviceJobState as State, DeviceJobTerminal, Error,
};
use device_storage::persistence::SnapshotStore;

const HEADER: usize = 64;
const ENTRY: usize = 64;
const MAGIC: &[u8; 8] = b"DJOB0001";
pub(super) fn length<const R: usize, const N: usize>() -> Result<usize, Error> {
    let length = R
        .checked_mul(ENTRY)
        .and_then(|n| n.checked_add(HEADER))
        .ok_or(Error::Capacity)?;
    if !(32..=usize::from(u16::MAX)).contains(&R) || length > N {
        return Err(Error::Capacity);
    }
    Ok(length)
}
pub(super) fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}
fn u64_at(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes([
        bytes[at],
        bytes[at + 1],
        bytes[at + 2],
        bytes[at + 3],
        bytes[at + 4],
        bytes[at + 5],
        bytes[at + 6],
        bytes[at + 7],
    ])
}
pub(super) fn put16(bytes: &mut [u8], at: usize, value: u16) {
    bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
}
fn put32(bytes: &mut [u8], at: usize, value: u32) {
    bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn put64(bytes: &mut [u8], at: usize, value: u64) {
    bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
pub(super) fn revision(bytes: &[u8]) -> u64 {
    u64_at(bytes, 16)
}
pub(super) fn boot(bytes: &[u8]) -> u64 {
    u64_at(bytes, 24)
}
pub(super) fn count(bytes: &[u8]) -> usize {
    usize::from(u16_at(bytes, 14))
}
pub(super) fn row(bytes: &[u8], index: usize) -> &[u8] {
    &bytes[HEADER + index * ENTRY..HEADER + (index + 1) * ENTRY]
}
fn row_mut(bytes: &mut [u8], index: usize) -> &mut [u8] {
    &mut bytes[HEADER + index * ENTRY..HEADER + (index + 1) * ENTRY]
}
pub(super) fn load<S: SnapshotStore, const R: usize, const N: usize>(
    store: &S,
    out: &mut [u8; N],
) -> Result<bool, Error> {
    let Some(snapshot) = store.load(out).map_err(|_| Error::Storage)? else {
        return Ok(false);
    };
    if snapshot.length != length::<R, N>()? {
        return Err(Error::Storage);
    }
    validate::<R, N>(out, snapshot.revision)?;
    Ok(true)
}
pub(super) fn initialize<const R: usize, const N: usize>(
    bytes: &mut [u8; N],
    boot: u64,
) -> Result<(), Error> {
    bytes.fill(0);
    bytes[..8].copy_from_slice(MAGIC);
    put16(bytes, 8, 1);
    put16(bytes, 10, 64);
    put16(bytes, 12, u16::try_from(R).map_err(|_| Error::Capacity)?);
    put64(bytes, 16, 1);
    put64(bytes, 24, boot);
    put64(bytes, 32, boot);
    put16(bytes, 52, 64);
    Ok(())
}
pub(super) fn record(row: &[u8]) -> Result<DeviceJobRecord, Error> {
    let mut operation_id = [0; 16];
    operation_id.copy_from_slice(&row[..16]);
    let mut principal = [0; 16];
    principal.copy_from_slice(&row[16..32]);
    let state = match row[41] {
        1 => State::Accepted,
        2 => State::Running,
        3 => State::Succeeded,
        4 => State::Failed,
        5 => State::Interrupted,
        _ => return Err(Error::Storage),
    };
    Ok(DeviceJobRecord {
        operation_id,
        principal,
        job: u32_at(row, 32),
        operation: u16_at(row, 36),
        policy_revision: u16_at(row, 38),
        state,
        result: u16_at(row, 46),
        reason: u16_at(row, 48),
    })
}
pub(super) fn binding(a: DeviceJobRecord, b: DeviceJobRecord) -> bool {
    a.operation_id == b.operation_id
        && a.principal == b.principal
        && a.job == b.job
        && a.operation == b.operation
        && a.policy_revision == b.policy_revision
}
pub(super) fn valid_terminal(t: DeviceJobTerminal) -> bool {
    match t.state {
        State::Succeeded => t.result == 0 && t.reason == 0,
        State::Failed | State::Interrupted => t.result != 0 && t.result <= 0x12 && t.result != 0x10,
        State::Accepted | State::Running => false,
    }
}
fn count_epoch_transition(previous: &mut u64, count: &mut u64, epoch: u64) -> Result<(), Error> {
    if epoch < *previous {
        return Err(Error::Storage);
    }
    if epoch > *previous {
        *count = count.checked_add(1).ok_or(Error::Storage)?;
    }
    *previous = epoch;
    Ok(())
}
pub(super) fn validate<const R: usize, const N: usize>(
    bytes: &[u8; N],
    revision: u64,
) -> Result<(), Error> {
    let length = length::<R, N>()?;
    let count = count(bytes);
    let current = boot(bytes);
    let initial = u64_at(bytes, 32);
    let updates = u64_at(bytes, 40);
    if &bytes[..8] != MAGIC
        || u16_at(bytes, 8) != 1
        || u16_at(bytes, 10) != 64
        || usize::from(u16_at(bytes, 12)) != R
        || count > R
        || revision == 0
        || self::revision(bytes) != revision
        || current == 0
        || initial == 0
        || current < initial
        || updates > current - initial
        || (updates == 0) != (current == initial)
        || u32_at(bytes, 48) != u32::try_from(count).map_err(|_| Error::Storage)?
        || u16_at(bytes, 52) != 64
        || bytes[54..64] != [0; 10]
        || bytes[HEADER + count * ENTRY..length]
            .iter()
            .any(|&b| b != 0)
        || bytes[length..].iter().any(|&b| b != 0)
    {
        return Err(Error::Storage);
    }
    let mut aggregate = 1_u64
        .checked_add(u64::try_from(count).map_err(|_| Error::Storage)?)
        .and_then(|v| v.checked_add(updates))
        .ok_or(Error::Storage)?;
    let mut previous_admission = initial;
    let mut minimum_updates = 0_u64;
    for index in 0..count {
        let row = row(bytes, index);
        let r = record(row)?;
        let admission = u64_at(row, 52);
        let flags = row[60];
        if r.operation_id == [0; 16]
            || r.principal == [0; 16]
            || r.job != u32::try_from(index + 1).map_err(|_| Error::Storage)?
            || !matches!(r.operation, 1 | 2)
            || r.policy_revision == 0
            || row[40] != 2
            || u16_at(row, 42) != 1
            || u16_at(row, 44) != 2
            || row[50..52] != [1, 0]
            || admission < previous_admission
            || admission > current
            || flags > 3
            || flags == 2
            || row[61..64] != [0; 3]
        {
            return Err(Error::Storage);
        }
        count_epoch_transition(&mut previous_admission, &mut minimum_updates, admission)?;
        let valid = match r.state {
            State::Accepted => flags == 0 && admission == current && r.result == 0 && r.reason == 0,
            State::Running => flags == 1 && admission == current && r.result == 0 && r.reason == 0,
            State::Succeeded | State::Failed => {
                flags == 3
                    && valid_terminal(DeviceJobTerminal {
                        state: r.state,
                        result: r.result,
                        reason: r.reason,
                    })
            }
            State::Interrupted => {
                if flags == 3 {
                    valid_terminal(DeviceJobTerminal {
                        state: r.state,
                        result: r.result,
                        reason: r.reason,
                    })
                } else {
                    admission < current && r.result == 4 && r.reason == 0xff06
                }
            }
        };
        if !valid {
            return Err(Error::Storage);
        }
        aggregate = aggregate
            .checked_add(u64::from(flags & 1))
            .and_then(|v| v.checked_add(u64::from((flags >> 1) & 1)))
            .ok_or(Error::Storage)?;
        for old in 0..index {
            let previous = record(self::row(bytes, old))?;
            if r.principal == previous.principal && r.operation_id == previous.operation_id {
                return Err(Error::Storage);
            }
        }
    }
    count_epoch_transition(&mut previous_admission, &mut minimum_updates, current)?;
    if aggregate != revision || minimum_updates > updates {
        return Err(Error::Storage);
    }
    Ok(())
}
pub(super) fn advance(bytes: &mut [u8]) -> Result<(), Error> {
    let revision = revision(bytes).checked_add(1).ok_or(Error::Storage)?;
    put64(bytes, 16, revision);
    Ok(())
}
pub(super) fn append(bytes: &mut [u8], r: DeviceJobRecord) -> Result<(), Error> {
    let index = count(bytes);
    let current = boot(bytes);
    let row = row_mut(bytes, index);
    row[..16].copy_from_slice(&r.operation_id);
    row[16..32].copy_from_slice(&r.principal);
    put32(row, 32, r.job);
    put16(row, 36, r.operation);
    put16(row, 38, r.policy_revision);
    row[40] = 2;
    row[41] = 1;
    put16(row, 42, 1);
    put16(row, 44, 2);
    row[50..52].copy_from_slice(&[1, 0]);
    put64(row, 52, current);
    put16(
        bytes,
        14,
        u16::try_from(index + 1).map_err(|_| Error::Capacity)?,
    );
    put32(bytes, 48, r.job);
    advance(bytes)
}
pub(super) fn start(bytes: &mut [u8], index: usize) -> Result<(), Error> {
    let row = row_mut(bytes, index);
    row[41] = 2;
    row[60] = 1;
    advance(bytes)
}
pub(super) fn finish(bytes: &mut [u8], index: usize, t: DeviceJobTerminal) -> Result<(), Error> {
    let row = row_mut(bytes, index);
    row[41] = t.state as u8;
    put16(row, 46, t.result);
    put16(row, 48, t.reason);
    row[60] = 3;
    advance(bytes)
}
pub(super) fn interrupt<const R: usize, const N: usize>(
    bytes: &mut [u8; N],
    boot: u64,
) -> Result<(), Error> {
    let updates = u64_at(bytes, 40).checked_add(1).ok_or(Error::Storage)?;
    put64(bytes, 24, boot);
    put64(bytes, 40, updates);
    for index in 0..count(bytes) {
        let row = row_mut(bytes, index);
        if matches!(row[41], 1 | 2) {
            row[41] = 5;
            put16(row, 46, 4);
            put16(row, 48, 0xff06);
        }
    }
    advance(bytes)
}
pub(super) fn preserves<const R: usize, const N: usize>(
    old: &[u8; N],
    new: &[u8; N],
) -> Result<(), Error> {
    if revision(new) < revision(old)
        || count(new) < count(old)
        || boot(new) < boot(old)
        || u64_at(new, 32) != u64_at(old, 32)
        || u64_at(new, 40) < u64_at(old, 40)
    {
        return Err(Error::Storage);
    }
    if revision(new) == revision(old) && new != old {
        return Err(Error::Storage);
    }
    let boot_delta = boot(new) - boot(old);
    let updates_delta = u64_at(new, 40) - u64_at(old, 40);
    if (boot_delta == 0) != (updates_delta == 0) || updates_delta > boot_delta {
        return Err(Error::Storage);
    }
    let mut previous_admission = boot(old);
    let mut minimum_updates = 0_u64;
    for index in count(old)..count(new) {
        let admission = u64_at(row(new, index), 52);
        count_epoch_transition(&mut previous_admission, &mut minimum_updates, admission)?;
    }
    count_epoch_transition(&mut previous_admission, &mut minimum_updates, boot(new))?;
    if minimum_updates > updates_delta {
        return Err(Error::Storage);
    }
    for index in 0..count(old) {
        let a = row(old, index);
        let b = row(new, index);
        let ar = record(a)?;
        let br = record(b)?;
        if !binding(ar, br)
            || a[42..46] != b[42..46]
            || a[50..60] != b[50..60]
            || a[60] & b[60] != a[60]
        {
            return Err(Error::Storage);
        }
        match ar.state {
            State::Succeeded | State::Failed | State::Interrupted => {
                if a != b {
                    return Err(Error::Storage);
                }
            }
            State::Running => {
                if br.state == State::Accepted {
                    return Err(Error::Storage);
                }
            }
            State::Accepted => {}
        }
    }
    Ok(())
}
