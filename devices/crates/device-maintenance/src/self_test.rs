//! Criterion-backed self-test jobs; external verdicts require a trusted verifier.
use crate::{Authority, Error, Profile};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Controlled test method from the frozen dictionary.
pub enum Method {
    /// Automatic evaluation of actual criteria-backed observations.
    Automatic = 0,
    /// Requires verified human/instrument evidence.
    Manual = 1,
    /// Requires automatic checks and controlled external judgement.
    Hybrid = 2,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Frozen self-test states0..7; terminal results are immutable.
pub enum JobState {
    /// Accepted and queued.
    Queued = 0,
    /// Actual backend work is running.
    Running = 1,
    /// Actual criteria or verified evidence passed.
    Passed = 2,
    /// Actual criteria or backend work failed.
    Failed = 3,
    /// Required real observation or support is unavailable.
    Unsupported = 4,
    /// Deadline expired after confirmed cleanup.
    TimedOut = 5,
    /// Cancelled after confirmed cleanup.
    Cancelled = 6,
    /// Waiting for independently verified external judgement.
    WaitManual = 7,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Trusted judgement origin; never request-supplied.
pub enum Origin {
    /// Automatic evaluation of actual criteria-backed observations.
    Automatic = 0,
    /// Verified human observation.
    Human = 1,
    /// Verified station-instrument judgement.
    Instrument = 2,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Claimed or independently verified conclusion.
pub enum Verdict {
    /// Passing conclusion.
    Pass,
    /// Failing conclusion.
    Fail,
}
#[derive(Clone, Copy, PartialEq, Eq, Default)]
/// Actual backend metric; Debug omits measured values.
pub struct Metric {
    /// Nonzero controlled manifest identity.
    pub id: u16,
    /// Actual integer metric value.
    pub value: i32,
    /// Decimal scaling exponent, fixed by the criterion.
    pub scale_exp: i8,
    /// Frozen unit0..5, fixed by the criterion.
    pub unit: u8,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Mandatory product criterion in exact integer/exponent/unit representation.
pub struct MetricCriterion {
    /// Nonzero controlled manifest identity.
    pub id: u16,
    /// Inclusive minimum passing integer value.
    pub minimum: i32,
    /// Inclusive maximum passing integer value.
    pub maximum: i32,
    /// Decimal scaling exponent, fixed by the criterion.
    pub scale_exp: i8,
    /// Frozen unit0..5, fixed by the criterion.
    pub unit: u8,
}
#[derive(Debug, Clone, Copy)]
/// Versioned controlled product test manifest entry. Automatic tests need actual
/// metric limits; manual/hybrid tests need allowed evidence kinds and origins.
pub struct TestSpec<'a> {
    /// Nonzero controlled manifest identity.
    pub id: u16,
    /// Nonzero immutable product criterion revision.
    pub criterion_rev: u16,
    /// Automatic/manual/hybrid method from the product manifest.
    pub method: Method,
    /// Sorted unique required metrics, at most eight.
    pub criteria: &'a [MetricCriterion],
    /// Positive minimum timeout in milliseconds.
    pub min_timeout_ms: u32,
    /// Maximum timeout, at least the minimum.
    pub max_timeout_ms: u32,
    /// Requires independent dangerous-write scope when true.
    pub dangerous: bool,
    /// Sorted unique nonzero evidence-kind IDs allowed by this product.
    pub evidence_kinds: &'a [u16],
    /// Allowed bits1 human and2 instrument; reserved bits zero.
    pub allowed_origins: u8,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Actual backend startup phase; job acceptance is not a pass.
pub enum Phase {
    /// Accepted and queued.
    Queued,
    /// Actual backend work is running.
    Running,
    /// Actual setup is ready for controlled external evidence.
    ManualReady,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Actual backend progress, never a backend-supplied PASS verdict.
pub enum Observation {
    /// No new actual observation yet.
    Pending,
    /// Actual backend work is running.
    Running,
    /// Number of actual initialized metrics in the fixed buffer.
    Readings(usize),
    /// Actual setup is ready for controlled external evidence.
    ManualReady,
}
/// Actual controlled self-test backend, with no default implementation.
/// Missing hardware must return unsupported/error. Metrics must be real valid
/// observations; cancel confirms actual cleanup and is safe after completion.
pub trait TestPort {
    /// Report whether this real backend implements the controlled test.
    fn supports(&self, test: u16) -> bool;
    /// Start actual controlled test work; acceptance never proves passing.
    /// # Errors
    /// Returns explicit policy, lifecycle, criterion, capacity, clock or backend failures.
    fn start(&mut self, test: u16, criterion_rev: u16) -> Result<Phase, Error>;
    /// Advance from actual backend observations; thresholds determine automatic results.
    /// # Errors
    /// Returns explicit policy, lifecycle, criterion, capacity, clock or backend failures.
    fn poll(&mut self, test: u16, out: &mut [Metric; 8]) -> Result<Observation, Error>;
    /// Stop actual work, preserving terminal results and making repeats idempotent.
    /// # Errors
    /// Returns explicit policy, lifecycle, criterion, capacity, clock or backend failures.
    fn cancel(&mut self, test: u16) -> Result<(), Error>;
}
#[derive(Clone, Copy, PartialEq, Eq)]
/// Untrusted external-result fields; evidence and all bindings are verified.
pub struct ExternalRequest {
    /// Nonzero RUN-bound job identity.
    pub job: u32,
    /// RUN-bound controlled test identity.
    pub test: u16,
    /// Phone-claimed verdict, accepted only if independent proof agrees.
    pub result: Verdict,
    /// Authenticated station identity, never request-granted authority.
    pub station_id: u32,
    /// Opaque evidence reference; Debug must not expose it.
    pub evidence: [u8; 16],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Independently trusted evidence resolution, never an echoed request.
pub struct VerifiedEvidence {
    /// Authenticated station identity, never request-granted authority.
    pub station_id: u32,
    /// Trustworthy origin fixed by the real backend or evidence verifier.
    pub origin: Origin,
    /// Verified product evidence-kind identity.
    pub kind: u16,
    /// Independently verified conclusion.
    pub verdict: Verdict,
}
/// Trusted evidence resolver. It must independently verify job, test,
/// criterion revision, station identity, evidence type and actual verdict.
/// There is no always-successful production verifier.
pub trait EvidenceVerifier {
    /// Independently verify evidence and immutable bindings, never echo phone claims.
    /// # Errors
    /// Returns explicit policy, lifecycle, criterion, capacity, clock or backend failures.
    fn verify(
        &self,
        request: &ExternalRequest,
        criterion_rev: u16,
    ) -> Result<VerifiedEvidence, Error>;
}
#[derive(Clone, Copy, PartialEq, Eq)]
/// Job-bound result with at most eight metrics; Debug omits evidence/values.
pub struct JobResult {
    /// Nonzero connection-local job identity, never reused.
    pub job_id: u32,
    /// Frozen job state0..7.
    pub state: JobState,
    /// Job-specific reason; zero means no additional reason.
    pub reason: u16,
    /// Immutable RUN-bound test ID.
    pub test_id: u16,
    /// Nonzero immutable product criterion revision.
    pub criterion_rev: u16,
    /// Trustworthy origin fixed by the real backend or evidence verifier.
    pub origin: Origin,
    /// Authenticated station identity, never request-granted authority.
    pub station_id: u32,
    /// Opaque evidence reference; Debug must not expose it.
    pub evidence: [u8; 16],
    values: [Metric; 8],
    count: usize,
}
impl JobResult {
    /// Borrow only initialized actual metrics.
    #[must_use]
    pub fn values(&self) -> &[Metric] {
        &self.values[..self.count]
    }
}
impl core::fmt::Debug for Metric {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Metric")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}
impl core::fmt::Debug for ExternalRequest {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ExternalRequest")
            .field("job", &self.job)
            .finish_non_exhaustive()
    }
}
impl core::fmt::Debug for JobResult {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("JobResult")
            .field("state", &self.state)
            .field("value_count", &self.count)
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy)]
struct Job {
    result: JobResult,
    spec: usize,
    deadline: u64,
}
/// One active job and eight retained terminal results per connection.
/// Product criteria, trusted station evidence and actual backend cleanup govern
/// transitions. Call disconnect before dropping an active engine. This actor
/// has no hidden timer; caller time must come from a trusted monotonic source.
pub struct SelfTestEngine<'a> {
    port: &'a mut dyn TestPort,
    specs: &'a [TestSpec<'a>],
    auth: &'a dyn Authority,
    active: Option<Job>,
    terminal: [Option<JobResult>; 8],
    next_terminal: usize,
    next_job: u32,
    last_now: u64,
    closed: bool,
}
impl core::fmt::Debug for SelfTestEngine<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SelfTestEngine")
            .field("active", &self.active.map(|j| j.result.job_id))
            .field("closed", &self.closed)
            .finish_non_exhaustive()
    }
}
impl<'a> SelfTestEngine<'a> {
    /// Construct only with development/factory policy, real backend and valid criteria.
    /// # Errors
    /// Returns explicit policy, lifecycle, criterion, capacity, clock or backend failures.
    pub fn new(
        profile: Profile,
        port: Option<&'a mut dyn TestPort>,
        specs: &'a [TestSpec<'a>],
        auth: &'a dyn Authority,
    ) -> Result<Self, Error> {
        if profile == Profile::Commercial {
            return Err(Error::Commercial);
        }
        if !auth.allows(crate::Capability::SelfTest) {
            return Err(Error::Denied);
        }
        let port = port.ok_or(Error::Unsupported)?;
        if specs.is_empty() || specs.len() > 254 {
            return Err(Error::CriterionUndefined);
        }
        let mut previous = 0;
        for spec in specs {
            validate_spec(*spec)?;
            if spec.id <= previous {
                return Err(Error::Invalid);
            }
            previous = spec.id;
        }
        Ok(Self {
            port,
            specs,
            auth,
            active: None,
            terminal: [None; 8],
            next_terminal: 0,
            next_job: 1,
            last_now: 0,
            closed: false,
        })
    }
    /// Page only manifest tests actually supported by the backend.
    /// # Errors
    /// Returns explicit policy, lifecycle, criterion, capacity, clock or backend failures.
    pub fn list(&mut self, start: u8, max: u8, out: &mut [u16]) -> Result<(u8, usize), Error> {
        self.check()?;
        if start == 255 || !(1..=8).contains(&max) {
            return Err(Error::Invalid);
        }
        let total = self
            .specs
            .iter()
            .filter(|s| self.port.supports(s.id))
            .count();
        let count = total
            .saturating_sub(usize::from(start))
            .min(usize::from(max))
            .min(out.len());
        if total > usize::from(start) && count == 0 {
            return Err(Error::Capacity);
        }
        for (dst, spec) in out[..count].iter_mut().zip(
            self.specs
                .iter()
                .filter(|s| self.port.supports(s.id))
                .skip(usize::from(start)),
        ) {
            *dst = spec.id;
        }
        Ok((
            if usize::from(start) + count >= total {
                255
            } else {
                u8::try_from(usize::from(start) + count).map_err(|_| Error::Capacity)?
            },
            count,
        ))
    }
    /// Validate scopes, options and timeout, then accept a nonreusing asynchronous job.
    /// # Errors
    /// Returns explicit policy, lifecycle, criterion, capacity, clock or backend failures.
    pub fn run(&mut self, test: u16, options: u16, timeout: u32, now: u64) -> Result<u32, Error> {
        self.advance(now)?;
        if self.active.is_some() {
            return Err(Error::Busy);
        }
        let index = self
            .specs
            .iter()
            .position(|s| s.id == test)
            .ok_or(Error::Unsupported)?;
        let spec = self.specs[index];
        if options != 0 || timeout < spec.min_timeout_ms || timeout > spec.max_timeout_ms {
            return Err(Error::Invalid);
        }
        if spec.dangerous && !self.auth.allows(crate::Capability::DangerousSelfTest) {
            return Err(Error::Denied);
        }
        if !self.port.supports(test) {
            return Err(Error::Unsupported);
        }
        if self.next_job == 0 {
            return Err(Error::Capacity);
        }
        let deadline = now.checked_add(u64::from(timeout)).ok_or(Error::Clock)?;
        let id = self.next_job;
        self.next_job = self.next_job.checked_add(1).unwrap_or(0);
        let result = JobResult {
            job_id: id,
            state: JobState::Queued,
            reason: 0,
            test_id: test,
            criterion_rev: spec.criterion_rev,
            origin: Origin::Automatic,
            station_id: 0,
            evidence: [0; 16],
            values: [Metric::default(); 8],
            count: 0,
        };
        self.active = Some(Job {
            result,
            spec: index,
            deadline,
        });
        let phase = match self.port.start(test, spec.criterion_rev) {
            Ok(phase) => phase,
            Err(error) => {
                self.finish(JobState::Unsupported)?;
                return Err(error);
            }
        };
        self.check()?;
        match phase {
            Phase::Queued => {}
            Phase::Running => self.set_state(JobState::Running),
            Phase::ManualReady => {
                if spec.method == Method::Automatic {
                    self.finish(JobState::Unsupported)?;
                    return Err(Error::InvalidState);
                }
                self.set_state(if spec.criteria.is_empty() {
                    JobState::WaitManual
                } else {
                    JobState::Running
                });
            }
        }
        Ok(id)
    }
    /// Advance from actual backend observations; thresholds determine automatic results.
    /// # Errors
    /// Returns explicit policy, lifecycle, criterion, capacity, clock or backend failures.
    pub fn poll(&mut self, now: u64) -> Result<(), Error> {
        self.advance(now)?;
        let Some(job) = self.active else {
            return Ok(());
        };
        if job.result.state == JobState::WaitManual {
            return Ok(());
        }
        let spec = self.specs[job.spec];
        let mut values = [Metric::default(); 8];
        let observation = match self.port.poll(spec.id, &mut values) {
            Ok(value) => value,
            Err(error) => {
                self.finish(JobState::Failed)?;
                return Err(error);
            }
        };
        self.check()?;
        match observation {
            Observation::Pending => {}
            Observation::Running => self.set_state(JobState::Running),
            Observation::ManualReady => {
                if spec.method == Method::Automatic {
                    self.finish(JobState::Unsupported)?;
                    return Err(Error::InvalidState);
                }
                if !spec.criteria.is_empty() && job.result.count != spec.criteria.len() {
                    return Err(Error::CriterionUndefined);
                }
                self.set_state(JobState::WaitManual);
            }
            Observation::Readings(count) => {
                if count > 8 {
                    self.finish(JobState::Failed)?;
                    return Err(Error::Capacity);
                }
                if count == 0 || spec.criteria.is_empty() {
                    if spec.method == Method::Manual {
                        self.set_state(JobState::WaitManual);
                        return Ok(());
                    }
                    self.finish(JobState::Unsupported)?;
                    return Err(Error::CriterionUndefined);
                }
                if count != spec.criteria.len() {
                    self.finish(JobState::Failed)?;
                    return Err(Error::Invalid);
                }
                let mut passed = true;
                for criterion in spec.criteria {
                    let mut matching = values[..count].iter().filter(|m| m.id == criterion.id);
                    let Some(metric) = matching.next() else {
                        self.finish(JobState::Failed)?;
                        return Err(Error::Invalid);
                    };
                    if matching.next().is_some()
                        || metric.unit != criterion.unit
                        || metric.scale_exp != criterion.scale_exp
                    {
                        self.finish(JobState::Failed)?;
                        return Err(Error::Invalid);
                    }
                    passed &= (criterion.minimum..=criterion.maximum).contains(&metric.value);
                }
                if let Some(active) = self.active.as_mut() {
                    active.result.values[..count].copy_from_slice(&values[..count]);
                    active.result.count = count;
                }
                if !passed {
                    self.finish(JobState::Failed)?;
                } else if spec.method == Method::Automatic {
                    self.finish(JobState::Passed)?;
                } else {
                    self.set_state(JobState::WaitManual);
                }
            }
        }
        Ok(())
    }
    /// Read an authorized active/retained result after enforcing the deadline.
    /// # Errors
    /// Returns explicit policy, lifecycle, criterion, capacity, clock or backend failures.
    pub fn result(&mut self, job: u32, now: u64) -> Result<JobResult, Error> {
        self.advance(now)?;
        self.find(job).ok_or(Error::NotFound)
    }
    /// Stop actual work, preserving terminal results and making repeats idempotent.
    /// # Errors
    /// Returns explicit policy, lifecycle, criterion, capacity, clock or backend failures.
    pub fn cancel(&mut self, job: u32, now: u64) -> Result<(), Error> {
        self.advance(now)?;
        if self.active.is_some_and(|j| j.result.job_id == job) {
            self.finish(JobState::Cancelled)?;
            return Ok(());
        }
        self.find(job).ok_or(Error::NotFound)?;
        Ok(())
    }
    /// Settle `WAIT_MANUAL` only with independently verified matching station evidence.
    /// # Errors
    /// Returns explicit policy, lifecycle, criterion, capacity, clock or backend failures.
    pub fn external_result(
        &mut self,
        request: ExternalRequest,
        verifier: &dyn EvidenceVerifier,
        now: u64,
    ) -> Result<(), Error> {
        self.advance(now)?;
        if !self.auth.allows(crate::Capability::ExternalEvidence)
            || self.auth.station_id() != Some(request.station_id)
            || request.station_id == 0
        {
            return Err(Error::Denied);
        }
        if request.evidence == [0; 16] {
            return Err(Error::Invalid);
        }
        let result = self.find(request.job).ok_or(Error::NotFound)?;
        if result.test_id != request.test {
            return Err(Error::InvalidState);
        }
        let spec = self
            .specs
            .iter()
            .find(|s| s.id == request.test)
            .ok_or(Error::InvalidState)?;
        if spec.method == Method::Automatic
            || (!spec.criteria.is_empty() && result.count != spec.criteria.len())
        {
            return Err(Error::InvalidState);
        }
        let proof = verifier.verify(&request, result.criterion_rev)?;
        self.check()?;
        if self.auth.station_id() != Some(request.station_id)
            || !self.auth.allows(crate::Capability::ExternalEvidence)
            || proof.station_id != request.station_id
            || proof.origin == Origin::Automatic
            || spec.allowed_origins & (1 << (proof.origin as u8)) == 0
            || !spec.evidence_kinds.contains(&proof.kind)
        {
            return Err(Error::Denied);
        }
        if proof.verdict != request.result {
            return Err(Error::Conflict);
        }
        if result.state != JobState::WaitManual {
            if result.origin == Origin::Automatic {
                return Err(Error::InvalidState);
            }
            let desired = if request.result == Verdict::Pass {
                JobState::Passed
            } else {
                JobState::Failed
            };
            return if result.state == desired
                && result.station_id == request.station_id
                && result.evidence == request.evidence
                && result.origin == proof.origin
            {
                Ok(())
            } else {
                Err(Error::Conflict)
            };
        }
        if let Some(active) = self.active.as_mut() {
            if active.result.job_id != request.job {
                return Err(Error::InvalidState);
            }
            active.result.origin = proof.origin;
            active.result.station_id = proof.station_id;
            active.result.evidence = request.evidence;
        } else {
            return Err(Error::InvalidState);
        }
        self.finish(if request.result == Verdict::Pass {
            JobState::Passed
        } else {
            JobState::Failed
        })
    }
    /// Stop actual work, end this connection actor and clear cached sensitive results.
    /// # Errors
    /// Returns explicit policy, lifecycle, criterion, capacity, clock or backend failures.
    pub fn disconnect(&mut self) -> Result<(), Error> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        let result = if let Some(active) = self.active.take() {
            self.port.cancel(active.result.test_id)
        } else {
            Ok(())
        };
        self.terminal.fill(None);
        result
    }
    fn check(&mut self) -> Result<(), Error> {
        if self.closed {
            return Err(Error::Closed);
        }
        if !self.auth.allows(crate::Capability::SelfTest)
            || self.active.is_some_and(|j| {
                self.specs[j.spec].dangerous
                    && !self.auth.allows(crate::Capability::DangerousSelfTest)
            })
        {
            self.disconnect()?;
            return Err(Error::Denied);
        }
        Ok(())
    }
    fn advance(&mut self, now: u64) -> Result<(), Error> {
        self.check()?;
        if now < self.last_now {
            self.disconnect()?;
            return Err(Error::Clock);
        }
        self.last_now = now;
        if self.active.is_some_and(|j| now >= j.deadline) {
            self.finish(JobState::TimedOut)?;
        }
        Ok(())
    }
    fn set_state(&mut self, state: JobState) {
        if let Some(active) = self.active.as_mut() {
            active.result.state = state;
        }
    }
    fn finish(&mut self, state: JobState) -> Result<(), Error> {
        let job = self.active.ok_or(Error::InvalidState)?;
        if let Err(error) = self.port.cancel(job.result.test_id) {
            self.closed = true;
            self.active = None;
            self.terminal.fill(None);
            return Err(error);
        }
        self.check()?;
        let mut result = self.active.take().ok_or(Error::InvalidState)?.result;
        result.state = state;
        self.terminal[self.next_terminal] = Some(result);
        self.next_terminal = (self.next_terminal + 1) % 8;
        Ok(())
    }
    fn find(&self, job: u32) -> Option<JobResult> {
        self.active
            .map(|j| j.result)
            .filter(|r| r.job_id == job)
            .or_else(|| {
                self.terminal
                    .iter()
                    .flatten()
                    .find(|r| r.job_id == job)
                    .copied()
            })
    }
}
fn validate_spec(spec: TestSpec<'_>) -> Result<(), Error> {
    if spec.id == 0
        || spec.criterion_rev == 0
        || spec.min_timeout_ms == 0
        || spec.min_timeout_ms > spec.max_timeout_ms
        || spec.criteria.len() > 8
    {
        return Err(Error::CriterionUndefined);
    }
    if spec.method == Method::Automatic {
        if spec.criteria.is_empty() || spec.allowed_origins != 0 || !spec.evidence_kinds.is_empty()
        {
            return Err(Error::CriterionUndefined);
        }
    } else if spec.allowed_origins == 0
        || spec.allowed_origins & !6 != 0
        || spec.evidence_kinds.is_empty()
    {
        return Err(Error::CriterionUndefined);
    }
    let mut previous = 0;
    for criterion in spec.criteria {
        if criterion.id <= previous || criterion.minimum > criterion.maximum || criterion.unit > 5 {
            return Err(Error::CriterionUndefined);
        }
        previous = criterion.id;
    }
    let mut previous = 0;
    for kind in spec.evidence_kinds {
        if *kind <= previous {
            return Err(Error::CriterionUndefined);
        }
        previous = *kind;
    }
    Ok(())
}
