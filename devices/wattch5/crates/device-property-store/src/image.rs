//! Strict complete image codec; values remain encoded in their actual schemas.
use crate::{DefaultValue, Error, Inner, ModuleCatalogue, PropertySchema, excluded};
use device_storage::persistence::{Error as StorageError, Snapshot, SnapshotStore};
const MAGIC: &[u8; 8] = b"DPROP001";
pub(crate) const HEADER: usize = 24;
const RECORD: usize = 14;
#[derive(Debug, Clone, Copy)]
pub(crate) struct Entry {
    pub(crate) position: usize,
    pub(crate) length: usize,
    pub(crate) revision: u32,
}
pub(crate) fn validate_catalogue(
    catalogue: &[ModuleCatalogue<'_>],
    schemas: &impl PropertySchema,
    capacity: usize,
) -> Result<(), Error> {
    if capacity < HEADER || catalogue.is_empty() {
        return Err(Error::InvalidCatalogue);
    }
    let mut previous = None;
    let mut count = 0usize;
    for module in catalogue {
        if previous.is_some_and(|old| old >= module.module)
            || matches!(module.module, 0 | 0x40 | 0xff)
            || module.properties.is_empty()
        {
            return Err(Error::InvalidCatalogue);
        }
        previous = Some(module.module);
        let mut key = None;
        for d in module.properties {
            if key.is_some_and(|old| old >= d.key)
                || excluded(module.module, d.key)
                || d.access != 11
                || d.schema == 0
                || d.value_max > 512
            {
                return Err(Error::InvalidCatalogue);
            }
            let actual = schemas.descriptor(module.module, d.key)?;
            if actual.key != d.key
                || actual.access != d.access
                || actual.schema != d.schema
                || actual.value_max < d.value_max
            {
                return Err(Error::InvalidCatalogue);
            }
            key = Some(d.key);
            count = count.checked_add(1).ok_or(Error::InvalidCatalogue)?;
        }
    }
    if count > usize::from(u16::MAX)
        || count
            .checked_mul(RECORD)
            .and_then(|n| n.checked_add(HEADER))
            .is_none_or(|n| n > capacity)
    {
        return Err(Error::InvalidCatalogue);
    }
    Ok(())
}
fn u16_at(bytes: &[u8], pos: usize) -> Result<u16, Error> {
    let b = bytes.get(pos..pos + 2).ok_or(Error::Integrity)?;
    Ok(u16::from_le_bytes([b[0], b[1]]))
}
fn entry(bytes: &[u8], position: usize) -> Result<Entry, Error> {
    let h = bytes
        .get(position..position + RECORD)
        .ok_or(Error::Integrity)?;
    let revision = u32::from_le_bytes([h[8], h[9], h[10], h[11]]);
    let length = usize::from(u16::from_le_bytes([h[12], h[13]]));
    if revision == 0
        || position
            .checked_add(RECORD)
            .and_then(|p| p.checked_add(length))
            .is_none_or(|end| end > bytes.len())
    {
        return Err(Error::Integrity);
    }
    Ok(Entry {
        position,
        length,
        revision,
    })
}
pub(crate) fn value<'a>(bytes: &'a [u8], entry: &Entry) -> &'a [u8] {
    &bytes[entry.position + RECORD..entry.position + RECORD + entry.length]
}
pub(crate) fn find(bytes: &[u8], module: u8, key: u16) -> Result<Entry, Error> {
    let count = u16_at(bytes, 16)?;
    let mut position = HEADER;
    for _ in 0..count {
        let e = entry(bytes, position)?;
        if bytes[position] == module && u16_at(bytes, position + 2)? == key {
            return Ok(e);
        }
        position += RECORD + e.length;
    }
    Err(Error::Unavailable)
}
pub(crate) fn validate(
    bytes: &[u8],
    revision: u64,
    catalogue: &[ModuleCatalogue<'_>],
    schemas: &impl PropertySchema,
) -> Result<(), Error> {
    if bytes.len() < HEADER
        || &bytes[..8] != MAGIC
        || bytes[18..24] != [0; 6]
        || revision == 0
        || bytes[8..16] != revision.to_le_bytes()
    {
        return Err(Error::Integrity);
    }
    let count = usize::from(u16_at(bytes, 16)?);
    let mut seen = 0usize;
    let mut aggregate_revision = 1u64;
    let mut position = HEADER;
    for module in catalogue {
        for d in module.properties {
            let e = entry(bytes, position)?;
            aggregate_revision = aggregate_revision
                .checked_add(u64::from(e.revision) - 1)
                .ok_or(Error::Integrity)?;
            if bytes[position] != module.module
                || bytes[position + 1] != d.access
                || u16_at(bytes, position + 2)? != d.key
                || u16_at(bytes, position + 4)? != d.schema
                || u16_at(bytes, position + 6)? != d.value_max
                || e.length > usize::from(d.value_max)
            {
                return Err(Error::Integrity);
            }
            schemas
                .validate(module.module, d.key, value(bytes, &e))
                .map_err(|_| Error::Integrity)?;
            seen += 1;
            position += RECORD + e.length;
        }
    }
    if seen != count || position != bytes.len() || aggregate_revision != revision {
        return Err(Error::Integrity);
    }
    Ok(())
}
pub(crate) fn load<S: SnapshotStore>(
    store: &S,
    catalogue: &[ModuleCatalogue<'_>],
    schemas: &impl PropertySchema,
    out: &mut [u8],
    floor: u64,
) -> Result<Option<Snapshot>, Error> {
    let Some(snapshot) = store.load(out).map_err(Error::Storage)? else {
        return if floor == 0 {
            Ok(None)
        } else {
            Err(Error::Integrity)
        };
    };
    if snapshot.revision < floor || snapshot.length > out.len() {
        return Err(Error::Integrity);
    }
    validate(
        &out[..snapshot.length],
        snapshot.revision,
        catalogue,
        schemas,
    )?;
    Ok(Some(snapshot))
}
pub(crate) fn defaults(
    defaults: &[DefaultValue<'_>],
    catalogue: &[ModuleCatalogue<'_>],
    schemas: &impl PropertySchema,
    out: &mut [u8],
) -> Result<usize, Error> {
    let count: usize = catalogue.iter().map(|m| m.properties.len()).sum();
    if defaults.len() != count {
        return Err(Error::InvalidValue);
    }
    out[..8].copy_from_slice(MAGIC);
    out[8..16].copy_from_slice(&1u64.to_le_bytes());
    out[16..18].copy_from_slice(
        &u16::try_from(count)
            .map_err(|_| Error::InvalidValue)?
            .to_le_bytes(),
    );
    let mut position = HEADER;
    let mut index = 0;
    for module in catalogue {
        for d in module.properties {
            let v = &defaults[index];
            if v.module != module.module
                || v.key != d.key
                || v.value.len() > usize::from(d.value_max)
            {
                return Err(Error::InvalidValue);
            }
            schemas.validate(v.module, v.key, v.value)?;
            let end = position
                .checked_add(RECORD)
                .and_then(|n| n.checked_add(v.value.len()))
                .filter(|n| *n <= out.len())
                .ok_or(Error::Storage(StorageError::Capacity))?;
            out[position] = v.module;
            out[position + 1] = d.access;
            out[position + 2..position + 4].copy_from_slice(&d.key.to_le_bytes());
            out[position + 4..position + 6].copy_from_slice(&d.schema.to_le_bytes());
            out[position + 6..position + 8].copy_from_slice(&d.value_max.to_le_bytes());
            out[position + 8..position + 12].copy_from_slice(&1u32.to_le_bytes());
            out[position + 12..position + 14].copy_from_slice(
                &u16::try_from(v.value.len())
                    .map_err(|_| Error::InvalidValue)?
                    .to_le_bytes(),
            );
            out[position + RECORD..end].copy_from_slice(v.value);
            position = end;
            index += 1;
        }
    }
    Ok(position)
}
pub(crate) fn replace<const N: usize>(
    old: &[u8],
    e: Entry,
    bytes: &mut [u8; N],
    value: &[u8],
    revision: u32,
) -> Result<usize, Error> {
    let prefix = e.position + RECORD;
    let tail = prefix + e.length;
    let length = old
        .len()
        .checked_sub(e.length)
        .and_then(|n| n.checked_add(value.len()))
        .filter(|n| *n <= N)
        .ok_or(Error::Storage(StorageError::Capacity))?;
    bytes[..prefix].copy_from_slice(&old[..prefix]);
    bytes[e.position + 8..e.position + 12].copy_from_slice(&revision.to_le_bytes());
    bytes[e.position + 12..e.position + 14].copy_from_slice(
        &u16::try_from(value.len())
            .map_err(|_| Error::InvalidValue)?
            .to_le_bytes(),
    );
    bytes[prefix..prefix + value.len()].copy_from_slice(value);
    bytes[prefix + value.len()..length].copy_from_slice(&old[tail..]);
    Ok(length)
}
pub(crate) fn replace_in_place(
    bytes: &mut [u8],
    old_length: usize,
    e: Entry,
    value: &[u8],
    revision: u32,
) -> Result<usize, Error> {
    let prefix = e.position + RECORD;
    let tail = prefix + e.length;
    let length = old_length
        .checked_sub(e.length)
        .and_then(|n| n.checked_add(value.len()))
        .filter(|n| *n <= bytes.len())
        .ok_or(Error::Storage(StorageError::Capacity))?;
    let encoded_length = u16::try_from(value.len()).map_err(|_| Error::InvalidValue)?;
    bytes.copy_within(tail..old_length, prefix + value.len());
    bytes[e.position + 8..e.position + 12].copy_from_slice(&revision.to_le_bytes());
    bytes[e.position + 12..e.position + 14].copy_from_slice(&encoded_length.to_le_bytes());
    bytes[prefix..prefix + value.len()].copy_from_slice(value);
    Ok(length)
}
pub(crate) fn persist<S: SnapshotStore, const N: usize>(
    inner: &mut Inner<S, N>,
    mut bytes: [u8; N],
    length: usize,
) -> Result<(), Error> {
    let next = inner
        .revision
        .checked_add(1)
        .ok_or(Error::Storage(StorageError::RevisionExhausted))?;
    bytes[8..16].copy_from_slice(&next.to_le_bytes());
    match inner.store.commit(inner.revision, &bytes[..length]) {
        Ok(snapshot) if snapshot.revision == next && snapshot.length == length => {
            inner.bytes = bytes;
            inner.length = length;
            inner.revision = next;
            inner.floor = next;
            inner.initialized = true;
            Ok(())
        }
        Ok(_) => {
            inner.poisoned = true;
            Err(Error::Storage(StorageError::NeedsRecovery))
        }
        Err(error) => {
            if !matches!(
                error,
                StorageError::Capacity | StorageError::Conflict | StorageError::RevisionExhausted
            ) {
                inner.poisoned = true;
            }
            Err(Error::Storage(error))
        }
    }
}
