//! Heap-free, explicit begin/poll DSNAP001 persistence.
//!
//! Each engine poll calls at most one medium `begin` OR `poll`. It never calls
//! the synchronous `Medium` API. See README-incremental.md for ownership and
//! fault assumptions. CRC detects accidental corruption, not malicious changes.
#![forbid(unsafe_code)]
use crate::persistence::{
    Error, Geometry, HEADER_BYTES, Layout, MAX_WRITE, Snapshot, decode_header, encode_header,
    validate,
};
use core::{fmt, task::Poll};
use device_protocol::Crc32c;

/// One bounded operation admitted by an asynchronous medium.
pub enum MediumOp<'a> {
    /// Read between one and 256 bytes, without alignment restrictions.
    Read {
        /// Absolute medium offset.
        offset: u32,
        /// Requested byte count.
        length: usize,
    },
    /// Copy and program one aligned write unit (at most 256 bytes).
    Program {
        /// Absolute aligned medium offset.
        offset: u32,
        /// Input must be consumed/copied before `begin` returns.
        bytes: &'a [u8],
    },
    /// Erase exactly one geometry erase unit.
    Erase {
        /// Absolute erase-unit-aligned medium offset.
        offset: u32,
    },
    /// Settle earlier ordered effects and establish durability.
    Sync,
}
impl fmt::Debug for MediumOp<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { offset, length } => f
                .debug_struct("Read")
                .field("offset", offset)
                .field("length", length)
                .finish(),
            Self::Program { offset, bytes } => f
                .debug_struct("Program")
                .field("offset", offset)
                .field("length", &bytes.len())
                .finish(),
            Self::Erase { offset } => f.debug_struct("Erase").field("offset", offset).finish(),
            Self::Sync => f.write_str("Sync"),
        }
    }
}
/// Exact completion for a previously admitted operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Completion {
    /// Exactly the requested number of bytes was copied into the poll output.
    Read(usize),
    /// The admitted program finished; durability still requires Sync.
    Programmed,
    /// The admitted single erase unit finished.
    Erased,
    /// Earlier ordered effects are durable and settled.
    Synced,
}
/// Admission failure; Busy guarantees no admission or effects.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StartError {
    /// No effects occurred; retrying in a later slice is safe.
    Busy,
    /// Admission may have had partial effects; recovery is required.
    Io,
}
/// Bounded exclusive asynchronous medium capability.
///
/// Both methods must return promptly without spinning or blocking for erase.
/// `begin` copies program input before return. Pending owns the medium until a
/// terminal poll. Each slice releases physical CS/bus ownership. Reads copy
/// only on successful completion, never on Pending/error, and must respect the
/// admitted range. Geometry remains fixed. Reads remain stable under exclusive
/// ownership. A terminal failure must permit a later recovery Sync to settle
/// any outstanding ordered effects. The synchronous persistence byte-fault
/// model also applies: arbitrary bytes may be applied, each byte atomically.
pub trait AsyncMedium {
    /// Return fixed geometry without I/O.
    fn geometry(&self) -> Geometry;
    /// Admit at most one bounded operation, taking ownership of program bytes.
    ///
    /// # Errors
    /// Busy means no effects; Io means an uncertain outcome.
    fn begin(&mut self, operation: MediumOp<'_>) -> Result<(), StartError>;
    /// Perform one bounded completion/status slice, with 256-byte output space.
    ///
    /// # Errors
    /// A terminal error is uncertain and requires a subsequent recovery Sync.
    fn poll(&mut self, read_out: &mut [u8]) -> Poll<Result<Completion, Error>>;
    /// Poll and validate the exact admitted completion before publishing bytes.
    /// A layering adapter can avoid an extra transfer page when its underlying
    /// medium implements this guard at its existing read-publication boundary.
    /// The default stages output and never exposes Pending/error/mismatch bytes.
    /// # Errors
    /// Same uncertainty as `poll`; mismatched completion is Corrupt. Short read
    /// output is Capacity without polling the accepted operation.
    fn poll_expected(
        &mut self,
        expected: Completion,
        out: &mut [u8],
    ) -> Poll<Result<Completion, Error>> {
        if matches!(expected, Completion::Read(n) if n > 256 || n > out.len()) {
            return Poll::Ready(Err(Error::Capacity));
        }
        let mut scratch = [0; 256];
        match self.poll(&mut scratch) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(done)) if done == expected => {
                if let Completion::Read(n) = done {
                    out[..n].copy_from_slice(&scratch[..n]);
                }
                Poll::Ready(Ok(done))
            }
            Poll::Ready(Ok(_)) => Poll::Ready(Err(Error::Corrupt)),
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
        }
    }
}
/// Checked monotonically increasing token, unique within one engine lifetime.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CommitId(u64);
/// Final sync and exact readback have confirmed this snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct DurableReceipt {
    /// Token identifying the accepted commit.
    pub id: CommitId,
    /// Confirmed committed metadata.
    pub snapshot: Snapshot,
}
/// Incremental admission/progress failure.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AsyncError {
    /// Another commit or recovery is active.
    Busy,
    /// Token or poll kind does not match the current retained operation.
    InvalidOperation,
    /// Commit tokens cannot increment without wrapping.
    TokenExhausted,
    /// Medium returned an incorrect kind or read length; recovery is required.
    InvalidCompletion,
    /// Storage preflight, I/O, corruption or recovery failure.
    Storage(Error),
}
/// Rejection of an in-place snapshot encoding, before any medium I/O.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum CommitEncodeError<E> {
    /// Snapshot admission or encoded length failed; the encoder may not run.
    Storage(AsyncError),
    /// Encoder failed; committed bytes, token and terminal result remain intact.
    Encoding(E),
}
impl<E> fmt::Debug for CommitEncodeError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Storage(error) => f.debug_tuple("Storage").field(error).finish(),
            Self::Encoding(_) => f.write_str("Encoding(..)"),
        }
    }
}
impl From<Error> for AsyncError {
    fn from(error: Error) -> Self {
        Self::Storage(error)
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Phase {
    Idle,
    Crc,
    Retire,
    RetireSync,
    Erase,
    Header,
    Payload,
    PrepareSync,
    Marker,
    FinalSync,
    VerifyMarker,
    VerifyHeader,
    VerifyPayload,
    RecoverSync,
    ScanMarker,
    ScanHeader,
    ScanPayload,
    Reload,
}
#[derive(Clone, Copy, Debug)]
enum Job {
    None,
    Commit { id: CommitId, snapshot: Snapshot },
    Recover,
}
#[derive(Clone, Copy, Debug)]
struct Bank {
    snapshot: Snapshot,
    crc: u32,
}
#[derive(Clone, Copy, Debug)]
enum Command {
    Read { offset: u32, length: usize },
    Program { offset: u32, length: usize },
    Erase(u32),
    Sync,
}
impl Command {
    fn completion(self) -> Completion {
        match self {
            Self::Read { length, .. } => Completion::Read(length),
            Self::Program { .. } => Completion::Programmed,
            Self::Erase(_) => Completion::Erased,
            Self::Sync => Completion::Synced,
        }
    }
}
/// Incremental two-bank store with a borrowed transfer page.
/// The default S=2 preserves old committed reads throughout healthy commits.
/// S=1 is an opt-in exclusive metadata writer: accepted commits block image reads.
///
/// The old committed slot remains readable throughout a healthy pending commit.
/// Every uncertainty poisons reads and further commits until recovery succeeds.
/// Terminal results replay until a *successful* next commit/recovery admission;
/// failed preflight and stale tokens never consume or replace them. Recovery
/// results use the same retention rule. Tokens are engine-local, not persistent.
/// Debug omits medium, scratch, headers and payloads.
pub struct IncrementalSnapshot<M, const N: usize, const S: usize = 2> {
    medium: M,
    layout: Layout,
    geometry: Geometry,
    payload_offset: u32,
    slots: [[u8; N]; S],
    committed: usize,
    current: Option<Snapshot>,
    current_bank: u32,
    poisoned: bool,
    counter: u64,
    job: Job,
    phase: Phase,
    command: Option<Command>,
    terminal: Option<Result<(), AsyncError>>,
    target: u32,
    cursor: u32,
    crc: Crc32c,
    banks: [Option<Bank>; 2],
    header: [u8; HEADER_BYTES],
}
impl<M, const N: usize, const S: usize> fmt::Debug for IncrementalSnapshot<M, N, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IncrementalSnapshot")
            .field("layout", &self.layout)
            .field("current", &self.current)
            .field("poisoned", &self.poisoned)
            .field("phase", &self.phase)
            .finish_non_exhaustive()
    }
}
impl<M: AsyncMedium, const N: usize> IncrementalSnapshot<M, N> {
    /// Transfer an idle metadata engine into a single-image exclusive writer.
    /// Use only when the enclosing owner keeps its own confirmed decoded state
    /// and never reads this engine during commits. Physical two-bank atomicity
    /// and readback are unchanged. Encoding rejection preserves confirmed bytes;
    /// an accepted commit makes image reads unavailable until verified completion.
    /// # Errors
    /// An active operation returns the original engine alongside Busy, preserving
    /// ownership and progress instead of dropping an admitted medium operation.
    #[allow(clippy::result_large_err)] // Returning the owned in-flight capability is essential in this heap-free API.
    pub fn into_exclusive(self) -> Result<IncrementalSnapshot<M, N, 1>, (Self, AsyncError)> {
        if self.active() {
            return Err((self, AsyncError::Busy));
        }
        Ok(IncrementalSnapshot {
            medium: self.medium,
            layout: self.layout,
            geometry: self.geometry,
            payload_offset: self.payload_offset,
            slots: [self.slots[self.committed]],
            committed: 0,
            current: self.current,
            current_bank: self.current_bank,
            poisoned: self.poisoned,
            counter: self.counter,
            job: self.job,
            phase: self.phase,
            command: self.command,
            terminal: self.terminal,
            target: self.target,
            cursor: self.cursor,
            crc: self.crc,
            banks: self.banks,
            header: self.header,
        })
    }
}
impl<M: AsyncMedium, const N: usize, const S: usize> IncrementalSnapshot<M, N, S> {
    /// Validate layout without I/O; reads/commits require explicit recovery.
    ///
    /// # Errors
    /// Returns `InvalidLayout` for unsupported geometry, ranges or capacity.
    pub fn new_unrecovered(medium: M, layout: Layout) -> Result<Self, Error> {
        if S != 1 && S != 2 {
            return Err(Error::InvalidLayout);
        }
        let geometry = medium.geometry();
        let payload_offset = validate::<N>(geometry, layout)?;
        Ok(Self {
            medium,
            layout,
            geometry,
            payload_offset,
            slots: [[0; N]; S],
            committed: 0,
            current: None,
            current_bank: 0,
            poisoned: true,
            counter: 0,
            job: Job::None,
            phase: Phase::Idle,
            command: None,
            terminal: None,
            target: 0,
            cursor: 0,
            crc: Crc32c::new(),
            banks: [None; 2],
            header: [0; HEADER_BYTES],
        })
    }
    /// Return whether a commit or recovery is still pending, without I/O.
    ///
    /// True covers the entire accepted operation, including CPU preparation and
    /// gaps between medium commands. Retained terminal results are not busy.
    /// False does not establish health: idle uncertainty still makes
    /// `load_committed` return `NeedsRecovery`.
    #[must_use]
    pub fn is_busy(&self) -> bool {
        self.active()
    }
    /// Read the old confirmed image, including healthy pending commits for S=2.
    /// The exclusive S=1 mode blocks image reads during accepted commits.
    ///
    /// # Errors
    /// Returns `NeedsRecovery` after uncertainty/during recovery, or Capacity for
    /// short output. A confirmed blank medium returns None without formatting.
    pub fn load_committed(&self, out: &mut [u8]) -> Result<Option<Snapshot>, Error> {
        if self.poisoned {
            return Err(Error::NeedsRecovery);
        }
        if let Some(snapshot) = self.current {
            if out.len() < snapshot.length {
                return Err(Error::Capacity);
            }
            out[..snapshot.length].copy_from_slice(&self.slots[self.committed][..snapshot.length]);
        }
        Ok(self.current)
    }
    /// Inspect the exact confirmed image without copying it or performing I/O.
    ///
    /// A confirmed blank medium supplies `None` and an empty slice. A healthy
    /// pending commit in S=2 still supplies the old confirmed image, never its
    /// candidate. The exclusive S=1 mode blocks inspection during commits.
    /// The scoped slice is immutable and cannot escape the callback's result.
    ///
    /// ```compile_fail
    /// use device_storage::incremental::{AsyncMedium, IncrementalSnapshot};
    /// fn escape<M: AsyncMedium, const N: usize>(engine: &IncrementalSnapshot<M, N>) -> &[u8] {
    ///     engine.inspect_committed(|_, bytes| bytes).unwrap()
    /// }
    /// ```
    ///
    /// # Errors
    /// Returns `NeedsRecovery` without invoking the callback after uncertainty,
    /// before the first recovery, and throughout an active recovery.
    pub fn inspect_committed<R>(
        &self,
        inspect: impl for<'image> FnOnce(Option<Snapshot>, &'image [u8]) -> R,
    ) -> Result<R, Error> {
        if self.poisoned {
            return Err(Error::NeedsRecovery);
        }
        let length = self.current.map_or(0, |snapshot| snapshot.length);
        Ok(inspect(self.current, &self.slots[self.committed][..length]))
    }
    /// Accept an owned candidate after checking CAS, bounds and both counters.
    ///
    /// This admission copies at most N bytes; polling CRC is bounded to 256.
    /// It performs no medium I/O. Acceptance does not imply durability.
    ///
    /// # Errors
    /// Busy, `NeedsRecovery`, Conflict, Capacity, `RevisionExhausted` or
    /// `TokenExhausted` are returned without mutation or loss of retained results.
    pub fn begin_commit(&mut self, expected: u64, bytes: &[u8]) -> Result<CommitId, AsyncError> {
        let (revision, id) = self.commit_preflight(expected, Some(bytes.len()))?;
        let spare = self.spare();
        self.slots[spare][..bytes.len()].copy_from_slice(bytes);
        Ok(self.accept_commit(revision, id, bytes.len()))
    }
    /// Encode directly into the existing spare slot, without another N-byte image.
    ///
    /// Performs no medium I/O. The encoder runs once only after admission checks;
    /// it must initialize every byte in its returned length. Encoding/length
    /// failure may change the private spare slot but preserves committed reads,
    /// the token counter and the retained terminal result. Tokens publish only
    /// after successful encoding; normal begin/poll durability rules then apply.
    /// # Errors
    /// Returns typed encoding failure or the same admission errors as
    /// `begin_commit`; a returned length above N is a capacity error.
    pub fn begin_commit_encoded<E>(
        &mut self,
        expected: u64,
        encode: impl FnOnce(&mut [u8; N]) -> Result<usize, E>,
    ) -> Result<CommitId, CommitEncodeError<E>> {
        let (revision, id) = self
            .commit_preflight(expected, None)
            .map_err(CommitEncodeError::Storage)?;
        let spare = self.spare();
        let length = if S == 1 {
            // A rejected encoder must preserve the confirmed image. The temporary
            // exists only during admission, never in the persistent owner.
            let mut candidate = [0; N];
            let length = encode(&mut candidate).map_err(CommitEncodeError::Encoding)?;
            if length > N {
                return Err(CommitEncodeError::Storage(Error::Capacity.into()));
            }
            self.slots[spare][..length].copy_from_slice(&candidate[..length]);
            length
        } else {
            let length = encode(&mut self.slots[spare]).map_err(CommitEncodeError::Encoding)?;
            if length > N {
                return Err(CommitEncodeError::Storage(Error::Capacity.into()));
            }
            length
        };
        Ok(self.accept_commit(revision, id, length))
    }
    fn commit_preflight(
        &self,
        expected: u64,
        length: Option<usize>,
    ) -> Result<(u64, CommitId), AsyncError> {
        if self.active() {
            return Err(AsyncError::Busy);
        }
        if self.poisoned {
            return Err(Error::NeedsRecovery.into());
        }
        let revision = self.current.map_or(0, |s| s.revision);
        if expected != revision {
            return Err(Error::Conflict.into());
        }
        if length.is_some_and(|length| length > N) {
            return Err(Error::Capacity.into());
        }
        let revision = revision.checked_add(1).ok_or(Error::RevisionExhausted)?;
        let counter = self
            .counter
            .checked_add(1)
            .ok_or(AsyncError::TokenExhausted)?;
        Ok((revision, CommitId(counter)))
    }
    fn accept_commit(&mut self, revision: u64, id: CommitId, length: usize) -> CommitId {
        self.counter = id.0;
        if S == 1 {
            self.poisoned = true;
        }
        self.job = Job::Commit {
            id,
            snapshot: Snapshot { revision, length },
        };
        self.phase = Phase::Crc;
        self.cursor = 0;
        self.crc = Crc32c::new();
        self.terminal = None;
        self.target = if self.current.is_some() {
            1 - self.current_bank
        } else {
            0
        };
        id
    }
    /// Advance exactly one bounded slice for this token, or replay its result.
    ///
    /// # Errors
    /// Stale IDs return `InvalidOperation` without touching the current job.
    /// Failed medium operations/verification poison all reads/new commits.
    pub fn poll_commit(&mut self, id: CommitId) -> Poll<Result<DurableReceipt, AsyncError>> {
        self.poll_commit_with_scratch(id, &mut [0; MAX_WRITE])
    }
    /// Advance one commit slice using a scheduler-owned transfer page.
    /// The page may be overwritten or lent to another owner after every call,
    /// including Pending/Busy. Accepted program bytes belong to the medium.
    /// # Errors
    /// Same token, I/O and verification errors as `poll_commit`.
    pub fn poll_commit_with_scratch(
        &mut self,
        id: CommitId,
        scratch: &mut [u8; MAX_WRITE],
    ) -> Poll<Result<DurableReceipt, AsyncError>> {
        let Job::Commit {
            id: accepted,
            snapshot,
        } = self.job
        else {
            return Poll::Ready(Err(AsyncError::InvalidOperation));
        };
        if accepted != id {
            return Poll::Ready(Err(AsyncError::InvalidOperation));
        }
        self.step(scratch);
        match self.terminal {
            None => Poll::Pending,
            Some(result) => Poll::Ready(result.map(|()| DurableReceipt { id, snapshot })),
        }
    }
    /// Begin recovery with an ordered Sync, invalidating old terminal results.
    ///
    /// # Errors
    /// Returns Busy while any job is pending. This method performs no I/O.
    pub fn begin_recover(&mut self) -> Result<(), AsyncError> {
        if self.active() {
            return Err(AsyncError::Busy);
        }
        self.poisoned = true;
        self.job = Job::Recover;
        self.phase = Phase::RecoverSync;
        self.terminal = None;
        self.banks = [None; 2];
        self.cursor = 0;
        self.target = 0;
        Ok(())
    }
    /// Advance one recovery slice; select only after validating both banks.
    ///
    /// # Errors
    /// Returns `InvalidOperation` for the wrong job; I/O/corruption/invalid
    /// completion leaves `NeedsRecovery`. Terminal results replay until admission.
    pub fn poll_recover(&mut self) -> Poll<Result<(), AsyncError>> {
        self.poll_recover_with_scratch(&mut [0; MAX_WRITE])
    }
    /// Advance recovery using a page lent only for this slice, with no retained bytes.
    /// # Errors
    /// Same job, I/O and verification errors as `poll_recover`.
    pub fn poll_recover_with_scratch(
        &mut self,
        scratch: &mut [u8; MAX_WRITE],
    ) -> Poll<Result<(), AsyncError>> {
        if !matches!(self.job, Job::Recover) {
            return Poll::Ready(Err(AsyncError::InvalidOperation));
        }
        self.step(scratch);
        self.terminal.map_or(Poll::Pending, Poll::Ready)
    }
    /// Transfer exclusive responsibility, including any in-flight operation.
    /// This neither cancels nor settles it, and is never a durable receipt.
    pub fn into_medium(self) -> M {
        self.medium
    }
    fn spare(&self) -> usize {
        if S == 1 { 0 } else { 1 - self.committed }
    }
    fn active(&self) -> bool {
        self.phase != Phase::Idle
    }
    fn bank_start(&self) -> u32 {
        self.layout.start + self.target * self.layout.bank_bytes
    }
    fn marker_offset(&self) -> u32 {
        self.bank_start() + self.layout.bank_bytes - self.geometry.write_size
    }
    fn candidate(&self) -> Snapshot {
        match self.job {
            Job::Commit { snapshot, .. } => snapshot,
            _ => Snapshot {
                revision: 0,
                length: 0,
            },
        }
    }
    fn scan_bank(&self) -> Option<Bank> {
        self.banks[self.target as usize]
    }
    fn finish(&mut self, result: Result<(), AsyncError>) {
        if result.is_err() {
            self.poisoned = true;
        }
        self.phase = Phase::Idle;
        self.command = None;
        self.terminal = Some(result);
    }
    fn advance(&mut self, phase: Phase) {
        self.phase = phase;
        self.cursor = 0;
    }
    fn step(&mut self, scratch: &mut [u8; MAX_WRITE]) {
        if !self.active() {
            return;
        }
        if let Some(command) = self.command {
            match self.medium.poll(scratch) {
                Poll::Pending => {}
                Poll::Ready(Err(_)) => self.finish(Err(Error::Io.into())),
                Poll::Ready(Ok(completion)) => {
                    self.command = None;
                    if completion != command.completion() {
                        self.finish(Err(AsyncError::InvalidCompletion));
                    } else if let Err(error) = self.completed(scratch) {
                        self.finish(Err(error));
                    }
                }
            }
            return;
        }
        if self.phase == Phase::Crc {
            if let Err(error) = self.prepare_crc() {
                self.finish(Err(error));
            }
            return;
        }
        match self.next_command(scratch) {
            Err(error) => self.finish(Err(error)),
            Ok(command) => {
                let operation = match command {
                    Command::Read { offset, length } => MediumOp::Read { offset, length },
                    Command::Program { offset, length } => MediumOp::Program {
                        offset,
                        bytes: &scratch[..length],
                    },
                    Command::Erase(offset) => MediumOp::Erase { offset },
                    Command::Sync => MediumOp::Sync,
                };
                match self.medium.begin(operation) {
                    Ok(()) => self.command = Some(command),
                    Err(StartError::Busy) => {}
                    Err(StartError::Io) => self.finish(Err(Error::Io.into())),
                }
            }
        }
    }
    fn prepare_crc(&mut self) -> Result<(), AsyncError> {
        let snapshot = self.candidate();
        let start = self.cursor as usize;
        let end = start.saturating_add(MAX_WRITE).min(snapshot.length);
        self.crc.update(&self.slots[self.spare()][start..end]);
        self.cursor = u32::try_from(end).map_err(|_| Error::Capacity)?;
        if end == snapshot.length {
            match encode_header(snapshot, self.crc.finish()) {
                Ok(header) => {
                    self.header = header;
                    self.advance(Phase::Retire);
                }
                Err(e) => self.finish(Err(e.into())),
            }
        }
        Ok(())
    }
    #[inline(never)]
    fn next_command(&mut self, scratch: &mut [u8; MAX_WRITE]) -> Result<Command, AsyncError> {
        let start = self.bank_start();
        let unit = self.geometry.write_size as usize;
        let cursor = self.cursor as usize;
        let program = |offset, length| Command::Program { offset, length };
        let read = |offset, length| Command::Read { offset, length };
        Ok(match self.phase {
            Phase::Retire => {
                scratch[..unit].fill(0);
                program(self.marker_offset(), unit)
            }
            Phase::RetireSync | Phase::PrepareSync | Phase::FinalSync | Phase::RecoverSync => {
                Command::Sync
            }
            Phase::Erase => Command::Erase(start + self.cursor),
            Phase::Header => {
                scratch[..unit].fill(255);
                let count = unit.min(HEADER_BYTES - cursor);
                scratch[..count].copy_from_slice(&self.header[cursor..cursor + count]);
                program(start + self.cursor, unit)
            }
            Phase::Payload => {
                scratch[..unit].fill(255);
                let count = unit.min(self.candidate().length - cursor);
                scratch[..count].copy_from_slice(&self.slots[self.spare()][cursor..cursor + count]);
                program(start + self.payload_offset + self.cursor, unit)
            }
            Phase::Marker => {
                scratch[..unit].fill(0xa5);
                program(self.marker_offset(), unit)
            }
            Phase::VerifyMarker | Phase::ScanMarker => read(self.marker_offset(), unit),
            Phase::VerifyHeader | Phase::ScanHeader => read(start, HEADER_BYTES),
            Phase::VerifyPayload => read(
                start + self.payload_offset + self.cursor,
                MAX_WRITE.min(self.candidate().length - cursor),
            ),
            Phase::ScanPayload | Phase::Reload => {
                let bank = self.scan_bank().ok_or(Error::Corrupt)?;
                read(
                    start + self.payload_offset + self.cursor,
                    MAX_WRITE.min(bank.snapshot.length - cursor),
                )
            }
            Phase::Idle | Phase::Crc => return Err(AsyncError::InvalidOperation),
        })
    }
    #[inline(never)]
    fn completed(&mut self, scratch: &mut [u8; MAX_WRITE]) -> Result<(), AsyncError> {
        match self.phase {
            Phase::Retire => self.advance(Phase::RetireSync),
            Phase::RetireSync => self.advance(Phase::Erase),
            Phase::Erase => {
                self.cursor += self.geometry.erase_size;
                if self.cursor == self.layout.bank_bytes {
                    self.advance(Phase::Header);
                }
            }
            Phase::Header => {
                self.cursor += self.geometry.write_size;
                if self.cursor >= u32::try_from(HEADER_BYTES).map_err(|_| Error::InvalidLayout)? {
                    self.advance(if self.candidate().length == 0 {
                        Phase::PrepareSync
                    } else {
                        Phase::Payload
                    });
                }
            }
            Phase::Payload => {
                self.cursor += self.geometry.write_size;
                if self.cursor as usize >= self.candidate().length {
                    self.advance(Phase::PrepareSync);
                }
            }
            Phase::PrepareSync => self.advance(Phase::Marker),
            Phase::Marker => self.advance(Phase::FinalSync),
            Phase::FinalSync => self.advance(Phase::VerifyMarker),
            Phase::VerifyMarker => {
                if !scratch[..self.geometry.write_size as usize]
                    .iter()
                    .all(|&b| b == 0xa5)
                {
                    return Err(Error::Corrupt.into());
                }
                self.advance(Phase::VerifyHeader);
            }
            Phase::VerifyHeader => {
                if scratch[..HEADER_BYTES] != self.header {
                    return Err(Error::Corrupt.into());
                }
                self.crc = Crc32c::new();
                if self.candidate().length == 0 {
                    self.promote_commit()?;
                } else {
                    self.advance(Phase::VerifyPayload);
                }
            }
            Phase::VerifyPayload => {
                let start = self.cursor as usize;
                let count = MAX_WRITE.min(self.candidate().length - start);
                if scratch[..count] != self.slots[self.spare()][start..start + count] {
                    return Err(Error::Corrupt.into());
                }
                self.crc.update(&scratch[..count]);
                self.cursor += u32::try_from(count).map_err(|_| Error::Capacity)?;
                if self.cursor as usize == self.candidate().length {
                    self.promote_commit()?;
                }
            }
            Phase::RecoverSync
            | Phase::ScanMarker
            | Phase::ScanHeader
            | Phase::ScanPayload
            | Phase::Reload => self.completed_recovery(scratch)?,
            Phase::Idle | Phase::Crc => return Err(AsyncError::InvalidOperation),
        }
        Ok(())
    }
    fn completed_recovery(&mut self, scratch: &mut [u8; MAX_WRITE]) -> Result<(), AsyncError> {
        match self.phase {
            Phase::RecoverSync => {
                self.target = 0;
                self.advance(Phase::ScanMarker);
            }
            Phase::ScanMarker => {
                if scratch[..self.geometry.write_size as usize]
                    .iter()
                    .all(|&b| b == 0xa5)
                {
                    self.advance(Phase::ScanHeader);
                } else {
                    self.next_bank()?;
                }
            }
            Phase::ScanHeader => {
                self.header.copy_from_slice(&scratch[..HEADER_BYTES]);
                let (snapshot, crc) = decode_header::<N>(
                    &self.header,
                    self.geometry,
                    self.layout,
                    self.payload_offset,
                )?;
                self.banks[self.target as usize] = Some(Bank { snapshot, crc });
                self.crc = Crc32c::new();
                if snapshot.length == 0 {
                    if self.crc.finish() != crc {
                        return Err(Error::Corrupt.into());
                    }
                    self.next_bank()?;
                } else {
                    self.advance(Phase::ScanPayload);
                }
            }
            Phase::ScanPayload | Phase::Reload => {
                let bank = self.scan_bank().ok_or(Error::Corrupt)?;
                let start = self.cursor as usize;
                let count = MAX_WRITE.min(bank.snapshot.length - start);
                self.crc.update(&scratch[..count]);
                if self.phase == Phase::Reload {
                    let spare = self.spare();
                    self.slots[spare][start..start + count].copy_from_slice(&scratch[..count]);
                }
                self.cursor += u32::try_from(count).map_err(|_| Error::Capacity)?;
                if self.cursor as usize == bank.snapshot.length {
                    if self.crc.finish() != bank.crc {
                        return Err(Error::Corrupt.into());
                    }
                    if self.phase == Phase::Reload {
                        self.promote_recovery(Some(bank.snapshot));
                    } else {
                        self.next_bank()?;
                    }
                }
            }
            _ => return Err(AsyncError::InvalidOperation),
        }
        Ok(())
    }
    fn promote_commit(&mut self) -> Result<(), AsyncError> {
        let (_, expected) = decode_header::<N>(
            &self.header,
            self.geometry,
            self.layout,
            self.payload_offset,
        )?;
        if self.crc.finish() != expected {
            return Err(Error::Corrupt.into());
        }
        self.current = Some(self.candidate());
        self.committed = self.spare();
        self.current_bank = self.target;
        self.poisoned = false;
        self.finish(Ok(()));
        Ok(())
    }
    fn next_bank(&mut self) -> Result<(), AsyncError> {
        if self.target == 0 {
            self.target = 1;
            self.advance(Phase::ScanMarker);
            return Ok(());
        }
        self.target = match self.banks {
            [Some(a), Some(b)] if a.snapshot.revision == b.snapshot.revision => {
                return Err(Error::Corrupt.into());
            }
            [Some(a), Some(b)] => u32::from(b.snapshot.revision > a.snapshot.revision),
            [None, Some(_)] => 1,
            _ => 0,
        };
        let winner = self.scan_bank();
        self.crc = Crc32c::new();
        match winner {
            None => self.promote_recovery(None),
            Some(bank) if bank.snapshot.length == 0 => self.promote_recovery(Some(bank.snapshot)),
            Some(_) => self.advance(Phase::Reload),
        }
        Ok(())
    }
    fn promote_recovery(&mut self, snapshot: Option<Snapshot>) {
        self.current = snapshot;
        self.current_bank = self.target;
        self.committed = self.spare();
        self.poisoned = false;
        self.finish(Ok(()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[derive(Debug)]
    struct NoIo;
    impl AsyncMedium for NoIo {
        fn geometry(&self) -> Geometry {
            Geometry {
                capacity: 1024,
                erase_size: 64,
                write_size: 4,
            }
        }
        fn begin(&mut self, _operation: MediumOp<'_>) -> Result<(), StartError> {
            panic!("preflight must not perform I/O")
        }
        fn poll(&mut self, _out: &mut [u8]) -> Poll<Result<Completion, Error>> {
            panic!("preflight must not poll")
        }
    }
    #[test]
    fn test_should_reject_exhausted_token_without_replacing_a_retained_result() {
        let mut s = IncrementalSnapshot::<_, 128>::new_unrecovered(
            NoIo,
            Layout {
                start: 0,
                bank_bytes: 512,
            },
        )
        .unwrap_or_else(|e| panic!("new {e:?}"));
        s.poisoned = false;
        s.counter = u64::MAX;
        let id = CommitId(u64::MAX);
        let snapshot = Snapshot {
            revision: 1,
            length: 0,
        };
        s.current = Some(snapshot);
        s.job = Job::Commit { id, snapshot };
        s.terminal = Some(Ok(()));
        assert_eq!(s.begin_commit(1, b"next"), Err(AsyncError::TokenExhausted));
        assert_eq!(
            s.poll_commit(id),
            Poll::Ready(Ok(DurableReceipt { id, snapshot }))
        );
        assert_eq!(s.load_committed(&mut []), Ok(Some(snapshot)));
    }
}
