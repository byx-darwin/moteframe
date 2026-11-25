//! Static job lifecycle, physical arbitration, and explicit response/event admission.
use crate::{
    Backend, BackendError, Error, HealthStart, Latest, Module, Outcome, Reason, ResourceSet,
    ResultInfo, ResultSource, StopProgress, TriggerSource,
};
/// A nonzero boot-local identifier. A product must use one manager/sequence per boot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobId(u32);
impl JobId {
    /// Validate an incoming job identifier; zero is never a real job.
    /// # Errors
    /// Zero returns `InvalidField`.
    pub const fn new(value: u32) -> Result<Self, Error> {
        if value == 0 {
            Err(Error::InvalidField)
        } else {
            Ok(Self(value))
        }
    }
    /// Decode the exact four-byte STOP/QUERY business parameter.
    /// # Errors
    /// Rejects missing/trailing bytes and ID0.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() != 4 {
            return Err(Error::InvalidLength);
        }
        Self::new(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }
    /// Encode the job parameter without a frame header or status.
    /// # Errors
    /// Short caller storage returns Capacity before writing.
    pub fn encode(self, output: &mut [u8]) -> Result<usize, Error> {
        if output.len() < 4 {
            return Err(Error::Capacity);
        }
        output[..4].copy_from_slice(&self.0.to_le_bytes());
        Ok(4)
    }
    /// Exact wire value.
    #[must_use]
    pub const fn get(self) -> u32 {
        self.0
    }
}
/// Ownership controls disconnect cleanup, separately from action trigger and authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// App session work is cancelled/muted on disconnect.
    Application,
    /// Device-owned policy work is not stopped by an App disconnect.
    Device,
}
/// Proposed job states, not the core module STATUS dictionary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum JobState {
    /// Accepted; START response has not yet been admitted.
    Accepted = 1,
    /// Response admitted, work active.
    Running = 2,
    /// Definite successful process completion.
    Completed = 3,
    /// Definite cancellation/cleanup.
    Cancelled = 4,
    /// Definite failed completion/cleanup.
    Failed = 5,
}
impl JobState {
    const fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Failed)
    }
}
/// Read-only metadata snapshot. It contains no physiological payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobSnapshot {
    /// Stable identity while retained.
    pub id: JobId,
    /// Owning module, checked by every job operation.
    pub module: Module,
    /// Actual state; no resource release is inferred from a requested stop.
    pub state: JobState,
    /// Application versus device ownership.
    pub owner: Owner,
    /// Action trigger source.
    pub source: TriggerSource,
    /// Explicit physical lease.
    pub resources: ResourceSet,
    /// Definite terminal outcome, or None while active.
    pub outcome: Option<Outcome>,
}
/// Typed terminal event. The runtime maps outcome to existing core status/reason codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EndEvent {
    /// Stable job identity.
    pub job: JobId,
    /// Explicit owner module.
    pub module: Module,
    /// Actual trigger, not algorithm provenance.
    pub source: TriggerSource,
    /// Zero outside temperature; requested supported mode otherwise.
    pub temp_mode: u8,
    /// Definite lifecycle outcome; does not assert a valid measured value exists.
    pub outcome: Outcome,
}
/// A retained result for transport admission; borrowing prevents hidden heap copies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResultEvent<'a> {
    /// Explicit job association outside the unchanged LATEST business envelope.
    pub job: JobId,
    /// Stable per-job sample identity for deduplication and admission.
    pub sample_id: u64,
    /// Actual provenance, not automatically inserted into frozen payloads.
    pub source: ResultSource,
    /// Actual LATEST envelope; the selected concrete body codec has validated its payload.
    /// Product provenance/declaration and physiological accuracy remain separate checks.
    pub latest: Latest<'a>,
}
impl ResultEvent<'_> {
    /// Check actual carrier/direction capacity before queue admission and `confirm_result`.
    /// No maximum schema body is assumed sendable on every carrier or negotiated direction.
    /// # Errors
    /// Overflow,disabled direction or insufficient capacity is explicit and never truncates data.
    pub fn check_carrier(
        self,
        carrier: crate::ResultCarrier,
        outbound_max: usize,
    ) -> Result<usize, Error> {
        self.latest.check_carrier(carrier, outbound_max)
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Admission {
    AwaitingResponse,
    Admitted,
    Muted,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum ResultRetention {
    Empty,
    Fetched,
}
#[derive(Clone, Copy)]
struct Job {
    request: HealthStart,
    id: JobId,
    state: JobState,
    owner: Owner,
    source: TriggerSource,
    resources: ResourceSet,
    outcome: Option<Outcome>,
    admission: Admission,
    end_admitted: bool,
    stopping: bool,
    pending: ResultRetention,
    last_sample: u64,
    result_outstanding: bool,
}
impl Job {
    fn snapshot(self) -> JobSnapshot {
        JobSnapshot {
            id: self.id,
            module: self.request.module(),
            state: self.state,
            owner: self.owner,
            source: self.source,
            resources: self.resources,
            outcome: self.outcome,
        }
    }
    fn finish(&mut self, outcome: Outcome) -> Result<(), Error> {
        if let Outcome::Cancelled(reason) | Outcome::Failed(reason) = outcome
            && !reason.applies_to(self.request.module().wire())
        {
            return Err(Error::InvalidField);
        }
        if matches!(
            outcome,
            Outcome::Cancelled(Reason::None) | Outcome::Failed(Reason::None)
        ) {
            return Err(Error::InvalidField);
        }
        self.state = match outcome {
            Outcome::Completed => JobState::Completed,
            Outcome::Cancelled(_) => JobState::Cancelled,
            Outcome::Failed(_) => JobState::Failed,
        };
        self.outcome = Some(outcome);
        self.pending = ResultRetention::Empty;
        self.result_outstanding = false;
        Ok(())
    }
}
// Terminal slots cannot retain an outstanding result, but their query/replay
// identity and admission state remain observable after working-slot reuse.
#[derive(Clone, Copy)]
struct ArchivedJob {
    module: Module,
    record_type: u16,
    temp_mode: u8,
    id: JobId,
    owner: Owner,
    source: TriggerSource,
    resources: ResourceSet,
    outcome: Option<Outcome>,
    admission: Admission,
    end_admitted: bool,
    last_sample: [u8; 8],
}
impl ArchivedJob {
    fn save(job: Job) -> Self {
        Self {
            module: job.request.module(),
            record_type: job.request.record_type(),
            temp_mode: job.request.temp_mode(),
            id: job.id,
            owner: job.owner,
            source: job.source,
            resources: job.resources,
            outcome: job.outcome,
            admission: job.admission,
            end_admitted: job.end_admitted,
            last_sample: job.last_sample.to_le_bytes(),
        }
    }
    fn restore(self) -> Result<Job, Error> {
        // Mode/context are only passed to a backend when starting active work.
        // Archived work is terminal and can never restart or fetch a result.
        let kind = self.record_type.to_le_bytes();
        let request = HealthStart::decode(
            self.module.wire(),
            &[1, kind[0], kind[1], 0, 0, 0, 0, self.temp_mode],
        )?;
        Ok(Job {
            request,
            id: self.id,
            state: match self.outcome.ok_or(Error::InvalidState)? {
                Outcome::Completed => JobState::Completed,
                Outcome::Cancelled(_) => JobState::Cancelled,
                Outcome::Failed(_) => JobState::Failed,
            },
            owner: self.owner,
            source: self.source,
            resources: self.resources,
            outcome: self.outcome,
            admission: self.admission,
            end_admitted: self.end_admitted,
            stopping: false,
            pending: ResultRetention::Empty,
            last_sample: u64::from_le_bytes(self.last_sample),
            result_outstanding: false,
        })
    }
}
enum JobMut<'a> {
    Active(&'a mut Job),
    Archived {
        target: &'a mut ArchivedJob,
        value: Job,
    },
}
impl core::ops::Deref for JobMut<'_> {
    type Target = Job;
    fn deref(&self) -> &Job {
        match self {
            Self::Active(job) => job,
            Self::Archived { value, .. } => value,
        }
    }
}
impl core::ops::DerefMut for JobMut<'_> {
    fn deref_mut(&mut self) -> &mut Job {
        match self {
            Self::Active(job) => job,
            Self::Archived { value, .. } => value,
        }
    }
}
impl Drop for JobMut<'_> {
    fn drop(&mut self) {
        if let Self::Archived { target, value } = self {
            **target = ArchivedJob::save(*value);
        }
    }
}
/// A statically bounded job engine. It neither registers capabilities nor installs any backend.
/// Terminal slots are reusable only after end admission or after muted disconnect cleanup.
/// An additional eight-entry archive preserves the newest terminal job IDs when active slots
/// are reused. The fixed upper bound is N working slots plus eight archived terminal snapshots.
pub struct JobManager<const N: usize> {
    slots: [Option<Job>; N],
    // Terminal archive entries never carry a pending result. Keep the large
    // metadata in working-slot storage instead of all N+8 retained jobs.
    pending: [Option<ResultInfo>; N],
    terminal_archive: [Option<ArchivedJob>; 8],
    next_id: Option<u32>,
}
impl<const N: usize> core::fmt::Debug for JobManager<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("JobManager")
            .field("capacity", &N)
            .field("next_id", &self.next_id)
            .field(
                "retained_jobs",
                &(self.slots.iter().flatten().count()
                    + self.terminal_archive.iter().flatten().count()),
            )
            .finish_non_exhaustive()
    }
}
impl<const N: usize> JobManager<N> {
    /// Create one boot-scoped sequence with a nonzero first ID. Reconstructing/resetting it within
    /// the same boot requires the caller to preserve the next-ID high watermark, never start at1 again.
    /// # Errors
    /// First ID zero is rejected.
    pub fn new(first_id: u32) -> Result<Self, Error> {
        JobId::new(first_id)?;
        Ok(Self {
            slots: [None; N],
            pending: [None; N],
            terminal_archive: [None; 8],
            next_id: Some(first_id),
        })
    }
    /// The next unused ID, or None when exhausted. Preserve this before reconstructing the manager.
    #[must_use]
    pub const fn next_id(&self) -> Option<u32> {
        self.next_id
    }
    fn job(&self, module: Module, id: JobId) -> Result<Job, Error> {
        if let Some(job) = self
            .slots
            .iter()
            .flatten()
            .find(|j| j.id == id && j.request.module() == module)
        {
            return Ok(*job);
        }
        self.terminal_archive
            .iter()
            .flatten()
            .find(|j| j.id == id && j.module == module)
            .ok_or(Error::NotFound)?
            .restore()
    }
    fn job_mut(&mut self, module: Module, id: JobId) -> Result<JobMut<'_>, Error> {
        if let Some(job) = self
            .slots
            .iter_mut()
            .flatten()
            .find(|j| j.id == id && j.request.module() == module)
        {
            return Ok(JobMut::Active(job));
        }
        let target = self
            .terminal_archive
            .iter_mut()
            .flatten()
            .find(|j| j.id == id && j.module == module)
            .ok_or(Error::NotFound)?;
        let value = target.restore()?;
        Ok(JobMut::Archived { target, value })
    }
    fn pending_info(&self, module: Module, id: JobId) -> Result<Option<ResultInfo>, Error> {
        if self.job(module, id)?.pending == ResultRetention::Empty {
            return Ok(None);
        }
        let index = self
            .slots
            .iter()
            .position(|j| j.is_some_and(|j| j.id == id && j.request.module() == module))
            .ok_or(Error::InvalidState)?;
        self.pending[index].map(Some).ok_or(Error::InvalidState)
    }
    fn set_pending_info(
        &mut self,
        module: Module,
        id: JobId,
        info: Option<ResultInfo>,
    ) -> Result<(), Error> {
        let index = self
            .slots
            .iter()
            .position(|j| j.is_some_and(|j| j.id == id && j.request.module() == module))
            .ok_or(Error::InvalidState)?;
        self.pending[index] = info;
        Ok(())
    }
    fn archive(&mut self, job: Job) {
        let job = ArchivedJob::save(job);
        if let Some(slot) = self.terminal_archive.iter_mut().find(|j| j.is_none()) {
            *slot = Some(job);
            return;
        }
        let oldest = self
            .terminal_archive
            .iter()
            .enumerate()
            .filter_map(|(i, j)| j.map(|j| (i, j.id.get())))
            .min_by_key(|(_, id)| *id);
        if let Some((index, id)) = oldest
            && job.id.get() > id
        {
            self.terminal_archive[index] = Some(job);
        }
    }
    /// Validate backend availability/context, arbitrate resources and start actual work.
    /// # Errors
    /// Unavailable/unsupported never returns a successful job. Failed starts burn any issued ID
    /// to prevent reuse; the backend must have rolled back before returning an error.
    pub fn start<B: Backend>(
        &mut self,
        request: HealthStart,
        owner: Owner,
        source: TriggerSource,
        backend: &mut B,
    ) -> Result<JobId, Error> {
        if request.context_id() != 0 && !backend.context_is_valid(request.context_id()) {
            return Err(Error::InvalidField);
        }
        let resources = backend.resources(request).map_err(map_backend)?;
        if self
            .slots
            .iter()
            .flatten()
            .any(|j| !j.state.terminal() && resources.conflicts(j.resources))
        {
            return Err(Error::Busy);
        }
        let index = self
            .slots
            .iter()
            .position(Option::is_none)
            .or_else(|| {
                self.slots
                    .iter()
                    .enumerate()
                    .filter_map(|(i, j)| {
                        j.filter(|j| {
                            j.state.terminal()
                                && (j.end_admitted || j.admission == Admission::Muted)
                        })
                        .map(|j| (i, j.id.get()))
                    })
                    .min_by_key(|(_, id)| *id)
                    .map(|(i, _)| i)
            })
            .ok_or(Error::Capacity)?;
        let id = JobId::new(self.next_id.ok_or(Error::Exhausted)?)?;
        self.next_id = id.get().checked_add(1);
        if let Some(old) = self.slots[index].take() {
            self.archive(old);
        }
        backend.start(id, request).map_err(map_backend)?;
        self.pending[index] = None;
        self.slots[index] = Some(Job {
            request,
            id,
            state: JobState::Accepted,
            owner,
            source,
            resources,
            outcome: None,
            admission: Admission::AwaitingResponse,
            end_admitted: false,
            stopping: false,
            pending: ResultRetention::Empty,
            last_sample: 0,
            result_outstanding: false,
        });
        Ok(id)
    }
    /// Query a retained job; module identity is mandatory, even for terminal jobs.
    /// # Errors
    /// Unknown, evicted or wrong-module identity is `NotFound`.
    pub fn query(&self, module: Module, id: JobId) -> Result<JobSnapshot, Error> {
        Ok(self.job(module, id)?.snapshot())
    }
    /// Confirm actual START-response transport admission before any result/end can be emitted.
    /// # Errors
    /// Muted disconnected jobs cannot be rearmed by a stale response acknowledgement.
    pub fn confirm_start_response(&mut self, module: Module, id: JobId) -> Result<(), Error> {
        let mut job = self.job_mut(module, id)?;
        if job.admission == Admission::Muted {
            return Err(Error::InvalidState);
        }
        job.admission = Admission::Admitted;
        if job.state == JobState::Accepted {
            job.state = JobState::Running;
        }
        Ok(())
    }
    /// Request actual stop. Definite terminal jobs and repeated pending stops are idempotent.
    /// # Errors
    /// A stop error retains the active resource lease; it is never reported as stopped.
    pub fn stop<B: Backend>(
        &mut self,
        module: Module,
        id: JobId,
        backend: &mut B,
    ) -> Result<(), Error> {
        let mut job = self.job_mut(module, id)?;
        if job.state.terminal() || job.stopping {
            return Ok(());
        }
        match backend.stop(id).map_err(map_backend)? {
            StopProgress::Stopped => {
                let reason = if job.admission == Admission::Muted {
                    Reason::Disconnected
                } else {
                    Reason::UserCancelled
                };
                job.finish(Outcome::Cancelled(reason))?;
            }
            StopProgress::Stopping => job.stopping = true,
        }
        Ok(())
    }
    /// Poll definite completion. Fetched or retained backend results cannot be discarded by end.
    /// Drain terminal results until `fetch_result` returns None before admitting the terminal event.
    /// # Errors
    /// Polling error keeps resources leased. Pending result requires admission or explicit stop.
    pub fn poll_completion<B: Backend>(
        &mut self,
        module: Module,
        id: JobId,
        backend: &mut B,
    ) -> Result<JobState, Error> {
        let mut job = self.job_mut(module, id)?;
        if job.state.terminal() {
            return Ok(job.state);
        }
        if job.pending == ResultRetention::Fetched {
            return Err(Error::InvalidState);
        }
        if let Some(outcome) = backend.completion(id).map_err(map_backend)? {
            let pending = backend.has_pending_result(id).map_err(map_backend)?;
            job.finish(outcome)?;
            job.result_outstanding = pending;
        }
        Ok(job.state)
    }
    /// Peek the same immutable end until actual transport admission. No event before START response.
    /// # Errors
    /// Unknown/wrong-module job is `NotFound`.
    pub fn pending_end(&self, module: Module, id: JobId) -> Result<Option<EndEvent>, Error> {
        let job = self.job(module, id)?;
        if job.admission != Admission::Admitted
            || job.end_admitted
            || job.result_outstanding
            || job.pending == ResultRetention::Fetched
        {
            return Ok(None);
        }
        Ok(job.outcome.map(|outcome| EndEvent {
            job: id,
            module,
            source: job.source,
            temp_mode: job.request.temp_mode(),
            outcome,
        }))
    }
    /// Confirm end admission once; repeated acknowledgements are harmless and no end is regenerated.
    /// # Errors
    /// Rejects before terminal completion/START response or after disconnect muting.
    pub fn confirm_end_event(&mut self, module: Module, id: JobId) -> Result<(), Error> {
        let mut job = self.job_mut(module, id)?;
        if job.admission != Admission::Admitted
            || !job.state.terminal()
            || job.result_outstanding
            || job.pending == ResultRetention::Fetched
        {
            return Err(Error::InvalidState);
        }
        job.end_admitted = true;
        Ok(())
    }
    /// Borrow an actual retained result in caller storage. Retries keep the same identity; the
    /// backend contract requires immutable bytes until admission. No sample means None, never zero.
    /// # Errors
    /// Rejects premature events, mismatched type, nonmonotonic sample or invalid metadata/length.
    pub fn fetch_result<'a, B: Backend>(
        &mut self,
        module: Module,
        id: JobId,
        backend: &mut B,
        output: &'a mut [u8],
    ) -> Result<Option<ResultEvent<'a>>, Error> {
        let pending = self.pending_info(module, id)?;
        let mut job = self.job_mut(module, id)?;
        if job.admission != Admission::Admitted
            || (job.state.terminal() && (job.end_admitted || !job.result_outstanding))
        {
            return Err(Error::InvalidState);
        }
        let Some(info) = backend.result(id, output).map_err(map_backend)? else {
            if job.pending == ResultRetention::Fetched {
                return Err(Error::InvalidState);
            }
            job.result_outstanding = false;
            return Ok(None);
        };
        job.result_outstanding = true;
        if info.sample_id == 0 || info.record_type != job.request.record_type() {
            return Err(Error::InvalidField);
        }
        if info.length > output.len() {
            return Err(Error::InvalidLength);
        }
        if job.last_sample != 0 {
            let previous = job.last_sample;
            if info.sample_id == previous {
                return Ok(None);
            }
            if info.sample_id < previous {
                return Err(Error::InvalidState);
            }
        }
        if pending.is_some_and(|pending| pending != info) {
            return Err(Error::InvalidState);
        }
        let latest = Latest::new(
            module.wire(),
            info.record_type,
            info.schema,
            info.timestamp,
            &output[..info.length],
        )?;
        crate::validate_result_body(
            module.wire(),
            info.record_type,
            info.schema,
            latest.payload(),
            info.source,
        )?;
        job.pending = ResultRetention::Fetched;
        drop(job);
        self.set_pending_info(module, id, Some(info))?;
        Ok(Some(ResultEvent {
            job: id,
            sample_id: info.sample_id,
            source: info.source,
            latest,
        }))
    }
    /// Acknowledge result queue admission exactly once; duplicate acknowledgements do not recount.
    /// # Errors
    /// Unknown/unfetched identity fails. Backend failure leaves the same pending result retryable.
    pub fn confirm_result<B: Backend>(
        &mut self,
        module: Module,
        id: JobId,
        sample_id: u64,
        backend: &mut B,
    ) -> Result<(), Error> {
        let pending = self.pending_info(module, id)?;
        let mut job = self.job_mut(module, id)?;
        if job.admission != Admission::Admitted {
            return Err(Error::InvalidState);
        }
        if sample_id != 0 && job.last_sample == sample_id {
            return Ok(());
        }
        if pending.is_none_or(|p| p.sample_id != sample_id) {
            return Err(Error::InvalidState);
        }
        backend
            .acknowledge_result(id, sample_id)
            .map_err(map_backend)?;
        job.last_sample = sample_id;
        job.pending = ResultRetention::Empty;
        drop(job);
        self.set_pending_info(module, id, None)?;
        Ok(())
    }
    /// Mute all application events immediately and request cleanup of every active App job.
    /// Device-owned jobs remain active. Failed/asynchronous stop retains resource ownership until
    /// explicit recovery or `poll_completion` confirms cleanup. Every App job is attempted on error.
    /// # Errors
    /// Returns the first actual stop error after attempting all App cleanup, never fake success.
    pub fn disconnect<B: Backend>(&mut self, backend: &mut B) -> Result<(), Error> {
        let mut error = None;
        for (job, pending) in self.slots.iter().zip(&mut self.pending) {
            if job.is_some_and(|j| j.owner == Owner::Application) {
                *pending = None;
            }
        }
        for job in self
            .slots
            .iter_mut()
            .flatten()
            .filter(|j| j.owner == Owner::Application)
        {
            job.admission = Admission::Muted;
            job.pending = ResultRetention::Empty;
            job.result_outstanding = false;
            if job.state.terminal() || job.stopping {
                continue;
            }
            match backend.stop(job.id) {
                Ok(StopProgress::Stopped) => {
                    if let Err(e) = job.finish(Outcome::Cancelled(Reason::Disconnected)) {
                        error.get_or_insert(e);
                    }
                }
                Ok(StopProgress::Stopping) => job.stopping = true,
                Err(e) => {
                    error.get_or_insert(map_backend(e));
                }
            }
        }
        for job in self
            .terminal_archive
            .iter_mut()
            .flatten()
            .filter(|j| j.owner == Owner::Application)
        {
            job.admission = Admission::Muted;
        }
        if let Some(error) = error {
            Err(error)
        } else {
            Ok(())
        }
    }
}
const fn map_backend(error: BackendError) -> Error {
    match error {
        BackendError::Unsupported => Error::Unsupported,
        BackendError::Unavailable => Error::Unavailable,
        BackendError::Capacity => Error::Capacity,
        BackendError::Fault => Error::Backend,
    }
}
