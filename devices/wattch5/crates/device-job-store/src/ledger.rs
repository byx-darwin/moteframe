//! Existing service-trait adapter, retaining full principal-scoped immutable bindings.
use crate::{JobStore, ResetIsolation, TrustedBoot, TrustedJobPolicy, image};
use device_maintenance_service::{
    DeviceJobAccess, DeviceJobClaim, DeviceJobGuarantees, DeviceJobLedger, DeviceJobRecord,
    DeviceJobState as State, DeviceJobTerminal, Durable, Error, ExecutionClaim, NamedRequest,
};
use device_storage::persistence::SnapshotStore;

impl<
    S: SnapshotStore,
    B: TrustedBoot,
    P: TrustedJobPolicy,
    I: ResetIsolation,
    const R: usize,
    const N: usize,
> JobStore<S, B, P, I, R, N>
{
    fn find(
        &self,
        id: [u8; 16],
        principal: [u8; 16],
    ) -> Result<Option<(usize, DeviceJobRecord)>, Error> {
        for index in 0..image::count(&self.confirmed) {
            let r = image::record(image::row(&self.confirmed, index))?;
            if r.operation_id == id && r.principal == principal {
                return Ok(Some((index, r)));
            }
        }
        Ok(None)
    }
    fn authorize(
        &self,
        a: &dyn DeviceJobAccess,
        principal: [u8; 16],
        op: u16,
        revision: u16,
    ) -> Result<(), Error> {
        self.ready()?;
        if a.identity() != Some(principal) || principal == [0; 16] || !a.start_allowed(op) {
            return Err(Error::Denied);
        }
        if !a.dangerous_allowed() {
            return Err(Error::DangerousDenied);
        }
        if !matches!(op, 1 | 2) || !self.policy.supports(op) {
            return Err(Error::Unsupported);
        }
        if self.policy.current_revision()? == 0 {
            return Err(Error::Unavailable);
        }
        if revision == 0 || !self.policy.permits(principal, op, revision) {
            return Err(Error::Denied);
        }
        if a.identity() != Some(principal) || !a.start_allowed(op) {
            return Err(Error::Denied);
        }
        if !a.dangerous_allowed() {
            return Err(Error::DangerousDenied);
        }
        self.ready()
    }
    fn query_scope(a: &dyn DeviceJobAccess, principal: [u8; 16]) -> Result<(), Error> {
        if principal == [0; 16] || a.identity() != Some(principal) || !a.read_scope() {
            Err(Error::Denied)
        } else {
            Ok(())
        }
    }
}
impl<
    S: SnapshotStore,
    B: TrustedBoot,
    P: TrustedJobPolicy,
    I: ResetIsolation,
    const R: usize,
    const N: usize,
> DeviceJobLedger for JobStore<S, B, P, I, R, N>
{
    fn guarantees(&self) -> DeviceJobGuarantees {
        self.read_operation(|owner| {
            Ok({
                let ready = if owner.ready().is_ok() {
                    Durable::Implemented
                } else {
                    Durable::Unavailable
                };
                DeviceJobGuarantees {
                    atomic_identity: ready,
                    durable_identity: ready,
                    durable_terminal: ready,
                    current_boot_recovered: ready,
                    reset_preserves_ledger: if owner.check_isolation().is_ok() {
                        Durable::Implemented
                    } else {
                        Durable::Unavailable
                    },
                    retained_terminal: u16::try_from(R).unwrap_or(0),
                }
            })
        })
        .unwrap_or(DeviceJobGuarantees {
            atomic_identity: Durable::Unavailable,
            durable_identity: Durable::Unavailable,
            durable_terminal: Durable::Unavailable,
            current_boot_recovered: Durable::Unavailable,
            reset_preserves_ledger: Durable::Unavailable,
            retained_terminal: 0,
        })
    }
    fn available(&self, operation: u16) -> bool {
        self.read_operation(|owner| {
            Ok({
                owner.ready().is_ok()
                    && matches!(operation, 1 | 2)
                    && owner.policy.supports(operation)
                    && owner.policy.current_revision().is_ok_and(|v| v != 0)
            })
        })
        .unwrap_or(false)
    }
    fn claim_atomic(
        &mut self,
        request: NamedRequest<'_>,
        access: &dyn DeviceJobAccess,
    ) -> Result<DeviceJobClaim, Error> {
        self.write_operation(|owner| {
            owner.ready()?;
            let principal = access
                .identity()
                .filter(|id| *id != [0; 16])
                .ok_or(Error::Denied)?;
            if request.operation_id == [0; 16] {
                return Err(Error::Invalid);
            }
            if let Some((_, record)) = owner.find(request.operation_id, principal)? {
                owner.authorize(access, principal, record.operation, record.policy_revision)?;
                if record.operation != request.operation || request.params != [1, 0] {
                    return Err(Error::Conflict);
                }
                owner.authorize(access, principal, record.operation, record.policy_revision)?;
                return Ok(DeviceJobClaim {
                    record,
                    replayed: true,
                });
            }
            if !matches!(request.operation, 1 | 2) {
                return Err(Error::Unsupported);
            }
            let policy_revision = owner.policy.current_revision()?;
            owner.authorize(access, principal, request.operation, policy_revision)?;
            if request.params != [1, 0] {
                return Err(Error::Invalid);
            }
            let count = image::count(&owner.confirmed);
            if count == R {
                return Err(Error::Capacity);
            }
            let job = u32::try_from(count)
                .map_err(|_| Error::Capacity)?
                .checked_add(1)
                .ok_or(Error::Capacity)?;
            let record = DeviceJobRecord {
                operation_id: request.operation_id,
                operation: request.operation,
                job,
                principal,
                policy_revision,
                state: State::Accepted,
                result: 0,
                reason: 0,
            };
            let mut next = owner.confirmed;
            image::append(&mut next, record)?;
            owner.authorize(access, principal, request.operation, policy_revision)?;
            owner.persist(next, image::revision(&owner.confirmed))?;
            owner.authorize(access, principal, request.operation, policy_revision)?;
            Ok(DeviceJobClaim {
                record,
                replayed: false,
            })
        })
    }
    fn query(&self, id: [u8; 16], access: &dyn DeviceJobAccess) -> Result<DeviceJobRecord, Error> {
        self.read_operation(|owner| {
            owner.ready()?;
            let principal = access.identity().ok_or(Error::Denied)?;
            Self::query_scope(access, principal)?;
            if id == [0; 16] {
                return Err(Error::Invalid);
            }
            let record = if let Some((_, own)) = owner.find(id, principal)? {
                if !access.may_read(own) {
                    return Err(Error::Denied);
                }
                own
            } else {
                let mut selected = None;
                for index in 0..image::count(&owner.confirmed) {
                    let r = image::record(image::row(&owner.confirmed, index))?;
                    if r.operation_id != id {
                        continue;
                    }
                    let eligible = access.may_read(r);
                    Self::query_scope(access, principal)?;
                    owner.ready()?;
                    if eligible {
                        if selected.is_some() {
                            return Err(Error::Conflict);
                        }
                        selected = Some(r);
                    }
                }
                selected.ok_or(Error::NotFound)?
            };
            Self::query_scope(access, principal)?;
            if !access.may_read(record) {
                return Err(Error::Denied);
            }
            Self::query_scope(access, principal)?;
            owner.ready()?;
            Ok(record)
        })
    }
    fn begin_execution_atomic(
        &mut self,
        record: DeviceJobRecord,
        access: &dyn DeviceJobAccess,
    ) -> Result<ExecutionClaim, Error> {
        self.write_operation(|owner| {
            owner.ready()?;
            let (index, current) = owner
                .find(record.operation_id, record.principal)?
                .ok_or(Error::NotFound)?;
            if !image::binding(record, current) {
                return Err(Error::Conflict);
            }
            owner.authorize(
                access,
                current.principal,
                current.operation,
                current.policy_revision,
            )?;
            if current.state != State::Accepted {
                return Ok(ExecutionClaim::AlreadyStarted(current));
            }
            let mut next = owner.confirmed;
            image::start(&mut next, index)?;
            owner.authorize(
                access,
                current.principal,
                current.operation,
                current.policy_revision,
            )?;
            owner.persist(next, image::revision(&owner.confirmed))?;
            owner.authorize(
                access,
                current.principal,
                current.operation,
                current.policy_revision,
            )?;
            Ok(ExecutionClaim::Acquired(image::record(image::row(
                &owner.confirmed,
                index,
            ))?))
        })
    }
    fn execution_record(&self, id: [u8; 16]) -> Result<DeviceJobRecord, Error> {
        self.read_operation(|owner| {
            owner.ready()?;
            if id == [0; 16] {
                return Err(Error::Invalid);
            }
            let mut selected = None;
            for index in 0..image::count(&owner.confirmed) {
                let r = image::record(image::row(&owner.confirmed, index))?;
                if r.operation_id == id {
                    if selected.is_some() {
                        return Err(Error::Conflict);
                    }
                    selected = Some(r);
                }
            }
            selected.ok_or(Error::NotFound)
        })
    }
    fn execution_record_bound(&self, record: DeviceJobRecord) -> Result<DeviceJobRecord, Error> {
        self.read_operation(|owner| {
            owner.ready()?;
            let (_, current) = owner
                .find(record.operation_id, record.principal)?
                .ok_or(Error::NotFound)?;
            if !image::binding(record, current) {
                return Err(Error::Conflict);
            }
            Ok(current)
        })
    }
    fn finish_atomic(
        &mut self,
        record: DeviceJobRecord,
        terminal: DeviceJobTerminal,
    ) -> Result<DeviceJobRecord, Error> {
        self.write_operation(|owner| {
            owner.ready()?;
            if !image::valid_terminal(terminal) {
                return Err(Error::Invalid);
            }
            let (index, current) = owner
                .find(record.operation_id, record.principal)?
                .ok_or(Error::NotFound)?;
            if !image::binding(record, current) {
                return Err(Error::Conflict);
            }
            if matches!(
                current.state,
                State::Succeeded | State::Failed | State::Interrupted
            ) {
                if current.state == terminal.state
                    && current.result == terminal.result
                    && current.reason == terminal.reason
                {
                    return Ok(current);
                }
                return Err(Error::Conflict);
            }
            if current.state != State::Running || record.state != State::Running {
                return Err(Error::Conflict);
            }
            let mut next = owner.confirmed;
            image::finish(&mut next, index, terminal)?;
            owner.ready()?;
            owner.persist(next, image::revision(&owner.confirmed))?;
            owner.ready()?;
            image::record(image::row(&owner.confirmed, index))
        })
    }
}
