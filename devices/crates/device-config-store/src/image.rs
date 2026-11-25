//! Deterministic versioned bounded image codec and durable publication.
#[cfg(feature = "heart-rate")]
use crate::PolicyEntry;
#[cfg(all(feature = "heart-rate", feature = "sport"))]
use crate::ZoneEntry;
use crate::{Defaults, Error, Inner, MAX_ENCODED_LENGTH, State};
use device_configuration::ProductCapabilities;
#[cfg(all(feature = "heart-rate", feature = "sport"))]
use device_configuration::Zones;
#[cfg(feature = "heart-rate")]
use device_configuration::{HeartPolicy, Value, decode_property, encode_property};
use device_storage::persistence::{Error as StorageError, SnapshotStore};

const MAGIC: &[u8; 4] = b"DCFG";
const FORMAT: u8 = 1;
const FEATURES: u8 = (cfg!(feature = "heart-rate") as u8)
    | ((cfg!(all(feature = "heart-rate", feature = "sport")) as u8) << 1);

#[cfg(feature = "heart-rate")]
pub(super) fn validate_constraints(caps: &ProductCapabilities<'_>) -> Result<(), Error> {
    {
        let health = caps.health.ok_or(Error::Unavailable)?;
        let mut matching = health.iter().filter(|item| item.module == 0x20);
        let limits = matching.next().ok_or(Error::Unavailable)?.interval_s;
        if matching.next().is_some() || limits.min > limits.max {
            return Err(Error::InvalidValue);
        }
    }
    Ok(())
}
pub(super) fn defaults_state(
    defaults: Defaults<'_>,
    caps: &ProductCapabilities<'_>,
) -> Result<State, Error> {
    let _ = caps;
    #[cfg(not(feature = "heart-rate"))]
    if defaults.heart.is_some() {
        return Err(Error::Unavailable);
    }
    #[cfg(not(all(feature = "heart-rate", feature = "sport")))]
    if !defaults.zones.is_empty() {
        return Err(Error::Unavailable);
    }
    #[cfg(feature = "heart-rate")]
    let heart = {
        let value = defaults.heart.ok_or(Error::InvalidValue)?;
        policy_bytes(value, caps)?;
        PolicyEntry { value, revision: 1 }
    };
    #[cfg(all(feature = "heart-rate", feature = "sport"))]
    let zones = {
        let mut zones = [None; 24];
        for value in defaults.zones {
            zone_bytes(*value, caps)?;
            let slot = zones
                .get_mut(usize::from(value.sport_type))
                .ok_or(Error::InvalidValue)?;
            if slot.is_some() {
                return Err(Error::InvalidValue);
            }
            *slot = Some(ZoneEntry {
                value: *value,
                revision: 1,
            });
        }
        zones
    };
    Ok(State {
        #[cfg(feature = "heart-rate")]
        heart,
        #[cfg(all(feature = "heart-rate", feature = "sport"))]
        zones,
    })
}
pub(super) fn load<S: SnapshotStore>(
    store: &S,
    caps: &ProductCapabilities<'_>,
) -> Result<(Option<State>, u64), Error> {
    let mut bytes = [0; MAX_ENCODED_LENGTH];
    let Some(snapshot) = store.load(&mut bytes).map_err(Error::Storage)? else {
        return Ok((None, 0));
    };
    if snapshot.revision == 0 || snapshot.length > bytes.len() {
        return Err(Error::Integrity);
    }
    let state = decode_state(&bytes[..snapshot.length], caps)?;
    Ok((Some(state), snapshot.revision))
}
#[cfg_attr(
    not(feature = "heart-rate"),
    allow(
        clippy::trivially_copy_pass_by_ref,
        reason = "The same state API spans selected builds and the empty zero-sized configuration."
    )
)]
pub(super) fn persist<S: SnapshotStore>(
    inner: &mut Inner<S>,
    state: &State,
    caps: &ProductCapabilities<'_>,
) -> Result<(), Error> {
    let mut bytes = [0; MAX_ENCODED_LENGTH];
    let length = encode_state(state, &mut bytes, caps)?;
    let next = inner
        .revision
        .checked_add(1)
        .ok_or(Error::Storage(StorageError::RevisionExhausted))?;
    match inner.store.commit(inner.revision, &bytes[..length]) {
        Ok(snapshot) if snapshot.revision == next && snapshot.length == length => {
            inner.state = Some(*state);
            inner.revision = next;
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
#[cfg_attr(
    not(feature = "heart-rate"),
    allow(
        clippy::trivially_copy_pass_by_ref,
        reason = "The same codec API spans selected builds and the empty zero-sized configuration."
    )
)]
fn encode_state(
    state: &State,
    out: &mut [u8],
    caps: &ProductCapabilities<'_>,
) -> Result<usize, Error> {
    let _ = (state, caps);
    #[cfg(not(feature = "heart-rate"))]
    let required = 8;
    #[cfg(all(feature = "heart-rate", not(feature = "sport")))]
    let required = 19;
    #[cfg(all(feature = "heart-rate", feature = "sport"))]
    let required = 19 + 21 * state.zones.iter().flatten().count();
    if out.len() < required {
        return Err(Error::Storage(StorageError::Capacity));
    }
    out[..4].copy_from_slice(MAGIC);
    out[4] = FORMAT;
    out[5] = FEATURES;
    out[6] = 0;
    out[7] = 0;
    #[cfg(not(feature = "heart-rate"))]
    let position = 8;
    #[cfg(feature = "heart-rate")]
    let position = {
        out[8..12].copy_from_slice(&state.heart.revision.to_le_bytes());
        out[12..19].copy_from_slice(&policy_bytes(state.heart.value, caps)?);
        19
    };
    #[cfg(all(feature = "heart-rate", feature = "sport"))]
    let mut position = position;
    #[cfg(all(feature = "heart-rate", feature = "sport"))]
    for entry in state.zones.iter().flatten() {
        out[6] += 1;
        out[position..position + 4].copy_from_slice(&entry.revision.to_le_bytes());
        out[position + 4..position + 21].copy_from_slice(&zone_bytes(entry.value, caps)?);
        position += 21;
    }
    Ok(position)
}
fn decode_state(bytes: &[u8], caps: &ProductCapabilities<'_>) -> Result<State, Error> {
    let _ = caps;
    if bytes.len() < 8
        || &bytes[..4] != MAGIC
        || bytes[4] != FORMAT
        || bytes[5] != FEATURES
        || bytes[7] != 0
        || bytes[6] > 24
    {
        return Err(Error::Integrity);
    }
    #[cfg(not(all(feature = "heart-rate", feature = "sport")))]
    if bytes[6] != 0 {
        return Err(Error::Integrity);
    }
    #[cfg(not(feature = "heart-rate"))]
    let position = 8;
    #[cfg(feature = "heart-rate")]
    let (heart, position) = {
        if bytes.len() < 19 {
            return Err(Error::Integrity);
        }
        let revision = revision(&bytes[8..12])?;
        let Value::HeartPolicy(value) =
            decode_property(0x20, 1, &bytes[12..19], caps).map_err(|_| Error::Integrity)?
        else {
            return Err(Error::Integrity);
        };
        (PolicyEntry { value, revision }, 19)
    };
    #[cfg(all(feature = "heart-rate", feature = "sport"))]
    let mut position = position;
    #[cfg(all(feature = "heart-rate", feature = "sport"))]
    let zones = {
        let mut zones = [None; 24];
        let mut previous = None;
        for _ in 0..bytes[6] {
            if bytes.len() < position + 21 {
                return Err(Error::Integrity);
            }
            let revision = revision(&bytes[position..position + 4])?;
            let Value::Zones(value) =
                decode_property(0x20, 3, &bytes[position + 4..position + 21], caps)
                    .map_err(|_| Error::Integrity)?
            else {
                return Err(Error::Integrity);
            };
            if previous.is_some_and(|old| old >= value.sport_type) {
                return Err(Error::Integrity);
            }
            previous = Some(value.sport_type);
            zones[usize::from(value.sport_type)] = Some(ZoneEntry { value, revision });
            position += 21;
        }
        zones
    };
    if position != bytes.len() {
        return Err(Error::Integrity);
    }
    Ok(State {
        #[cfg(feature = "heart-rate")]
        heart,
        #[cfg(all(feature = "heart-rate", feature = "sport"))]
        zones,
    })
}
#[cfg(feature = "heart-rate")]
fn revision(bytes: &[u8]) -> Result<u32, Error> {
    let revision = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    if revision == 0 {
        Err(Error::Integrity)
    } else {
        Ok(revision)
    }
}
#[cfg(feature = "heart-rate")]
pub(super) fn policy_bytes(
    value: HeartPolicy,
    caps: &ProductCapabilities<'_>,
) -> Result<[u8; 7], Error> {
    let mut bytes = [0; 7];
    encode_property(0x20, 1, Value::HeartPolicy(value), &mut bytes, caps)
        .map_err(|_| Error::InvalidValue)?;
    Ok(bytes)
}
#[cfg(all(feature = "heart-rate", feature = "sport"))]
pub(super) fn zone_bytes(value: Zones, caps: &ProductCapabilities<'_>) -> Result<[u8; 17], Error> {
    let mut bytes = [0; 17];
    encode_property(0x20, 3, Value::Zones(value), &mut bytes, caps)
        .map_err(|_| Error::InvalidValue)?;
    Ok(bytes)
}
