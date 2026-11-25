//! Named persistent-job admission; no Flash/physical executor is supplied.
use crate::{Authority, Error, Profile};
/// Immutable caller operation identity and complete versioned parameters.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct NamedOperation<'a> {
    /// Module72 or73, selected by the corresponding crate feature.
    pub module: u8,
    /// Nonzero named operation from the V0.6 proposal, never an old0F command.
    pub operation: u16,
    /// Caller-persisted nonzero operation identity.
    pub operation_id: [u8; 16],
    /// Exact complete parameters beginning with schema:u16LE=1.
    pub params: &'a [u8],
}
impl core::fmt::Debug for NamedOperation<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NamedOperation")
            .field("module", &self.module)
            .field("operation", &self.operation)
            .field("parameter_length", &self.params.len())
            .finish_non_exhaustive()
    }
}
/// Controlled product schema decision; cannot relax mandatory dangerous scopes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationPolicy {
    /// Nonzero product schema/selector/criterion policy revision.
    pub revision: u16,
    /// Additional dangerous-write permission requirement.
    pub dangerous: bool,
}
/// Actual product selector/range/schema validator, with no default permissive schema.
pub trait OperationSchema {
    /// Check actual supported IDs, native ranges, criterion and recovery applicability.
    /// # Errors
    /// Missing real schema is Unavailable; unsupported/invalid values must be explicit.
    fn validate(&self, operation: NamedOperation<'_>) -> Result<OperationPolicy, Error>;
    /// Validate actual persisted output encoding using its original product policy.
    /// Missing output schema is unavailable, never a permissive default.
    /// # Errors
    /// Returns unavailable or explicit format/range/criterion mismatch.
    fn validate_output(
        &self,
        _module: u8,
        _operation: u16,
        _policy_revision: u16,
        _bytes: &[u8],
    ) -> Result<(), Error> {
        Err(Error::Unavailable)
    }
}
/// Trusted implementation status of one mandatory backend obligation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Guarantee {
    /// No actual implementation/acceptance; admission is unavailable.
    Unavailable,
    /// Actual integration asserts this obligation is implemented and verified.
    Implemented,
}
/// Mandatory actual durable-ledger guarantees. Fields are trusted integration assertions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Guarantees {
    /// Exact operation-ID/bytes comparison and initial job allocation are atomic.
    pub atomic_claim: Guarantee,
    /// Operation IDs and immutable bindings survive normal reboot/reset.
    pub durable_identity: Guarantee,
    /// Actual terminal results are persisted before exposure.
    pub durable_terminal: Guarantee,
    /// Real current-boot recovery already marked unresolved old jobs interrupted.
    pub recovery_ready: Guarantee,
    /// At least32 unacknowledged terminal records plus dedup tombstones are retained.
    pub retained_terminal_records: u16,
}
/// V0.6 proposed named-job states, deliberately separate from self-test0..7.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistentState {
    /// Durable admission only, no success/effect implied.
    Accepted = 1,
    /// Actual executor reports work running.
    Running = 2,
    /// Actual verified outcome durably succeeded.
    Succeeded = 3,
    /// Actual error durably failed.
    Failed = 4,
    /// Real reboot recovery found an unresolved operation; never automatically replayed.
    Interrupted = 5,
}
/// Actual durable record; IDs/outcomes cannot be claimed by phone payload.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct PersistentRecord {
    /// Original nonzero stable operation identity.
    pub operation_id: [u8; 16],
    /// Owning module.
    pub module: u8,
    /// Original named operation.
    pub operation: u16,
    /// Parameter schema1.
    pub schema: u16,
    /// Persistent nonzero job identity allocated by the ledger.
    pub job_id: u32,
    /// Original authenticated station, not request-granted identity.
    pub station_id: u32,
    /// Bound product validation revision.
    pub policy_revision: u16,
    /// Actual persistent state.
    pub state: PersistentState,
    /// Existing core result number; nonterminal zero has no terminal-success meaning.
    pub result: u16,
    /// Operation-specific actual reason.
    pub reason: u16,
}
impl core::fmt::Debug for PersistentRecord {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PersistentRecord")
            .field("module", &self.module)
            .field("operation", &self.operation)
            .field("job_id", &self.job_id)
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}
/// Ledger admission result, distinguishing a new reservation from an exact replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Claim {
    /// Actual persisted record.
    pub record: PersistentRecord,
    /// True only after exact full immutable identity/parameter comparison.
    pub replayed: bool,
}
/// Metadata for actual persisted operation output in caller-owned storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputMeta {
    /// Original operation number.
    pub operation: u16,
    /// Original parameter schema.
    pub schema: u16,
    /// Actual initialized output length; zero only for a declared no-output operation.
    pub length: usize,
}
/// Real durable ledger/executor boundary; no production memory or fake executor exists.
/// Exact operation ID, module, operation, schema and every parameter byte must be
/// compared atomically, not just a collision-prone checksum. New claim persists
/// admission before response and MUST NOT perform the dangerous operation. An
/// actual external executor may begin only after successful response admission.
/// Reboot recovery never replays unresolved work: it persists Interrupted with an
/// actual existing error. Normal reset preserves the separate operation ledger.
/// At least32 unacknowledged terminal records and tombstones must be retained;
/// reject full storage instead of evicting IDs. Factory reset cannot erase it.
pub trait StorageBackend {
    /// Report actually implemented durable/recovery guarantees.
    fn guarantees(&self) -> Guarantees;
    /// Actual executor and operation-specific recovery must exist before admission.
    fn available(&self, module: u8, operation: u16) -> bool;
    /// Atomically revalidate schema and live authority, compare full binding and persist.
    /// SameID+identical bytes returns the original job; any immutable difference conflicts.
    /// Preserve terminal states; recheck authority within the commit serialization boundary.
    /// # Errors
    /// Capacity/conflict/denied/backend/unknown outcomes must not be returned as success.
    fn claim_atomic(
        &mut self,
        operation: NamedOperation<'_>,
        policy: OperationPolicy,
        schema: &dyn OperationSchema,
        authority: &dyn Authority,
    ) -> Result<Claim, Error>;
    /// Read actual persistent state under the current authenticated station/scope.
    /// # Errors
    /// Unknown IDs are `NotFound`; missing recovery/backend is Unavailable; do not synthesize success.
    fn query(
        &self,
        module: u8,
        operation_id: [u8; 16],
        authority: &dyn Authority,
    ) -> Result<PersistentRecord, Error>;
    /// Read actual verified persisted output after terminal outcome, never echo params as proof.
    /// # Errors
    /// Nonterminal/conflicting/missing output and short buffers must be explicit.
    fn output(
        &self,
        module: u8,
        operation_id: [u8; 16],
        out: &mut [u8],
        authority: &dyn Authority,
    ) -> Result<OutputMeta, Error>;
}
/// Authorized schema-checked admission/query facade, with no local dedup or executor.
/// Durability and actual reboot outcomes remain mandatory real backend obligations.
pub struct PersistentEngine<'a> {
    backend: &'a mut dyn StorageBackend,
    schema: &'a dyn OperationSchema,
    auth: &'a dyn Authority,
    closed: bool,
}
impl core::fmt::Debug for PersistentEngine<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PersistentEngine").finish_non_exhaustive()
    }
}
impl<'a> PersistentEngine<'a> {
    /// Construct with actual durable and recovery-ready backend guarantees.
    /// # Errors
    /// Rejects commercial policy, missing ports or unimplemented guarantees.
    pub fn new(
        profile: Profile,
        backend: Option<&'a mut dyn StorageBackend>,
        schema: &'a dyn OperationSchema,
        auth: &'a dyn Authority,
    ) -> Result<Self, Error> {
        if profile == Profile::Commercial {
            return Err(Error::Commercial);
        }
        if auth.station_id().is_none_or(|id| id == 0) {
            return Err(Error::Denied);
        }
        let backend = backend.ok_or(Error::Unavailable)?;
        check_guarantees(backend.guarantees())?;
        Ok(Self {
            backend,
            schema,
            auth,
            closed: false,
        })
    }
    /// Persist a new named operation or recover its exact original deduplicated job.
    /// # Errors
    /// All scope/schema/identity/capacity/backend failures remain explicit.
    pub fn submit(&mut self, operation: NamedOperation<'_>) -> Result<Claim, Error> {
        self.access(operation.module, false)?;
        validate_params(operation)?;
        let policy = self.schema.validate(operation)?;
        if policy.revision == 0 {
            return Err(Error::Unavailable);
        }
        let dangerous =
            policy.dangerous || requires_dangerous(operation.module, operation.operation);
        self.access(operation.module, dangerous)?;
        check_guarantees(self.backend.guarantees())?;
        if !self
            .backend
            .available(operation.module, operation.operation)
        {
            return Err(Error::Unavailable);
        }
        let claimed = self
            .backend
            .claim_atomic(operation, policy, self.schema, self.auth);
        self.access(operation.module, dangerous)?;
        let claim = claimed?;
        self.validate_record(operation.module, operation.operation_id, claim.record)?;
        if claim.record.operation != operation.operation
            || (!claim.replayed
                && (claim.record.state != PersistentState::Accepted
                    || claim.record.policy_revision != policy.revision))
        {
            return Err(Error::Backend);
        }
        Ok(claim)
    }
    /// Read an actual persistent state, not the self-test state dictionary.
    /// # Errors
    /// Unknown, unauthorized, unsupported or invalid backend outcomes are rejected.
    pub fn query(&mut self, module: u8, id: [u8; 16]) -> Result<PersistentRecord, Error> {
        self.access(module, false)?;
        if id == [0; 16] {
            return Err(Error::Invalid);
        }
        check_guarantees(self.backend.guarantees())?;
        let record = self.backend.query(module, id, self.auth);
        self.access(module, false)?;
        let record = record?;
        self.validate_record(module, id, record)?;
        Ok(record)
    }
    /// Read actual persisted terminal output; no-output needs explicit backend semantics.
    /// # Errors
    /// Rejects nonterminal/unknown work or undefined output schemas and clears output on errors.
    pub fn output(
        &mut self,
        module: u8,
        id: [u8; 16],
        out: &mut [u8],
    ) -> Result<OutputMeta, Error> {
        let result = (|| {
            let record = self.query(module, id)?;
            if matches!(
                record.state,
                PersistentState::Accepted | PersistentState::Running
            ) {
                return Err(Error::Conflict);
            }
            let output = self.backend.output(module, id, out, self.auth);
            self.access(module, false)?;
            let output = output?;
            if output.operation != record.operation
                || output.schema != record.schema
                || output.length > out.len()
            {
                return Err(Error::Backend);
            }
            self.schema.validate_output(
                module,
                record.operation,
                record.policy_revision,
                &out[..output.length],
            )?;
            self.access(module, false)?;
            Ok(output)
        })();
        if result.is_err() {
            out.fill(0);
        }
        result
    }
    fn access(&mut self, module: u8, dangerous: bool) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        let capability = match module {
            0x72 if cfg!(feature = "engineering") => crate::Capability::Engineering,
            0x73 if cfg!(feature = "factory-config") => crate::Capability::FactoryConfig,
            _ => return Err(Error::Unsupported),
        };
        if !self.auth.allows(capability)
            || self.auth.station_id().is_none_or(|id| id == 0)
            || (dangerous && !self.auth.allows(crate::Capability::DangerousMaintenance))
        {
            self.closed = true;
            return Err(Error::Denied);
        }
        Ok(())
    }
    fn validate_record(
        &self,
        module: u8,
        id: [u8; 16],
        record: PersistentRecord,
    ) -> Result<(), Error> {
        if record.operation_id != id
            || record.module != module
            || record.schema != 1
            || record.job_id == 0
            || record.policy_revision == 0
            || Some(record.station_id) != self.auth.station_id()
            || !known_operation(module, record.operation)
        {
            return Err(Error::Backend);
        }
        if record.result > 0x12 || record.result == 0x10 {
            return Err(Error::Backend);
        }
        let valid = match record.state {
            PersistentState::Accepted | PersistentState::Running => record.result == 0,
            PersistentState::Succeeded => record.result == 0 && record.reason == 0,
            PersistentState::Failed | PersistentState::Interrupted => record.result != 0,
        };
        if !valid {
            return Err(Error::Backend);
        }
        Ok(())
    }
}

fn check_guarantees(g: Guarantees) -> Result<(), Error> {
    if g.atomic_claim != Guarantee::Implemented
        || g.durable_identity != Guarantee::Implemented
        || g.durable_terminal != Guarantee::Implemented
        || g.recovery_ready != Guarantee::Implemented
        || g.retained_terminal_records < 32
    {
        Err(Error::Unavailable)
    } else {
        Ok(())
    }
}
fn known_operation(module: u8, operation: u16) -> bool {
    match module {
        0x72 => (1..=12).contains(&operation),
        0x73 => (1..=5).contains(&operation),
        _ => false,
    }
}
fn requires_dangerous(module: u8, operation: u16) -> bool {
    module == 0x73 || (module == 0x72 && matches!(operation, 1 | 2 | 7 | 8 | 10 | 11))
}
fn u16_at(bytes: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([bytes[at], bytes[at + 1]])
}
fn u32_at(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}
fn validate_params(op: NamedOperation<'_>) -> Result<(), Error> {
    if op.operation_id == [0; 16]
        || op.params.len() < 2
        || op.params.len() > 492
        || u16_at(op.params, 0) != 1
        || !known_operation(op.module, op.operation)
    {
        return Err(Error::Invalid);
    }
    let length = match (op.module, op.operation) {
        (0x72, 1 | 2 | 8) | (0x73, 1 | 3) => 4,
        (0x72, 3 | 5 | 6 | 7) => 6,
        (0x72, 4) => 10,
        (0x72, 9) => 12,
        (0x72, 10 | 11) => 7,
        (0x72, 12) | (0x73, 5) => 3,
        (0x73, 4) => 5,
        (0x73, 2) => {
            if op.params.len() < 5 {
                return Err(Error::Invalid);
            }
            let n = usize::from(u16_at(op.params, 2));
            if !(1..=32).contains(&n)
                || op.params.len() != 4 + n
                || !op.params[4..].iter().all(u8::is_ascii_digit)
            {
                return Err(Error::Invalid);
            }
            return Ok(());
        }
        _ => return Err(Error::Unsupported),
    };
    if op.params.len() != length {
        return Err(Error::Invalid);
    }
    let valid = match (op.module, op.operation) {
        (0x72, 4) => u32_at(op.params, 6) > 0,
        (0x72, 5 | 6) => u16_at(op.params, 4) > 0,
        (0x72, 9) => u32_at(op.params, 4) > 0 && u32_at(op.params, 8) > 0,
        (0x72, 10) => match op.params[2] {
            1 => u32_at(op.params, 3) > 0,
            2 => u32_at(op.params, 3) == 0,
            _ => false,
        },
        (0x72, 11) => matches!(op.params[2], 1 | 2) && u32_at(op.params, 3) > 0,
        (0x72, 12) => op.params[2] <= 2,
        (0x73, 4) => op.params[4] <= 1,
        (0x73, 5) => op.params[2] <= 1,
        _ => true,
    };
    if !valid {
        return Err(Error::Invalid);
    }
    Ok(())
}
