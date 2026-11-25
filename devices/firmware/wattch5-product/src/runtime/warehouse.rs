//! Explicit selected-owner recovery; no formatting, backend activation or DATA ownership.
use device_property_store::{DefaultValue, PropertySchema, PropertyStore};
use device_runtime::PortError;
use device_storage::persistence::SnapshotStore;
/// Explicit blank-settings startup policy. Corrupt storage never enters this path.
#[derive(Debug, Clone, Copy)]
pub enum BlankSettings<'a> {
    /// Preserve confirmed blank storage as unavailable.
    KeepBlank,
    /// Persist these complete caller-selected defaults only on confirmed blank storage.
    Initialize(&'a [DefaultValue<'a>]),
}
/// Apply an explicit startup decision to an already strictly opened property owner.
/// # Errors
/// Existing, corrupt, failed and uncertain stores are never silently initialized.
pub fn initialize_blank_settings<S: SnapshotStore, C: PropertySchema, const N: usize>(
    owner: &PropertyStore<'_, S, C, N>,
    policy: BlankSettings<'_>,
) -> Result<(), PortError> {
    match policy {
        BlankSettings::KeepBlank => Ok(()),
        BlankSettings::Initialize(defaults) => {
            if owner.revision() != Err(device_property_store::Error::Uninitialized) {
                return Err(PortError::InvalidInput);
            }
            owner.initialize(defaults).map_err(|_| PortError::Fault)
        }
    }
}
mod sealed {
    pub trait Sealed {}
}
/// Actual existing owner recovery, sealed to the six supported durable owner families.
pub trait RecoverOwner: sealed::Sealed {
    /// Invoke its real recovery method after all connection actors have closed.
    /// # Errors
    /// Medium, schema, identity and continuity failures remain explicit.
    fn recover_owner(&mut self) -> Result<(), PortError>;
}
/// Shared configuration owner family; sealed through `RecoverOwner`.
pub trait SharedConfigRecovery: RecoverOwner {}
/// Ordinary property owner family; sealed through `RecoverOwner`.
pub trait PropertiesRecovery: RecoverOwner {}
/// Object owner family; sealed through `RecoverOwner`.
pub trait ObjectsRecovery: RecoverOwner {}
/// History and ACK owner family; sealed through `RecoverOwner`.
pub trait HistoryRecovery: RecoverOwner {}
/// Durable UTC owner family; sealed through `RecoverOwner`.
pub trait TimeRecovery: RecoverOwner {}
/// Durable job owner family; sealed through `RecoverOwner`.
pub trait JobsRecovery: RecoverOwner {}
/// Independent recovery outcomes; success means reloaded, not initialized or wire-available.
#[derive(Debug, Clone, Copy)]
pub struct RecoveryReport {
    /// Shared heart/sport configuration owner.
    pub shared_config: Result<(), PortError>,
    /// Ordinary properties.
    pub properties: Result<(), PortError>,
    /// Objects and their persistent identity reservations.
    pub objects: Result<(), PortError>,
    /// History records and durable ACK watermarks, under their one existing owner.
    pub history_ack: Result<(), PortError>,
    /// UTC continuity evidence.
    pub time: Result<(), PortError>,
    /// Retained jobs and actual boot reconciliation.
    pub jobs: Result<(), PortError>,
}
impl RecoveryReport {
    /// Whether every selected owner actually recovered; does not claim module availability.
    #[must_use]
    pub fn all_recovered(self) -> bool {
        [
            self.shared_config,
            self.properties,
            self.objects,
            self.history_ack,
            self.time,
            self.jobs,
        ]
        .iter()
        .all(Result::is_ok)
    }
}
/// Named coordinator borrowing all six independently owned partitions.
#[derive(Debug)]
pub struct SelectedWarehouse<'a, C, P, O, H, T, J> {
    owners: (
        &'a mut C,
        &'a mut P,
        &'a mut O,
        &'a mut H,
        &'a mut T,
        &'a mut J,
    ),
}
impl<
    'a,
    C: SharedConfigRecovery,
    P: PropertiesRecovery,
    O: ObjectsRecovery,
    H: HistoryRecovery,
    T: TimeRecovery,
    J: JobsRecovery,
> SelectedWarehouse<'a, C, P, O, H, T, J>
{
    /// Select actual external owners. Their strict open and explicit blank initialization precede this.
    pub fn new(
        owners: (
            &'a mut C,
            &'a mut P,
            &'a mut O,
            &'a mut H,
            &'a mut T,
            &'a mut J,
        ),
    ) -> Self {
        Self { owners }
    }
    /// Reconcile selected existing owners at startup, without selecting wire modules or formatting.
    /// Application sessions must not yet exist.
    pub fn boot_existing(&mut self) -> RecoveryReport {
        self.recover_after_shutdown()
    }
    /// Reload every owner independently after explicit `Runtime::shutdown` and dropping old actors.
    pub fn recover_after_shutdown(&mut self) -> RecoveryReport {
        RecoveryReport {
            shared_config: self.owners.0.recover_owner(),
            properties: self.owners.1.recover_owner(),
            objects: self.owners.2.recover_owner(),
            history_ack: self.owners.3.recover_owner(),
            time: self.owners.4.recover_owner(),
            jobs: self.owners.5.recover_owner(),
        }
    }
}
impl<S: SnapshotStore> sealed::Sealed for device_config_store::ConfigStore<'_, S> {}
impl<S: SnapshotStore> RecoverOwner for device_config_store::ConfigStore<'_, S> {
    fn recover_owner(&mut self) -> Result<(), PortError> {
        self.recover_and_reload().map_err(|_| PortError::Fault)
    }
}
impl<S: SnapshotStore, C: PropertySchema, const N: usize> sealed::Sealed
    for PropertyStore<'_, S, C, N>
{
}
impl<S: SnapshotStore, C: PropertySchema, const N: usize> RecoverOwner
    for PropertyStore<'_, S, C, N>
{
    fn recover_owner(&mut self) -> Result<(), PortError> {
        self.recover_and_reload().map_err(|_| PortError::Fault)
    }
}
impl<
    S: SnapshotStore,
    V: device_storage::ObjectSchema,
    const M: usize,
    const E: usize,
    const B: usize,
    const I: usize,
> sealed::Sealed for device_object_store::DurableObjects<S, V, M, E, B, I>
{
}
impl<
    S: SnapshotStore,
    V: device_storage::ObjectSchema,
    const M: usize,
    const E: usize,
    const B: usize,
    const I: usize,
> RecoverOwner for device_object_store::DurableObjects<S, V, M, E, B, I>
{
    fn recover_owner(&mut self) -> Result<(), PortError> {
        self.recover().map_err(|_| PortError::Fault)
    }
}
impl<
    S: SnapshotStore,
    V: device_history_store::HistorySchema,
    C: device_history_store::HistoryClock,
    const D: usize,
    const R: usize,
    const B: usize,
    const I: usize,
> sealed::Sealed for device_history_store::HistoryStore<S, V, C, D, R, B, I>
{
}
impl<
    S: SnapshotStore,
    V: device_history_store::HistorySchema,
    C: device_history_store::HistoryClock,
    const D: usize,
    const R: usize,
    const B: usize,
    const I: usize,
> RecoverOwner for device_history_store::HistoryStore<S, V, C, D, R, B, I>
{
    fn recover_owner(&mut self) -> Result<(), PortError> {
        self.recover().map_err(|_| PortError::Fault)
    }
}
impl<
    S: SnapshotStore,
    L: device_storage::persistence::Medium,
    V: device_history_store::HistorySchema,
    C: device_history_store::HistoryClock,
    const D: usize,
    const R: usize,
    const B: usize,
    const N: usize,
> sealed::Sealed for device_history_store::paged::PagedHistoryStore<S, L, V, C, D, R, B, N>
{
}
impl<
    S: SnapshotStore,
    L: device_storage::persistence::Medium,
    V: device_history_store::HistorySchema,
    C: device_history_store::HistoryClock,
    const D: usize,
    const R: usize,
    const B: usize,
    const N: usize,
> RecoverOwner for device_history_store::paged::PagedHistoryStore<S, L, V, C, D, R, B, N>
{
    fn recover_owner(&mut self) -> Result<(), PortError> {
        self.recover().map_err(|_| PortError::Fault)
    }
}
impl<S: SnapshotStore, C: device_time_store::TrustedClock> sealed::Sealed
    for device_time_store::TimeStore<S, C>
{
}
impl<S: SnapshotStore, C: device_time_store::TrustedClock> RecoverOwner
    for device_time_store::TimeStore<S, C>
{
    fn recover_owner(&mut self) -> Result<(), PortError> {
        self.recover_and_reload().map_err(|_| PortError::Fault)
    }
}
impl<
    S: SnapshotStore,
    B: device_job_store::TrustedBoot,
    P: device_job_store::TrustedJobPolicy,
    I: device_job_store::ResetIsolation,
    const R: usize,
    const N: usize,
> sealed::Sealed for device_job_store::JobStore<S, B, P, I, R, N>
{
}
impl<
    S: SnapshotStore,
    B: device_job_store::TrustedBoot,
    P: device_job_store::TrustedJobPolicy,
    I: device_job_store::ResetIsolation,
    const R: usize,
    const N: usize,
> RecoverOwner for device_job_store::JobStore<S, B, P, I, R, N>
{
    fn recover_owner(&mut self) -> Result<(), PortError> {
        self.recover_and_reload().map_err(|_| PortError::Fault)
    }
}

impl<S: SnapshotStore> SharedConfigRecovery for device_config_store::ConfigStore<'_, S> {}
impl<S: SnapshotStore, C: PropertySchema, const N: usize> PropertiesRecovery
    for PropertyStore<'_, S, C, N>
{
}
impl<
    S: SnapshotStore,
    V: device_storage::ObjectSchema,
    const M: usize,
    const E: usize,
    const B: usize,
    const I: usize,
> ObjectsRecovery for device_object_store::DurableObjects<S, V, M, E, B, I>
{
}
impl<
    S: SnapshotStore,
    V: device_history_store::HistorySchema,
    C: device_history_store::HistoryClock,
    const D: usize,
    const R: usize,
    const B: usize,
    const I: usize,
> HistoryRecovery for device_history_store::HistoryStore<S, V, C, D, R, B, I>
{
}
impl<
    S: SnapshotStore,
    L: device_storage::persistence::Medium,
    V: device_history_store::HistorySchema,
    C: device_history_store::HistoryClock,
    const D: usize,
    const R: usize,
    const B: usize,
    const N: usize,
> HistoryRecovery for device_history_store::paged::PagedHistoryStore<S, L, V, C, D, R, B, N>
{
}
impl<S: SnapshotStore, C: device_time_store::TrustedClock> TimeRecovery
    for device_time_store::TimeStore<S, C>
{
}
impl<
    S: SnapshotStore,
    B: device_job_store::TrustedBoot,
    P: device_job_store::TrustedJobPolicy,
    I: device_job_store::ResetIsolation,
    const R: usize,
    const N: usize,
> JobsRecovery for device_job_store::JobStore<S, B, P, I, R, N>
{
}
