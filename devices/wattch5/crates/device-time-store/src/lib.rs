//! Portable durable UTC mapping over an owned snapshot store and trusted clock.
//!
//! Timebase continuity, drift bounds, authorization and physical storage are
//! explicit integration inputs. No hardware source is selected by this crate.
#![no_std]
#![forbid(unsafe_code)]

use core::{cell::RefCell, fmt};
use device_services::{ClockSourceError, SoftwareClock, TimeAccess, TimeError, UtcMapping};
use device_storage::persistence::{Error as StorageError, SnapshotStore};

mod handler;
mod image;
pub use handler::DurableTimeHandler;

/// Actual sample from one trusted continuous calibrated clock.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ClockReading {
    /// Nonzero identity of the actual source and calibration scope.
    pub source_id: u64,
    /// Nonzero continuity epoch, never reused after a discontinuity.
    pub epoch: u64,
    /// Actual microseconds in this source/epoch coordinate.
    pub mono_us: u64,
}
impl fmt::Debug for ClockReading {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ClockReading").finish_non_exhaustive()
    }
}
/// Integration-owned clock continuity attestation, never decoded from a phone.
///
/// The source/epoch pair must never be reused after lost continuity or a change
/// of actual clock/calibration identity, including across physical restarts.
pub trait TrustedClock {
    /// Capture actual time and its continuity attestation.
    /// # Errors
    /// Return unavailable temporarily, or fault when continuity is lost.
    fn reading(&self) -> Result<ClockReading, ClockSourceError>;
}
impl<T: TrustedClock + ?Sized> TrustedClock for &T {
    fn reading(&self) -> Result<ClockReading, ClockSourceError> {
        T::reading(self)
    }
}
/// Last complete durable synchronization, for trusted internal diagnostics.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SyncEvidence {
    /// Full snapshot revision, starting at one.
    pub revision: u64,
    /// Actual clock sample captured for this synchronization.
    pub reading: ClockReading,
    /// Authorized UTC at the captured sample, in milliseconds.
    pub utc_ms: i64,
    /// Initial conservative uncertainty in milliseconds.
    pub uncertainty_ms: u32,
    /// Trusted conservative drift bound at synchronization.
    pub max_drift_ppm: u32,
}
impl fmt::Debug for SyncEvidence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SyncEvidence")
            .field("revision", &self.revision)
            .finish_non_exhaustive()
    }
}
/// Ownership, authorization, clock progression or durable storage failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// A recursive callback or another checked borrow already owns the state.
    Busy,
    /// The initial live scope is absent.
    AccessDenied,
    /// Live scope was revoked after the operation began.
    AuthorizationRevoked,
    /// The real trusted source is unavailable or failed.
    Clock(ClockSourceError),
    /// A source or continuity epoch is zero and cannot attest continuity.
    InvalidReading,
    /// Actual time regressed within one attested coordinate.
    NonMonotonic,
    /// UTC, elapsed drift or uncertainty cannot be represented.
    Overflow,
    /// Stored format, revision or confirmed generation is inconsistent.
    Integrity,
    /// Actual persistence failed; uncertain outcomes require explicit recovery.
    Storage(StorageError),
}
struct Inner<S> {
    store: S,
    evidence: Option<SyncEvidence>,
    revision: u64,
    last: Option<ClockReading>,
    invalid: bool,
    poisoned: bool,
}
impl<S> Inner<S> {
    fn ready(&self) -> Result<(), Error> {
        if self.poisoned {
            Err(Error::Storage(StorageError::NeedsRecovery))
        } else {
            Ok(())
        }
    }
    fn observe(&mut self, reading: ClockReading) -> Result<(), Error> {
        if let Some(last) = self.last {
            if same_identity(last, reading) {
                if reading.mono_us < last.mono_us {
                    self.invalid = true;
                    return Err(Error::NonMonotonic);
                }
            } else {
                self.invalid = true;
            }
        }
        self.last = Some(reading);
        if reading.source_id == 0 || reading.epoch == 0 {
            self.invalid = true;
            return Err(Error::InvalidReading);
        }
        Ok(())
    }
}
fn same_identity(left: ClockReading, right: ClockReading) -> bool {
    left.source_id == right.source_id && left.epoch == right.epoch
}
/// Single-context owner of one durable time partition and trusted clock source.
///
/// Checked borrowing serializes callbacks, not interrupts or multiple cores.
/// Integration supplies any synchronization required beyond this model.
pub struct TimeStore<S: SnapshotStore, C: TrustedClock> {
    inner: RefCell<Inner<S>>,
    source: C,
    max_drift_ppm: u32,
}
impl<S: SnapshotStore, C: TrustedClock> fmt::Debug for TimeStore<S, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TimeStore").finish_non_exhaustive()
    }
}
impl<S: SnapshotStore, C: TrustedClock> TimeStore<S, C> {
    /// Load strict existing evidence or confirmed blank media, without writing.
    /// Zero drift is appropriate only for an exact trusted source.
    /// # Errors
    /// Reject storage failure or incompatible/corrupt stored evidence.
    pub fn open(store: S, source: C, max_drift_ppm: u32) -> Result<Self, Error> {
        let evidence = image::load(&store)?;
        Ok(Self {
            inner: RefCell::new(Inner {
                store,
                evidence,
                revision: evidence.map_or(0, |item| item.revision),
                last: evidence.map(|item| item.reading),
                invalid: false,
                poisoned: false,
            }),
            source,
            max_drift_ppm,
        })
    }
    /// Release the actual store and clock provider for shutdown or reopening.
    pub fn into_parts(self) -> (S, C) {
        (self.inner.into_inner().store, self.source)
    }
    /// Read the confirmed full snapshot revision; blank storage has revision zero.
    /// # Errors
    /// Return busy or recovery-required state explicitly.
    pub fn revision(&self) -> Result<u64, Error> {
        let inner = self.inner.try_borrow().map_err(|_| Error::Busy)?;
        inner.ready()?;
        Ok(inner.revision)
    }
    /// Read last durable evidence for trusted diagnostics, never as wire access.
    /// # Errors
    /// Return busy or recovery-required state explicitly.
    pub fn evidence(&self) -> Result<Option<SyncEvidence>, Error> {
        let inner = self.inner.try_borrow().map_err(|_| Error::Busy)?;
        inner.ready()?;
        Ok(inner.evidence)
    }
    pub(crate) fn check_access(&self, access: &dyn TimeAccess, set: bool) -> Result<(), Error> {
        let _inner = self.inner.try_borrow_mut().map_err(|_| Error::Busy)?;
        if access.allowed(set) {
            Ok(())
        } else {
            Err(Error::AccessDenied)
        }
    }
    fn sample(
        &self,
        inner: &mut Inner<S>,
        access: &dyn TimeAccess,
        set: bool,
    ) -> Result<ClockReading, Error> {
        if !access.allowed(set) {
            return Err(Error::AccessDenied);
        }
        let sampled = match self.source.reading() {
            Ok(reading) => inner.observe(reading).map(|()| reading),
            Err(error) => {
                if error == ClockSourceError::Fault {
                    inner.invalid = true;
                }
                Err(Error::Clock(error))
            }
        };
        if !access.allowed(set) {
            return Err(Error::AuthorizationRevoked);
        }
        sampled
    }
    /// Map an actual trusted sample with conservative uncertainty progression.
    /// # Errors
    /// Reject absent/revoked read scope, real clock failures, overflow or storage uncertainty.
    pub fn read(&self, access: &dyn TimeAccess) -> Result<UtcMapping, Error> {
        let mut inner = self.inner.try_borrow_mut().map_err(|_| Error::Busy)?;
        inner.ready()?;
        let reading = self.sample(&mut inner, access, false)?;
        let mapping = if inner.invalid {
            unavailable(reading.mono_us)
        } else if let Some(evidence) = inner.evidence {
            if same_identity(evidence.reading, reading) {
                progress(evidence, reading.mono_us, self.max_drift_ppm)?
            } else {
                inner.invalid = true;
                unavailable(reading.mono_us)
            }
        } else {
            unavailable(reading.mono_us)
        };
        if !access.allowed(false) {
            return Err(Error::AuthorizationRevoked);
        }
        Ok(mapping)
    }
    /// Persist an explicitly authorized synchronization before publishing UTC.
    /// The first successful synchronization initializes blank media at revision one.
    /// # Errors
    /// Reject scope, clock, revision or persistence failures. Uncertain writes poison access.
    pub fn synchronize(
        &self,
        utc_ms: i64,
        uncertainty_ms: u32,
        access: &dyn TimeAccess,
    ) -> Result<UtcMapping, Error> {
        let mut inner = self.inner.try_borrow_mut().map_err(|_| Error::Busy)?;
        inner.ready()?;
        let reading = self.sample(&mut inner, access, true)?;
        let revision = inner
            .revision
            .checked_add(1)
            .ok_or(Error::Storage(StorageError::RevisionExhausted))?;
        let evidence = SyncEvidence {
            revision,
            reading,
            utc_ms,
            uncertainty_ms,
            max_drift_ppm: self.max_drift_ppm,
        };
        let bytes = image::encode(evidence);
        if !access.allowed(true) {
            return Err(Error::AuthorizationRevoked);
        }
        let expected = inner.revision;
        match inner.store.commit(expected, &bytes) {
            Ok(snapshot) if snapshot.revision == revision && snapshot.length == bytes.len() => {
                inner.evidence = Some(evidence);
                inner.revision = revision;
                inner.invalid = false;
            }
            Ok(_) => {
                inner.poisoned = true;
                return Err(Error::Storage(StorageError::NeedsRecovery));
            }
            Err(error) => {
                if !matches!(
                    error,
                    StorageError::Capacity
                        | StorageError::Conflict
                        | StorageError::RevisionExhausted
                ) {
                    inner.poisoned = true;
                }
                return Err(Error::Storage(error));
            }
        }
        if !access.allowed(true) {
            return Err(Error::AuthorizationRevoked);
        }
        Ok(UtcMapping {
            utc_ms,
            mono_us: reading.mono_us,
            uncertainty_ms,
            valid: true,
        })
    }
    /// Recover and validate the whole partition while retaining observed clock faults/floor.
    /// Existing connection actors must be reconstructed after recovery.
    /// # Errors
    /// Reject recovery failure, corrupt evidence or rollback below the confirmed generation.
    pub fn recover_and_reload(&self) -> Result<(), Error> {
        let mut inner = self.inner.try_borrow_mut().map_err(|_| Error::Busy)?;
        inner.poisoned = true;
        inner.store.recover().map_err(Error::Storage)?;
        let evidence = image::load(&inner.store)?;
        let revision = evidence.map_or(0, |item| item.revision);
        if revision < inner.revision || (revision == inner.revision && evidence != inner.evidence) {
            return Err(Error::Integrity);
        }
        if let Some(item) = evidence
            && inner.last.is_none_or(|last| {
                same_identity(last, item.reading) && last.mono_us < item.reading.mono_us
            })
        {
            inner.last = Some(item.reading);
        }
        inner.evidence = evidence;
        inner.revision = revision;
        inner.poisoned = false;
        Ok(())
    }
}
fn unavailable(mono_us: u64) -> UtcMapping {
    UtcMapping {
        utc_ms: 0,
        mono_us,
        uncertainty_ms: 0,
        valid: false,
    }
}
fn progress(evidence: SyncEvidence, now: u64, configured_drift: u32) -> Result<UtcMapping, Error> {
    let mut clock = SoftwareClock::new(evidence.max_drift_ppm.max(configured_drift));
    clock
        .synchronize(
            evidence.utc_ms,
            evidence.uncertainty_ms,
            evidence.reading.mono_us,
        )
        .map_err(time_error)?;
    clock.read(now).map_err(time_error)
}
fn time_error(error: TimeError) -> Error {
    match error {
        TimeError::NonMonotonic => Error::NonMonotonic,
        TimeError::Overflow => Error::Overflow,
    }
}
