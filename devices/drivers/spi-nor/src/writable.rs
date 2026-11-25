//! Bounded caller-attested SPI NOR operations. No board is qualified by this API.
//!
//! All handles are foreground only. One externally owned chip retains the device
//! while a snapshot engine owns its non-clone partition handle. Recovery Sync
//! settles the chip; a separate trusted coordinator attests bank/owner recovery.
use core::{cell::RefCell, fmt, task::Poll};
use device_storage::{
    incremental::{AsyncMedium, Completion, MediumOp, StartError},
    persistence::{Error, Geometry},
};
use embedded_hal::spi::{Operation, SpiDevice};

/// Prompt monotonic millisecond source, continuously advanced by integration.
/// A stalled source cannot prove elapsed-time bounds; wrap/regression is an error.
pub trait Monotonic {
    /// Read without sleeping, spinning, invoking the scheduler or using this SPI.
    fn now_ms(&mut self) -> u64;
}
/// Explicit diagnostics; none of these imply that a mutation was rolled back.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Fault {
    /// Structurally inconsistent or unsupported profile.
    Profile,
    /// Missing caller evidence identifier; identifiers do not prove the evidence.
    Attestation,
    /// Invalid or nonexclusive declared partition.
    Partition,
    /// Another handle/operation owns the chip or its checked borrow.
    Busy,
    /// Partition writer was already claimed, including a dropped handle.
    Claimed,
    /// Recovery, settled chip state or owner validation is incomplete.
    NeedsRecovery,
    /// Range is outside the granted storage/asset region or chunk bound.
    Bounds,
    /// Program/erase alignment or exact unit is invalid.
    Alignment,
    /// Observed status does not match caller-attested protection state.
    Protection,
    /// WEL does not match the expected qualified armed/settled state.
    WriteEnable,
    /// Clock regressed before or after a transaction; physical effects may remain.
    ClockRegression,
    /// Deadline arithmetic overflowed.
    DeadlineOverflow,
    /// An admitted or issued command exceeded its configured deadline.
    Deadline,
    /// SPI transaction failed and may have had physical effects.
    Transport,
    /// Normal readback disagrees with expected bytes or a0-to1 request was made.
    Readback,
    /// Actual JEDEC bytes disagree with the attested profile.
    Identity,
    /// Poll output is too short or no operation was admitted.
    Completion,
    /// Recovery token/settlement/order is invalid.
    RecoveryOrder,
}
/// Caller-supplied conventional 05/06/02/20/03 protocol configuration.
/// Values must come from actual qualification; this structure proves no hardware facts.
#[derive(Clone, Copy, Debug)]
pub struct ProfileConfig {
    /// Exact expected JEDEC bytes, not an inferred part number.
    pub id: [u8; 3],
    /// Conventional density must agree; maximum is16MiB for24-bit commands.
    pub capacity: u32,
    /// Logical unit1 or256; repeated partial-page programming must be qualified.
    pub write_size: u32,
    /// Complete SR1 protection predicate mask, excluding WIP/WEL.
    pub protection_mask: u8,
    /// Required masked SR1 value; other status/lock/pin state is caller attested.
    pub protection_value: u8,
    /// Nonzero status-poll spacing.
    pub poll_interval_ms: u64,
    /// Qualified page-program maximum from physical command issue.
    pub program_max_ms: u64,
    /// Qualified single-sector erase maximum from physical command issue.
    pub erase_max_ms: u64,
    /// Bounded recovery/normal Sync settlement window.
    pub sync_max_ms: u64,
    /// Inclusive whole operation bound including scheduling, SPI and readback.
    /// Time is checked before and after transactions; completion exactly at it is valid.
    pub operation_max_ms: u64,
}
/// Structurally validated configuration; deliberately not named hardware-qualified.
#[derive(Clone, Copy, Debug)]
pub struct Profile(ProfileConfig);
impl Profile {
    /// Validate arithmetic/masks/geometry only; never infer qualification.
    /// # Errors
    /// Returns Profile for unsupported or incoherent fields.
    pub fn validate(config: ProfileConfig) -> Result<Self, Fault> {
        if matches!(config.id[0], 0 | 255)
            || matches!(config.id[1], 0 | 255)
            || !(16..=24).contains(&config.id[2])
            || config.capacity
                != 1_u32
                    .checked_shl(u32::from(config.id[2]))
                    .ok_or(Fault::Profile)?
            || !matches!(config.write_size, 1 | 256)
            || config.protection_mask & 3 != 0
            || config.protection_value & !config.protection_mask != 0
            || [
                config.poll_interval_ms,
                config.program_max_ms,
                config.erase_max_ms,
                config.sync_max_ms,
                config.operation_max_ms,
            ]
            .contains(&0)
            || config.operation_max_ms < config.program_max_ms.max(config.erase_max_ms)
            || config.poll_interval_ms
                > config
                    .program_max_ms
                    .min(config.erase_max_ms)
                    .min(config.sync_max_ms)
        {
            return Err(Fault::Profile);
        }
        Ok(Self(config))
    }
}
/// Declared exclusive array partition; all medium offsets are relative to it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Partition {
    /// Physical24-bit sector-aligned start.
    pub start: u32,
    /// Nonzero sector-aligned capacity.
    pub length: u32,
}
/// Explicit trusted caller evidence references, including model-only references.
///
/// Nonempty text is not independently verified evidence. By passing this value,
/// integration attests exact part, complete protection/pin state, maxima/transfer
/// bounds, exclusivity, repeated page programming and durable byte-fault model.
/// Parts needing unimplemented SR2/3/lock checks must not be attested. The device
/// must already be awake and be owned solely by this chip for its entire lifetime.
#[derive(Clone, Copy, Debug)]
pub struct Attestation {
    /// Exact part/geometry and awake-state qualification evidence.
    pub part: &'static str,
    /// Complete protection/lock/pin policy evidence beyond the SR1 predicate.
    pub protection: &'static str,
    /// Actual operation, transfer and foreground scheduling bounds.
    pub timing: &'static str,
    /// Provisioned partition and exclusive chip/array owner evidence.
    pub ownership: &'static str,
    /// Ordered durability, partial programming and atomic-byte fault assumptions.
    pub durability: &'static str,
}
/// Opaque checked epoch for one externally coordinated recovery job.
/// Tokens are local to one chip instance; never transfer them between chips.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct RecoveryToken(u64);
/// Last array/arming obligation, retained after errors and cleared only by Sync.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PendingEffect {
    /// Partition-relative potentially affected start.
    pub offset: u32,
    /// Maximum potentially affected length.
    pub length: u32,
    /// An array command transaction was attempted, including partial/error paths.
    pub command_may_have_reached_chip: bool,
    /// WREN may have reached the chip and settled WEL has not yet been observed.
    pub write_enable_may_be_set: bool,
}
/// Boot-owned single-chip owner; use its asset method instead of a second reader.
pub struct NorChip<S: SpiDevice, C: Monotonic> {
    inner: RefCell<Inner<S, C>>,
}
struct Inner<S, C> {
    device: S,
    clock: C,
    profile: Profile,
    partition: Partition,
    claimed: bool,
    gate: Gate,
    epoch: u64,
    job: Option<Job>,
    terminal: Option<Result<Completion, Error>>,
    last_now: Option<u64>,
    last_fault: Option<Fault>,
    effect: Option<PendingEffect>,
    program: [u8; 256],
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Gate {
    NeedsRecovery,
    Drain(u64),
    Scan(u64),
    Open,
}
#[derive(Clone, Copy, Debug)]
enum Kind {
    Read { offset: u32, length: usize },
    Program { offset: u32, length: usize },
    Erase { offset: u32 },
    Sync,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Status,
    OldRead,
    Wren,
    Wel,
    Issue,
    Wait,
    Verify,
    Read,
    SyncStatus,
    Identity,
}
#[derive(Clone, Copy, Debug)]
struct Job {
    kind: Kind,
    phase: Phase,
    deadline: u64,
    busy_deadline: Option<u64>,
    next_poll: u64,
    cursor: u32,
}
#[derive(Clone, Copy, Debug)]
struct Limits {
    operation: u64,
    issued: Option<u64>,
}
impl Job {
    fn limits(&self) -> Limits {
        Limits {
            operation: self.deadline,
            issued: self.busy_deadline,
        }
    }
}
impl<S: SpiDevice, C: Monotonic> fmt::Debug for NorChip<S, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NorChip").finish_non_exhaustive()
    }
}
impl<S: SpiDevice, C: Monotonic> NorChip<S, C> {
    /// Own an awake device under the explicit trusted attestation boundary.
    /// Does no SPI I/O; public access stays shut until settled bank/owner recovery.
    /// # Errors
    /// Rejects invalid partition or missing evidence references.
    pub fn new(
        device: S,
        clock: C,
        profile: Profile,
        partition: Partition,
        attestation: Attestation,
    ) -> Result<Self, Fault> {
        if [
            attestation.part,
            attestation.protection,
            attestation.timing,
            attestation.ownership,
            attestation.durability,
        ]
        .iter()
        .any(|s| s.trim().is_empty())
        {
            return Err(Fault::Attestation);
        }
        Self::construct(device, clock, profile, partition)
    }
    /// Construct an explicitly unqualified offline-candidate policy.
    /// This grants no board qualification or evidence attestation. The caller
    /// supplies every deadline/protection expectation; no policy is inferred.
    /// Access remains closed until actual Sync checks JEDEC, settled WIP/WEL
    /// and SR1, followed by recovery of every owning format. No unprotect,
    /// wake, formatting or migration command is issued.
    /// # Errors
    /// Rejects incoherent caller policy or out-of-range partition without I/O.
    #[cfg(feature = "experimental-candidate")]
    pub fn new_experimental(
        device: S,
        clock: C,
        policy: ProfileConfig,
        partition: Partition,
    ) -> Result<Self, Fault> {
        Self::construct(device, clock, Profile::validate(policy)?, partition)
    }
    fn construct(
        device: S,
        clock: C,
        profile: Profile,
        partition: Partition,
    ) -> Result<Self, Fault> {
        if partition.length == 0
            || !partition.start.is_multiple_of(4096)
            || !partition.length.is_multiple_of(4096)
            || partition
                .start
                .checked_add(partition.length)
                .is_none_or(|end| end > profile.0.capacity)
        {
            return Err(Fault::Partition);
        }
        Ok(Self {
            inner: RefCell::new(Inner {
                device,
                clock,
                profile,
                partition,
                claimed: false,
                gate: Gate::NeedsRecovery,
                epoch: 0,
                job: None,
                terminal: None,
                last_now: None,
                last_fault: None,
                effect: None,
                program: [0; 256],
            }),
        })
    }
    /// Issue the sole non-clone storage handle; dropping it never releases its claim.
    /// # Errors
    /// Returns Busy/Claimed for conflicting ownership.
    pub fn claim_writer(&self) -> Result<WritableNor<'_, S, C>, Fault> {
        let mut state = self.inner.try_borrow_mut().map_err(|_| Fault::Busy)?;
        if state.claimed {
            return Err(Fault::Claimed);
        }
        state.claimed = true;
        Ok(WritableNor {
            chip: self,
            geometry: Geometry {
                capacity: state.partition.length,
                erase_size: 4096,
                write_size: state.profile.0.write_size,
            },
        })
    }
    /// Close assets and start a checked recovery epoch before engine recovery polls.
    /// # Errors
    /// Rejects active medium operations, counter exhaustion and conflicting borrow.
    pub fn begin_recovery(&self) -> Result<RecoveryToken, Fault> {
        let mut state = self.inner.try_borrow_mut().map_err(|_| Fault::Busy)?;
        if state.job.is_some() {
            return Err(Fault::Busy);
        }
        let epoch = state.epoch.checked_add(1).ok_or(Fault::RecoveryOrder)?;
        state.epoch = epoch;
        state.gate = Gate::Drain(epoch);
        Ok(RecoveryToken(epoch))
    }
    /// Trusted integration attestation that both banks AND owner recovered successfully.
    ///
    /// Caller must first observe engine/owner recovery success; Sync alone is not
    /// sufficient. Hardware settlement, exact epoch and no active command are checked
    /// here. This method does not itself validate banks or produce a durable receipt.
    /// # Errors
    /// Rejects stale epochs, incomplete Sync, active operations or retained uncertainty.
    pub fn attest_recovery_complete(&self, token: RecoveryToken) -> Result<(), Fault> {
        let mut state = self.inner.try_borrow_mut().map_err(|_| Fault::Busy)?;
        if state.gate != Gate::Scan(token.0) || state.job.is_some() || state.effect.is_some() {
            return Err(Fault::RecoveryOrder);
        }
        if let Err(fault) = state.sample_time() {
            state.fail(fault);
            return Err(fault);
        }
        state.gate = Gate::Open;
        Ok(())
    }
    /// Read <=256 bytes wholly outside the storage partition, only while public idle.
    /// Output remains untouched on error. No alternative same-die reader may bypass it.
    /// # Errors
    /// Rejects active/recovering/faulted chip, bounds, clock or transport failure.
    pub fn read_asset(&self, offset: u32, out: &mut [u8]) -> Result<(), Fault> {
        let mut state = self.inner.try_borrow_mut().map_err(|_| Fault::Busy)?;
        if state.gate != Gate::Open {
            return Err(Fault::NeedsRecovery);
        }
        if state.job.is_some() {
            return Err(Fault::Busy);
        }
        let length = u32::try_from(out.len()).map_err(|_| Fault::Bounds)?;
        let end = offset.checked_add(length).ok_or(Fault::Bounds)?;
        let partition_end = state.partition.start + state.partition.length;
        if length == 0
            || length > 256
            || end > state.profile.0.capacity
            || (offset < partition_end && end > state.partition.start)
        {
            return Err(Fault::Bounds);
        }
        let mut scratch = [0; 256];
        let result = (|| {
            let now = state.sample_time()?;
            let operation = now
                .checked_add(state.profile.0.operation_max_ms)
                .ok_or(Fault::DeadlineOverflow)?;
            state.read_into(
                offset,
                &mut scratch[..out.len()],
                Limits {
                    operation,
                    issued: None,
                },
            )
        })();
        if let Err(fault) = result {
            state.fail(fault);
            return Err(fault);
        }
        out.copy_from_slice(&scratch[..out.len()]);
        Ok(())
    }
    /// Most recent diagnostic, retained across settlement for auditing.
    /// # Errors
    /// Returns Busy if another foreground call owns the checked borrow.
    pub fn last_fault(&self) -> Result<Option<Fault>, Fault> {
        Ok(self.inner.try_borrow().map_err(|_| Fault::Busy)?.last_fault)
    }
    /// Inspect retained physical responsibility without clearing it.
    /// # Errors
    /// Returns Busy on a conflicting checked borrow.
    pub fn outstanding_effect(&self) -> Result<Option<PendingEffect>, Fault> {
        Ok(self.inner.try_borrow().map_err(|_| Fault::Busy)?.effect)
    }
}
/// Exclusive partition capability consumed by `IncrementalSnapshot`; never clone it.
pub struct WritableNor<'a, S: SpiDevice, C: Monotonic> {
    chip: &'a NorChip<S, C>,
    geometry: Geometry,
}
impl<S: SpiDevice, C: Monotonic> fmt::Debug for WritableNor<'_, S, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WritableNor")
            .field("geometry", &self.geometry)
            .finish_non_exhaustive()
    }
}
impl<S: SpiDevice, C: Monotonic> AsyncMedium for WritableNor<'_, S, C> {
    fn geometry(&self) -> Geometry {
        self.geometry
    }
    fn begin(&mut self, operation: MediumOp<'_>) -> Result<(), StartError> {
        let mut state = self
            .chip
            .inner
            .try_borrow_mut()
            .map_err(|_| StartError::Busy)?;
        if state.job.is_some() {
            return Err(StartError::Busy);
        }
        if let Err(fault) = state.admit(&operation) {
            state.fail(fault);
            return Err(StartError::Io);
        }
        Ok(())
    }
    fn poll(&mut self, out: &mut [u8]) -> Poll<Result<Completion, Error>> {
        self.poll_checked(out, None)
    }
    fn poll_expected(
        &mut self,
        expected: Completion,
        out: &mut [u8],
    ) -> Poll<Result<Completion, Error>> {
        if matches!(expected, Completion::Read(n) if n > 256 || n > out.len()) {
            return Poll::Ready(Err(Error::Capacity));
        }
        self.poll_checked(out, Some(expected))
    }
}
impl<S: SpiDevice, C: Monotonic> WritableNor<'_, S, C> {
    #[inline(never)]
    fn poll_checked(
        &mut self,
        out: &mut [u8],
        expected: Option<Completion>,
    ) -> Poll<Result<Completion, Error>> {
        let Ok(mut state) = self.chip.inner.try_borrow_mut() else {
            return Poll::Pending;
        };
        let Some(mut job) = state.job.take() else {
            let result = state.terminal.unwrap_or(Err(Error::Io));
            if result.is_ok_and(|done| expected.is_some_and(|e| e != done)) {
                state.fail(Fault::Completion);
                return Poll::Ready(Err(Error::Corrupt));
            }
            if let Ok(Completion::Read(length)) = result {
                if out.len() < length {
                    state.fail(Fault::Completion);
                    return Poll::Ready(Err(Error::Io));
                }
                out[..length].copy_from_slice(&state.program[..length]);
            }
            return Poll::Ready(result);
        };
        let result = state.advance(&mut job, out.len());
        match result {
            Err(fault) => {
                state.fail(fault);
                Poll::Ready(Err(Error::Io))
            }
            Ok(None) => {
                state.job = Some(job);
                Poll::Pending
            }
            Ok(Some(completion)) => {
                if expected.is_some_and(|e| e != completion) {
                    state.fail(Fault::Completion);
                    return Poll::Ready(Err(Error::Corrupt));
                }
                if let Completion::Read(length) = completion {
                    out[..length].copy_from_slice(&state.program[..length]);
                }
                state.terminal = Some(Ok(completion));
                Poll::Ready(Ok(completion))
            }
        }
    }
}

impl<S: SpiDevice, C: Monotonic> Inner<S, C> {
    fn sample_time(&mut self) -> Result<u64, Fault> {
        let now = self.clock.now_ms();
        if self.last_now.is_some_and(|old| now < old) {
            return Err(Fault::ClockRegression);
        }
        self.last_now = Some(now);
        Ok(now)
    }
    fn check_time(&mut self, limits: Limits) -> Result<u64, Fault> {
        let now = self.sample_time()?;
        if now > limits.operation || limits.issued.is_some_and(|deadline| now > deadline) {
            return Err(Fault::Deadline);
        }
        Ok(now)
    }
    fn fail(&mut self, fault: Fault) {
        self.last_fault = Some(fault);
        self.gate = Gate::NeedsRecovery;
        self.job = None;
        self.terminal = Some(Err(Error::Io));
    }
    fn physical(&self, offset: u32, length: usize) -> Result<u32, Fault> {
        let length = u32::try_from(length).map_err(|_| Fault::Bounds)?;
        if length == 0
            || offset
                .checked_add(length)
                .is_none_or(|end| end > self.partition.length)
        {
            return Err(Fault::Bounds);
        }
        self.partition
            .start
            .checked_add(offset)
            .ok_or(Fault::Bounds)
    }
    fn admit(&mut self, operation: &MediumOp<'_>) -> Result<(), Fault> {
        let config = self.profile.0;
        let kind = match *operation {
            MediumOp::Read { offset, length } => {
                if !matches!(self.gate, Gate::Open | Gate::Scan(_)) {
                    return Err(Fault::NeedsRecovery);
                }
                self.physical(offset, length)?;
                if length > 256 {
                    return Err(Fault::Bounds);
                }
                Kind::Read { offset, length }
            }
            MediumOp::Program { offset, bytes } => {
                if self.gate != Gate::Open {
                    return Err(Fault::NeedsRecovery);
                }
                let physical = self.physical(offset, bytes.len())?;
                if bytes.len() != config.write_size as usize
                    || !offset.is_multiple_of(config.write_size)
                    || physical % 256 + config.write_size > 256
                {
                    return Err(Fault::Alignment);
                }
                self.program[..bytes.len()].copy_from_slice(bytes);
                Kind::Program {
                    offset,
                    length: bytes.len(),
                }
            }
            MediumOp::Erase { offset } => {
                if self.gate != Gate::Open {
                    return Err(Fault::NeedsRecovery);
                }
                self.physical(offset, 4096)?;
                if !offset.is_multiple_of(4096) {
                    return Err(Fault::Alignment);
                }
                Kind::Erase { offset }
            }
            MediumOp::Sync => Kind::Sync,
        };
        let now = self.sample_time()?;
        let bound = if matches!(kind, Kind::Sync) {
            config.sync_max_ms
        } else {
            config.operation_max_ms
        };
        let deadline = now.checked_add(bound).ok_or(Fault::DeadlineOverflow)?;
        let phase = if matches!(kind, Kind::Sync) {
            Phase::SyncStatus
        } else {
            Phase::Status
        };
        if matches!(kind, Kind::Sync) && self.gate == Gate::NeedsRecovery {
            self.gate = Gate::Drain(self.epoch);
        }
        self.job = Some(Job {
            kind,
            phase,
            deadline,
            busy_deadline: None,
            next_poll: now,
            cursor: 0,
        });
        self.terminal = None;
        Ok(())
    }
    fn status(&mut self, limits: Limits) -> Result<u8, Fault> {
        let mut status = [0];
        self.device
            .transaction(&mut [Operation::Write(&[0x05]), Operation::Read(&mut status)])
            .map_err(|_| Fault::Transport)?;
        self.check_time(limits)?;
        if status[0] & self.profile.0.protection_mask != self.profile.0.protection_value {
            return Err(Fault::Protection);
        }
        Ok(status[0])
    }
    fn read_into(&mut self, physical: u32, out: &mut [u8], limits: Limits) -> Result<(), Fault> {
        let bytes = physical.to_be_bytes();
        let command = [0x03, bytes[1], bytes[2], bytes[3]];
        self.device
            .transaction(&mut [Operation::Write(&command), Operation::Read(out)])
            .map_err(|_| Fault::Transport)?;
        self.check_time(limits)?;
        Ok(())
    }
    fn read_program(&mut self, physical: u32, length: usize, limits: Limits) -> Result<(), Fault> {
        let bytes = physical.to_be_bytes();
        let command = [0x03, bytes[1], bytes[2], bytes[3]];
        self.device
            .transaction(&mut [
                Operation::Write(&command),
                Operation::Read(&mut self.program[..length]),
            ])
            .map_err(|_| Fault::Transport)?;
        self.check_time(limits)?;
        Ok(())
    }
    #[inline(never)]
    fn check_old_program(
        &mut self,
        physical: u32,
        length: usize,
        limits: Limits,
    ) -> Result<(), Fault> {
        let mut scratch = [0; 256];
        self.read_into(physical, &mut scratch[..length], limits)?;
        if scratch[..length]
            .iter()
            .zip(&self.program[..length])
            .any(|(&old, &new)| old & new != new)
        {
            return Err(Fault::Readback);
        }
        Ok(())
    }
    fn settled(status: u8) -> Result<(), Fault> {
        if status & 1 != 0 {
            return Err(Fault::NeedsRecovery);
        }
        if status & 2 != 0 {
            return Err(Fault::WriteEnable);
        }
        Ok(())
    }
    fn advance(&mut self, job: &mut Job, output: usize) -> Result<Option<Completion>, Fault> {
        if let Kind::Read { length, .. } = job.kind
            && output < length
        {
            return Err(Fault::Completion);
        }
        let now = self.check_time(job.limits())?;
        if matches!(job.phase, Phase::Wait | Phase::SyncStatus) && now < job.next_poll {
            return Ok(None);
        }
        match job.phase {
            Phase::Status => {
                Self::settled(self.status(job.limits())?)?;
                job.phase = match job.kind {
                    Kind::Read { .. } => Phase::Read,
                    Kind::Program { .. } => Phase::OldRead,
                    Kind::Erase { .. } => Phase::Wren,
                    Kind::Sync => return Err(Fault::Completion),
                };
            }
            Phase::OldRead => {
                let Kind::Program { offset, length } = job.kind else {
                    return Err(Fault::Completion);
                };
                self.check_old_program(self.physical(offset, length)?, length, job.limits())?;
                job.phase = Phase::Wren;
            }
            Phase::Wren => {
                let (offset, length) = match job.kind {
                    Kind::Program { offset, length } => {
                        (offset, u32::try_from(length).map_err(|_| Fault::Bounds)?)
                    }
                    Kind::Erase { offset } => (offset, 4096),
                    _ => return Err(Fault::Completion),
                };
                self.effect = Some(PendingEffect {
                    offset,
                    length,
                    command_may_have_reached_chip: false,
                    write_enable_may_be_set: true,
                });
                self.device.write(&[0x06]).map_err(|_| Fault::Transport)?;
                self.check_time(job.limits())?;
                job.phase = Phase::Wel;
            }
            Phase::Wel => {
                let status = self.status(job.limits())?;
                if status & 3 != 2 {
                    return Err(Fault::WriteEnable);
                }
                job.phase = Phase::Issue;
            }
            Phase::Issue => self.issue(job, now)?,
            Phase::Wait | Phase::SyncStatus => self.wait(job, now)?,
            Phase::Verify => return self.verify(job),
            Phase::Read => {
                let Kind::Read { offset, length } = job.kind else {
                    return Err(Fault::Completion);
                };
                self.read_program(self.physical(offset, length)?, length, job.limits())?;
                return Ok(Some(Completion::Read(length)));
            }
            Phase::Identity => {
                let mut id = [0; 3];
                self.device
                    .transaction(&mut [Operation::Write(&[0x9f]), Operation::Read(&mut id)])
                    .map_err(|_| Fault::Transport)?;
                self.check_time(job.limits())?;
                if id != self.profile.0.id {
                    return Err(Fault::Identity);
                }
                self.effect = None;
                if let Gate::Drain(epoch) = self.gate {
                    self.gate = Gate::Scan(epoch);
                }
                return Ok(Some(Completion::Synced));
            }
        }
        Ok(None)
    }
    fn wait(&mut self, job: &mut Job, now: u64) -> Result<(), Fault> {
        let status = self.status(job.limits())?;
        if status & 1 != 0 {
            job.next_poll = now
                .checked_add(self.profile.0.poll_interval_ms)
                .ok_or(Fault::DeadlineOverflow)?;
            return Ok(());
        }
        Self::settled(status)?;
        if let Some(effect) = &mut self.effect {
            effect.write_enable_may_be_set = false;
        }
        job.busy_deadline = None;
        job.phase = if job.phase == Phase::SyncStatus {
            Phase::Identity
        } else {
            Phase::Verify
        };
        Ok(())
    }
    fn issue(&mut self, job: &mut Job, now: u64) -> Result<(), Fault> {
        let (offset, length, opcode, maximum) = match job.kind {
            Kind::Program { offset, length } => {
                (offset, length, 0x02, self.profile.0.program_max_ms)
            }
            Kind::Erase { offset } => (offset, 4096, 0x20, self.profile.0.erase_max_ms),
            _ => return Err(Fault::Completion),
        };
        let deadline = now.checked_add(maximum).ok_or(Fault::DeadlineOverflow)?;
        let next = now
            .checked_add(self.profile.0.poll_interval_ms)
            .ok_or(Fault::DeadlineOverflow)?;
        let bytes = self.physical(offset, length)?.to_be_bytes();
        let command = [opcode, bytes[1], bytes[2], bytes[3]];
        if let Some(effect) = &mut self.effect {
            effect.command_may_have_reached_chip = true;
        }
        job.busy_deadline = Some(deadline);
        if opcode == 0x02 {
            self.device
                .transaction(&mut [
                    Operation::Write(&command),
                    Operation::Write(&self.program[..length]),
                ])
                .map_err(|_| Fault::Transport)?;
        } else {
            self.device.write(&command).map_err(|_| Fault::Transport)?;
        }
        self.check_time(job.limits())?;
        job.next_poll = next;
        job.phase = Phase::Wait;
        Ok(())
    }
    // Pre-program validation and post-program verification never overlap.
    // Release this readback page between phases instead of keeping two pages
    // in the polling caller's frame.
    #[inline(never)]
    fn verify(&mut self, job: &mut Job) -> Result<Option<Completion>, Fault> {
        match job.kind {
            Kind::Program { offset, length } => {
                let mut scratch = [0; 256];
                self.read_into(
                    self.physical(offset, length)?,
                    &mut scratch[..length],
                    job.limits(),
                )?;
                if scratch[..length] != self.program[..length] {
                    return Err(Fault::Readback);
                }
                Ok(Some(Completion::Programmed))
            }
            Kind::Erase { offset } => {
                self.read_program(self.physical(offset + job.cursor, 256)?, 256, job.limits())?;
                if self.program.iter().any(|&byte| byte != 255) {
                    return Err(Fault::Readback);
                }
                job.cursor += 256;
                Ok((job.cursor == 4096).then_some(Completion::Erased))
            }
            _ => Err(Fault::Completion),
        }
    }
}
