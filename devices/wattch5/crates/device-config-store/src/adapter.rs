//! Optional module-bound property paths over the checked owner.
use crate::image::persist;
use crate::{ConfigStore, Error, State};
#[cfg(feature = "heart-rate")]
use crate::{PolicyEntry, image::policy_bytes};
#[cfg(all(feature = "heart-rate", feature = "sport"))]
use crate::{ZoneEntry, image::zone_bytes};
use core::fmt;
#[cfg(all(feature = "heart-rate", feature = "wellbeing"))]
use device_configuration::HeartPolicy;
use device_configuration::ProductCapabilities;
#[cfg(feature = "heart-rate")]
use device_configuration::{Value, decode_property};
use device_services::{
    PropertyAccess, PropertyBackend, PropertyDescriptor, PropertyFailure, PropertyOperation,
    PropertyValue, RevisionChange,
};
use device_storage::persistence::{Error as StorageError, SnapshotStore};

/// A module-bound property adapter referencing the one shared owner.
pub struct PropertyAdapter<'owner, 'caps, S: SnapshotStore> {
    pub(super) owner: &'owner ConfigStore<'caps, S>,
    pub(super) module: u8,
    pub(super) sport_type: Option<u16>,
}
impl<S: SnapshotStore> fmt::Debug for PropertyAdapter<'_, '_, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PropertyAdapter")
            .field("module", &self.module)
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "heart-rate")]
const POLICY: PropertyDescriptor = PropertyDescriptor {
    key: 1,
    access: 11,
    schema: 1,
    value_max: 7,
};
#[cfg(all(feature = "heart-rate", feature = "sport"))]
const HR_ZONES: PropertyDescriptor = PropertyDescriptor {
    key: 3,
    access: 11,
    schema: 1,
    value_max: 17,
};
#[cfg(all(feature = "heart-rate", feature = "sport"))]
const SPORT_ZONES: PropertyDescriptor = PropertyDescriptor {
    key: 2,
    access: 11,
    schema: 1,
    value_max: 17,
};
#[cfg(all(feature = "heart-rate", feature = "wellbeing"))]
const RMSSD: PropertyDescriptor = PropertyDescriptor {
    key: 1,
    access: 11,
    schema: 1,
    value_max: 1,
};
impl<S: SnapshotStore> PropertyAdapter<'_, '_, S> {
    fn descriptor(&self, key: u16) -> Result<(), PropertyFailure> {
        if self.catalogue().iter().any(|entry| entry.key == key) {
            Ok(())
        } else {
            Err(PropertyFailure::NotFound)
        }
    }
}
impl<S: SnapshotStore> PropertyBackend for PropertyAdapter<'_, '_, S> {
    fn catalogue(&self) -> &[PropertyDescriptor] {
        match self.module {
            #[cfg(all(feature = "heart-rate", feature = "sport"))]
            0x20 if self.sport_type.is_some() => &[POLICY, HR_ZONES],
            #[cfg(feature = "heart-rate")]
            0x20 => &[POLICY],
            #[cfg(all(feature = "heart-rate", feature = "sport"))]
            0x22 => &[SPORT_ZONES],
            #[cfg(all(feature = "heart-rate", feature = "wellbeing"))]
            0x2a => &[RMSSD],
            _ => &[],
        }
    }
    fn read(&self, key: u16, out: &mut [u8]) -> Result<PropertyValue, PropertyFailure> {
        self.descriptor(key)?;
        let inner = self
            .owner
            .inner
            .try_borrow()
            .map_err(|_| PropertyFailure::Busy)?;
        let state = inner.ready().map_err(property_error)?;
        read_property(
            state,
            self.module,
            key,
            self.sport_type,
            out,
            &self.owner.capabilities,
        )
    }
    fn validate_value(&self, key: u16, value: &[u8]) -> Result<(), PropertyFailure> {
        self.descriptor(key)?;
        let inner = self
            .owner
            .inner
            .try_borrow()
            .map_err(|_| PropertyFailure::Busy)?;
        let state = inner.ready().map_err(property_error)?;
        updated(
            state,
            self.module,
            key,
            self.sport_type,
            value,
            &self.owner.capabilities,
            false,
        )
        .map(|_| ())
    }
    fn commit_atomic(
        &mut self,
        module: u8,
        key: u16,
        expected: u32,
        value: &[u8],
        access: &dyn PropertyAccess,
    ) -> Result<RevisionChange, PropertyFailure> {
        if module != self.module {
            return Err(PropertyFailure::AccessDenied);
        }
        self.descriptor(key)?;
        let mut inner = self
            .owner
            .inner
            .try_borrow_mut()
            .map_err(|_| PropertyFailure::Busy)?;
        let old = inner.ready().map_err(property_error)?;
        let (state, previous) = updated(
            old,
            module,
            key,
            self.sport_type,
            value,
            &self.owner.capabilities,
            true,
        )?;
        if expected != 0 && expected != previous {
            return Err(PropertyFailure::Conflict);
        }
        let current = previous
            .checked_add(1)
            .ok_or(PropertyFailure::RevisionExhausted)?;
        if !access.allowed(module, PropertyOperation::Set, None)
            || !access.allowed(module, PropertyOperation::Set, Some(key))
        {
            return Err(PropertyFailure::AccessDenied);
        }
        persist(&mut inner, &state, &self.owner.capabilities).map_err(property_error)?;
        Ok(RevisionChange { previous, current })
    }
}

fn property_error(error: Error) -> PropertyFailure {
    match error {
        Error::Busy => PropertyFailure::Busy,
        Error::Uninitialized | Error::Unavailable => PropertyFailure::NotFound,
        Error::InvalidValue => PropertyFailure::InvalidValue,
        Error::AlreadyInitialized | Error::Storage(StorageError::Conflict) => {
            PropertyFailure::Conflict
        }
        Error::Integrity | Error::Storage(StorageError::Corrupt) => {
            PropertyFailure::IntegrityFailure
        }
        Error::Storage(StorageError::Capacity) => PropertyFailure::LimitExceeded,
        Error::Storage(StorageError::RevisionExhausted) => PropertyFailure::RevisionExhausted,
        Error::Storage(_) => PropertyFailure::StorageFailure,
    }
}
#[cfg_attr(
    not(feature = "heart-rate"),
    allow(
        clippy::trivially_copy_pass_by_ref,
        reason = "The adapter signature spans selected builds and an empty zero-sized state."
    )
)]
fn read_property(
    state: &State,
    module: u8,
    key: u16,
    sport: Option<u16>,
    out: &mut [u8],
    caps: &ProductCapabilities<'_>,
) -> Result<PropertyValue, PropertyFailure> {
    let _ = (state, sport, &out, caps);
    match (module, key) {
        #[cfg(feature = "heart-rate")]
        (0x20, 1) => copy_value(
            state.heart.revision,
            &policy_bytes(state.heart.value, caps).map_err(property_error)?,
            out,
        ),
        #[cfg(all(feature = "heart-rate", feature = "wellbeing"))]
        (0x2a, 1) => copy_value(
            state.heart.revision,
            &[u8::from(state.heart.value.rmssd_enabled)],
            out,
        ),
        #[cfg(all(feature = "heart-rate", feature = "sport"))]
        (0x20, 3) | (0x22, 2) => {
            let entry = zone_entry(state, sport)?;
            copy_value(
                entry.revision,
                &zone_bytes(entry.value, caps).map_err(property_error)?,
                out,
            )
        }
        _ => Err(PropertyFailure::NotFound),
    }
}
#[cfg(feature = "heart-rate")]
fn copy_value(
    revision: u32,
    bytes: &[u8],
    out: &mut [u8],
) -> Result<PropertyValue, PropertyFailure> {
    if out.len() < bytes.len() {
        return Err(PropertyFailure::LimitExceeded);
    }
    out[..bytes.len()].copy_from_slice(bytes);
    Ok(PropertyValue {
        revision,
        length: bytes.len(),
    })
}
#[cfg(all(feature = "heart-rate", feature = "sport"))]
fn zone_entry(state: &State, sport: Option<u16>) -> Result<ZoneEntry, PropertyFailure> {
    sport
        .and_then(|sport| state.zones.get(usize::from(sport)))
        .and_then(|entry| *entry)
        .ok_or(PropertyFailure::NotFound)
}
#[cfg_attr(
    not(feature = "heart-rate"),
    allow(
        clippy::trivially_copy_pass_by_ref,
        reason = "The shared update API also handles the empty zero-sized configuration."
    )
)]
fn updated(
    state: &State,
    module: u8,
    key: u16,
    sport: Option<u16>,
    bytes: &[u8],
    caps: &ProductCapabilities<'_>,
    advance: bool,
) -> Result<(State, u32), PropertyFailure> {
    let _ = (state, sport, bytes, caps, advance);
    #[cfg(feature = "heart-rate")]
    let mut state = *state;
    match (module, key) {
        #[cfg(feature = "heart-rate")]
        (0x20, 1) => {
            let Value::HeartPolicy(value) = decode_property(module, key, bytes, caps)
                .map_err(|_| PropertyFailure::InvalidValue)?
            else {
                return Err(PropertyFailure::InvalidValue);
            };
            let old = state.heart.revision;
            state.heart = PolicyEntry {
                value,
                revision: next_revision(old, advance)?,
            };
            Ok((state, old))
        }
        #[cfg(all(feature = "heart-rate", feature = "wellbeing"))]
        (0x2a, 1) => {
            if bytes.len() != 1 || bytes[0] > 1 {
                return Err(PropertyFailure::InvalidValue);
            }
            let value = HeartPolicy {
                rmssd_enabled: bytes[0] == 1,
                ..state.heart.value
            };
            policy_bytes(value, caps).map_err(property_error)?;
            let old = state.heart.revision;
            state.heart = PolicyEntry {
                value,
                revision: next_revision(old, advance)?,
            };
            Ok((state, old))
        }
        #[cfg(all(feature = "heart-rate", feature = "sport"))]
        (0x20, 3) | (0x22, 2) => {
            let old = zone_entry(&state, sport)?;
            let Value::Zones(value) = decode_property(module, key, bytes, caps)
                .map_err(|_| PropertyFailure::InvalidValue)?
            else {
                return Err(PropertyFailure::InvalidValue);
            };
            if Some(value.sport_type) != sport {
                return Err(PropertyFailure::InvalidValue);
            }
            state.zones[usize::from(value.sport_type)] = Some(ZoneEntry {
                value,
                revision: next_revision(old.revision, advance)?,
            });
            Ok((state, old.revision))
        }
        _ => Err(PropertyFailure::NotFound),
    }
}

#[cfg(feature = "heart-rate")]
fn next_revision(old: u32, advance: bool) -> Result<u32, PropertyFailure> {
    if advance {
        old.checked_add(1).ok_or(PropertyFailure::RevisionExhausted)
    } else {
        Ok(old)
    }
}
