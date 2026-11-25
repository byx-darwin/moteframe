//! Portable durable commercial job identities over an exclusively owned snapshot store.
//!
//! Actual boot identity, policy and reset isolation remain trusted integration ports.
//! This crate provides no physical clear/reset executor.
#![no_std]
#![forbid(unsafe_code)]

use core::{cell::Cell, fmt};
use device_maintenance_service::Error;
use device_storage::persistence::{Error as StorageError, SnapshotStore};

mod image;
mod ledger;

/// Trusted actual boot epoch, stable within one boot and never reused after reset.
pub trait TrustedBoot {
    /// Read the nonzero monotonic actual boot epoch, never a peer/session value.
    /// # Errors
    /// Report unavailable or failed trusted boot evidence explicitly.
    fn current_boot(&self) -> Result<u64, Error>;
}
/// Trusted current product policy; captured historical semantics need explicit permission.
pub trait TrustedJobPolicy {
    /// Whether the product currently supports this commercial operation.
    fn supports(&self, operation: u16) -> bool;
    /// Read the nonzero current policy revision for a new acceptance.
    /// # Errors
    /// Report unavailable policy explicitly.
    fn current_revision(&self) -> Result<u16, Error>;
    /// Check current authorization for an operation under its captured policy revision.
    fn permits(&self, principal: [u8; 16], operation: u16, captured_revision: u16) -> bool;
}
/// Trusted integration proof that real clear/reset effects preserve this ledger partition.
pub trait ResetIsolation {
    /// Affirm actual isolation for the supplied operation; absence must return false.
    fn preserves_ledger(&self, operation: u16) -> bool;
}
/// Exclusive portable owner of bounded, permanently retained commercial job identities.
///
/// `R` is the lifetime identity capacity (at least 32); `N` must hold `64 + 64*R`
/// bytes. The owned snapshot namespace must have no other writers. Boot, policy,
/// reset isolation and interrupt/core synchronization remain integration duties.
pub struct JobStore<S, B, P, I, const R: usize = 32, const N: usize = 4096> {
    store: S,
    boot: B,
    policy: P,
    isolation: I,
    confirmed: [u8; N],
    initialized: bool,
    poisoned: bool,
    observed_boot: Cell<u64>,
    active: Cell<bool>,
}
impl<S, B, P, I, const R: usize, const N: usize> fmt::Debug for JobStore<S, B, P, I, R, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobStore")
            .field("retention_capacity", &R)
            .field("initialized", &self.initialized)
            .field("poisoned", &self.poisoned)
            .finish_non_exhaustive()
    }
}
impl<
    S: SnapshotStore,
    B: TrustedBoot,
    P: TrustedJobPolicy,
    I: ResetIsolation,
    const R: usize,
    const N: usize,
> JobStore<S, B, P, I, R, N>
{
    /// Load a strict existing image or confirmed blank namespace without formatting.
    /// A newer actual boot is durably reconciled before this returns a ready owner.
    /// # Errors
    /// Reject invalid bounds, storage/format failures or invalid trusted boot evidence.
    pub fn open(store: S, boot: B, policy: P, isolation: I) -> Result<Self, Error> {
        image::length::<R, N>()?;
        let mut owner = Self {
            store,
            boot,
            policy,
            isolation,
            confirmed: [0; N],
            initialized: false,
            poisoned: false,
            observed_boot: Cell::new(0),
            active: Cell::new(false),
        };
        let current = owner.observe_boot()?;
        let mut loaded = [0; N];
        if image::load::<S, R, N>(&owner.store, &mut loaded)? {
            owner.confirmed = loaded;
            owner.initialized = true;
            owner.reconcile_boot(current)?;
        }
        Ok(owner)
    }
    /// Persist the full fixed-capacity empty image in a confirmed blank namespace.
    /// # Errors
    /// Reject existing data, missing context, precheck failures or uncertain durability.
    pub fn initialize(&mut self) -> Result<(), Error> {
        self.write_operation(|owner| {
            if owner.poisoned {
                return Err(Error::Storage);
            }
            if owner.initialized {
                return Err(Error::Conflict);
            }
            owner.check_isolation()?;
            let boot = owner.observe_boot()?;
            if owner.policy.current_revision()? == 0 {
                return Err(Error::Unavailable);
            }
            let mut next = [0; N];
            image::initialize::<R, N>(&mut next, boot)?;
            owner.persist(next, 0)
        })
    }
    /// Recover uncertain storage and reload while preserving confirmed in-owner history.
    /// Same-boot recovery retains unresolved work; only newer actual boot evidence interrupts it.
    /// # Errors
    /// Reject storage errors, rollback, changed retained identities or invalid context.
    pub fn recover_and_reload(&mut self) -> Result<(), Error> {
        self.write_operation(|owner| {
            owner.poisoned = true;
            let boot = owner.observe_boot()?;
            owner.store.recover().map_err(|_| Error::Storage)?;
            let mut next = [0; N];
            let initialized = image::load::<S, R, N>(&owner.store, &mut next)?;
            if owner.initialized {
                if !initialized {
                    return Err(Error::Storage);
                }
                image::preserves::<R, N>(&owner.confirmed, &next)?;
            }
            if initialized && image::boot(&next) > boot {
                return Err(Error::Unavailable);
            }
            if initialized {
                owner.confirmed = next;
                owner.initialized = true;
                // The recovered image is now the confirmed floor even if boot reconciliation fails.
                owner.reconcile_boot(boot)?;
            }
            owner.poisoned = false;
            Ok(())
        })
    }
    /// Read the confirmed revision only while initialized and ready.
    /// # Errors
    /// Return unavailable context or poisoned storage explicitly.
    pub fn revision(&self) -> Result<u64, Error> {
        self.read_operation(|owner| {
            owner.ready()?;
            Ok(image::revision(&owner.confirmed))
        })
    }
    /// Read the number of permanently retained identities.
    /// # Errors
    /// Return unavailable context or poisoned storage explicitly.
    pub fn retained(&self) -> Result<usize, Error> {
        self.read_operation(|owner| {
            owner.ready()?;
            Ok(image::count(&owner.confirmed))
        })
    }
    /// Release adapters for explicit shutdown/recovery; no effects are executed.
    pub fn into_parts(self) -> (S, B, P, I) {
        (self.store, self.boot, self.policy, self.isolation)
    }
    fn read_operation<T>(
        &self,
        operation: impl FnOnce(&Self) -> Result<T, Error>,
    ) -> Result<T, Error> {
        if self.active.replace(true) {
            return Err(Error::Busy);
        }
        let result = operation(self);
        self.active.set(false);
        result
    }
    fn write_operation<T>(
        &mut self,
        operation: impl FnOnce(&mut Self) -> Result<T, Error>,
    ) -> Result<T, Error> {
        if self.active.replace(true) {
            return Err(Error::Busy);
        }
        let result = operation(self);
        self.active.set(false);
        result
    }
    fn observe_boot(&self) -> Result<u64, Error> {
        let boot = self.boot.current_boot()?;
        if boot == 0 || boot < self.observed_boot.get() {
            return Err(Error::Unavailable);
        }
        self.observed_boot.set(boot);
        Ok(boot)
    }
    fn check_isolation(&self) -> Result<(), Error> {
        if self.isolation.preserves_ledger(1) && self.isolation.preserves_ledger(2) {
            Ok(())
        } else {
            Err(Error::Unavailable)
        }
    }
    fn ready(&self) -> Result<(), Error> {
        if self.poisoned {
            return Err(Error::Storage);
        }
        if !self.initialized {
            return Err(Error::Unavailable);
        }
        self.check_isolation()?;
        if self.observe_boot()? != image::boot(&self.confirmed) {
            return Err(Error::Unavailable);
        }
        Ok(())
    }
    fn reconcile_boot(&mut self, boot: u64) -> Result<(), Error> {
        let old = image::boot(&self.confirmed);
        if boot < old {
            return Err(Error::Unavailable);
        }
        self.check_isolation()?;
        if boot == old {
            return Ok(());
        }
        let mut next = self.confirmed;
        image::interrupt::<R, N>(&mut next, boot)?;
        self.persist(next, image::revision(&self.confirmed))
    }
    fn persist(&mut self, next: [u8; N], expected: u64) -> Result<(), Error> {
        let length = image::length::<R, N>()?;
        let revision = expected.checked_add(1).ok_or(Error::Storage)?;
        image::validate::<R, N>(&next, revision)?;
        match self.store.commit(expected, &next[..length]) {
            Ok(receipt) if receipt.revision == revision && receipt.length == length => {
                self.confirmed = next;
                self.initialized = true;
                Ok(())
            }
            Err(StorageError::Capacity) => Err(Error::Capacity),
            Err(StorageError::Conflict) => Err(Error::Conflict),
            Err(StorageError::RevisionExhausted) => Err(Error::Storage),
            _ => {
                self.poisoned = true;
                Err(Error::UnknownOutcome)
            }
        }
    }
}
