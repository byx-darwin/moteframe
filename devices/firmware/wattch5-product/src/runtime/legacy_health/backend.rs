//! Pure retained product calculations; real input readiness belongs to the supplied source.
use super::{LegacyInputs, calculate};
use device_health::{
    AcquisitionKind, Backend, BackendError, HealthStart, JobId, MeasurementMode, Module, Outcome,
    Owner, ProductDeclaration, Provenance, ResourceSet, ResultInfo, ResultSource, StopProgress,
    Timestamp, TriggerSource,
};
use device_health_service::{
    Error, HealthImportPort, HealthProduct, HealthSource, ModuleRunStatus,
};
/// Product-owned actual input snapshots. Implementations must reject stale/unavailable HR/clock.
/// Supplying this port does not initialize or claim a sensor; tests inject fixtures separately.
pub trait LegacyInputSource {
    /// Obtain a current valid snapshot for the selected local calculation.
    /// # Errors
    /// Missing input, lack of current actual samples or unsupported module fails explicitly.
    fn inputs(&self, module: u8) -> Result<LegacyInputs, device_health::Error>;
    /// Actual independent UTC, or unknown; peer time never sets the device clock.
    fn timestamp(&self) -> Timestamp;
}
#[derive(Clone, Copy)]
struct Job {
    id: JobId,
    bytes: [u8; 9],
    utc: i64,
    record_type: u16,
    length: u8,
    valid: bool,
    pending: bool,
}
/// One shared, three-slot value backend; CPU-only formulas consume supplied snapshots.
pub struct LegacyBackend<'a, I> {
    inputs: &'a I,
    jobs: [Option<Job>; 3],
}
impl<I> core::fmt::Debug for LegacyBackend<'_, I> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LegacyBackend").finish_non_exhaustive()
    }
}
impl<'a, I: LegacyInputSource> LegacyBackend<'a, I> {
    /// Bind the actual source, without acquiring hardware or installing synthetic defaults.
    #[must_use]
    pub const fn new(inputs: &'a I) -> Self {
        Self {
            inputs,
            jobs: [None; 3],
        }
    }
    fn job(&self, id: JobId) -> Result<&Job, BackendError> {
        self.jobs
            .iter()
            .flatten()
            .find(|j| j.id == id)
            .ok_or(BackendError::Unavailable)
    }
    fn job_mut(&mut self, id: JobId) -> Result<&mut Job, BackendError> {
        self.jobs
            .iter_mut()
            .flatten()
            .find(|j| j.id == id)
            .ok_or(BackendError::Unavailable)
    }
}
impl<I: LegacyInputSource> Backend for LegacyBackend<'_, I> {
    fn resources(&self, r: HealthStart) -> Result<ResourceSet, BackendError> {
        if r.mode() != MeasurementMode::Single {
            return Err(BackendError::Unavailable);
        }
        slot(r.module().wire())?;
        self.inputs
            .inputs(r.module().wire())
            .map_err(|_| BackendError::Unavailable)?;
        Ok(ResourceSet::from_bits(0))
    }
    fn context_is_valid(&self, _: u32) -> bool {
        false
    }
    fn start(&mut self, id: JobId, r: HealthStart) -> Result<(), BackendError> {
        self.resources(r)?;
        let index = slot(r.module().wire())?;
        if self.jobs[index].is_some_and(|j| j.pending) {
            return Err(BackendError::Unavailable);
        }
        let inputs = self
            .inputs
            .inputs(r.module().wire())
            .map_err(|_| BackendError::Unavailable)?;
        let mut bytes = [0; 9];
        let length = calculate(r.module().wire(), inputs, &mut bytes)
            .map_err(|_| BackendError::Unavailable)?;
        let timestamp = self.inputs.timestamp();
        self.jobs[index] = Some(Job {
            id,
            bytes,
            utc: timestamp.utc_ms(),
            record_type: r.record_type(),
            length: u8::try_from(length).map_err(|_| BackendError::Unavailable)?,
            valid: timestamp.is_valid(),
            pending: true,
        });
        Ok(())
    }
    fn stop(&mut self, id: JobId) -> Result<StopProgress, BackendError> {
        // Calculation already completed; stop releases no physical resource and must
        // preserve its immutable result until the transport explicitly acknowledges it.
        self.job(id)?;
        Ok(StopProgress::Stopped)
    }
    fn completion(&mut self, id: JobId) -> Result<Option<Outcome>, BackendError> {
        self.job(id)?;
        Ok(Some(Outcome::Completed))
    }
    fn has_pending_result(&self, id: JobId) -> Result<bool, BackendError> {
        Ok(self.job(id)?.pending)
    }
    fn result(&mut self, id: JobId, out: &mut [u8]) -> Result<Option<ResultInfo>, BackendError> {
        let j = self.job(id)?;
        if !j.pending {
            return Ok(None);
        }
        let length = usize::from(j.length);
        if out.len() < length {
            return Err(BackendError::Unavailable);
        }
        out[..length].copy_from_slice(&j.bytes[..length]);
        Ok(Some(ResultInfo {
            sample_id: 1,
            record_type: j.record_type,
            schema: 2,
            timestamp: Timestamp::new(j.utc, j.valid).map_err(|_| BackendError::Unavailable)?,
            length,
            source: ResultSource::DeviceAlgorithm,
        }))
    }
    fn acknowledge_result(&mut self, id: JobId, sample: u64) -> Result<(), BackendError> {
        let j = self.job_mut(id)?;
        if sample != 1 || !j.pending {
            return Err(BackendError::Unavailable);
        }
        j.pending = false;
        Ok(())
    }
}
fn slot(m: u8) -> Result<usize, BackendError> {
    match m {
        0x25 => Ok(0),
        0x26 => Ok(1),
        0x27 => Ok(2),
        _ => Err(BackendError::Unavailable),
    }
}
/// Actual local/import declaration selector sharing the same input and durable import ports.
pub struct LegacyProduct<'a, I, P> {
    inputs: &'a I,
    imports: &'a P,
}
impl<I, P> core::fmt::Debug for LegacyProduct<'_, I, P> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LegacyProduct").finish_non_exhaustive()
    }
}
impl<'a, I: LegacyInputSource, P: HealthImportPort> LegacyProduct<'a, I, P> {
    /// Bind the actual sources used by backend and import wrapper.
    #[must_use]
    pub const fn new(inputs: &'a I, imports: &'a P) -> Self {
        Self { inputs, imports }
    }
}
impl<I: LegacyInputSource, P: HealthImportPort> HealthSource for LegacyProduct<'_, I, P> {
    fn module_run_status(&self, m: u8) -> Result<ModuleRunStatus, Error> {
        slot(m).map_err(|_| Error::Invalid)?;
        Ok(ModuleRunStatus {
            state: u8::from(self.imports.available(m)),
            reason: 0,
        })
    }
}
impl<I: LegacyInputSource, P: HealthImportPort> HealthProduct for LegacyProduct<'_, I, P> {
    fn declaration(&self, m: u8, t: u16) -> Result<ProductDeclaration<'_>, Error> {
        self.declaration_for_source(m, t, ResultSource::DeviceAlgorithm)
    }
    fn declaration_for_source(
        &self,
        m: u8,
        t: u16,
        source: ResultSource,
    ) -> Result<ProductDeclaration<'_>, Error> {
        let module = Module::from_wire(m)?;
        if !module.accepts(t) || !matches!(m, 0x25..=0x27) {
            return Err(Error::Invalid);
        }
        let (acquisition, algorithm_id) = if source == ResultSource::PhoneImport {
            (AcquisitionKind::PhoneImport, "wap-health-import")
        } else {
            match m {
                0x25 => (AcquisitionKind::DerivedEstimate, "wattch5-legacy-bp"),
                0x26 => (AcquisitionKind::DerivedEstimate, "wattch5-legacy-oxygen"),
                _ => (
                    AcquisitionKind::LegacyGenerated,
                    "wattch5-legacy-clock-glucose",
                ),
            }
        };
        Ok(ProductDeclaration {
            module,
            record_type: t,
            acquisition,
            algorithm_id,
            algorithm_version: "rust-port-1",
            temperature_modes: 0,
            mood_labels: &[],
        })
    }
    fn provenance(&self, m: u8, t: u16, source: ResultSource) -> Result<Provenance<'_>, Error> {
        let d = self.declaration_for_source(m, t, source)?;
        Ok(Provenance {
            source,
            acquisition: d.acquisition,
            algorithm_id: d.algorithm_id,
            algorithm_version: d.algorithm_version,
        })
    }
    fn start_binding(&self, r: HealthStart) -> Result<(Owner, TriggerSource), Error> {
        self.inputs.inputs(r.module().wire())?;
        Ok((Owner::Application, TriggerSource::App))
    }
}
