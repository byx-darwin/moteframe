//! Existing page requests and observations over the same durable source as BLE.
use super::{HealthLog, LegacyInputSource};
use crate::runtime::{Actions, Observation, ObservationSource};
use device_history_store::HistoryClock;
use device_runtime::PortError;
use device_storage::incremental::AsyncMedium;
use wattch5_parity_ui_demo::{MeasureKind, UiRequest};
/// Actual local start/stop adapter. Other product actions remain explicitly unsupported.
pub struct LegacyActions<'a, 's, I, M: AsyncMedium, L: AsyncMedium, C: HistoryClock> {
    inputs: &'a I,
    log: &'a HealthLog<'s, M, L, C>,
}
impl<I, M: AsyncMedium, L: AsyncMedium, C: HistoryClock> core::fmt::Debug
    for LegacyActions<'_, '_, I, M, L, C>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LegacyActions").finish_non_exhaustive()
    }
}
impl<'a, 's, I: LegacyInputSource, M: AsyncMedium, L: AsyncMedium, C: HistoryClock>
    LegacyActions<'a, 's, I, M, L, C>
{
    /// Bind actual inputs and the same boot-owned log used by App imports.
    #[must_use]
    pub const fn new(inputs: &'a I, log: &'a HealthLog<'s, M, L, C>) -> Self {
        Self { inputs, log }
    }
}
impl<I: LegacyInputSource, M: AsyncMedium, L: AsyncMedium, C: HistoryClock> Actions
    for LegacyActions<'_, '_, I, M, L, C>
{
    fn perform(&mut self, request: UiRequest) -> Result<(), PortError> {
        match request {
            UiRequest::MeasureStart(kind) => {
                let module = module(kind).map_err(|_| PortError::Unavailable)?;
                let inputs = self
                    .inputs
                    .inputs(module)
                    .map_err(|_| PortError::Unavailable)?;
                self.log
                    .start_ui(module, inputs, self.inputs.timestamp())
                    .map_err(|_| PortError::Fault)
            }
            UiRequest::MeasureStop(kind) => self
                .log
                .stop_ui(module(kind).map_err(|_| PortError::Unavailable)?)
                .map_err(|_| PortError::Fault),
            _ => Err(PortError::Unavailable),
        }
    }
}
/// Adds the three actual committed/pending pages to a caller's existing complete observation source.
pub struct LegacyObservations<'a, 's, O, M: AsyncMedium, L: AsyncMedium, C: HistoryClock> {
    base: O,
    log: &'a HealthLog<'s, M, L, C>,
    last: Option<(u64, u64, bool)>,
    generation: u64,
}
impl<O, M: AsyncMedium, L: AsyncMedium, C: HistoryClock> core::fmt::Debug
    for LegacyObservations<'_, '_, O, M, L, C>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("LegacyObservations").finish_non_exhaustive()
    }
}
impl<'a, 's, O: ObservationSource, M: AsyncMedium, L: AsyncMedium, C: HistoryClock>
    LegacyObservations<'a, 's, O, M, L, C>
{
    /// Wrap supplied real base observations; no defaults for time/battery/other sensors are provided.
    #[must_use]
    pub const fn new(base: O, log: &'a HealthLog<'s, M, L, C>) -> Self {
        Self {
            base,
            log,
            last: None,
            generation: 0,
        }
    }
}
impl<O: ObservationSource, M: AsyncMedium, L: AsyncMedium, C: HistoryClock> ObservationSource
    for LegacyObservations<'_, '_, O, M, L, C>
{
    fn capture(&mut self) -> Result<Observation<'_>, PortError> {
        let healthy = self.log.ready().is_ok();
        let mut observation = self.base.capture()?;
        if observation.generation == 0
            || self
                .last
                .is_some_and(|(prior, _, _)| observation.generation < prior)
        {
            return Err(PortError::InvalidInput);
        }
        let stamp = (observation.generation, self.log.view_generation(), healthy);
        if self.last != Some(stamp) {
            self.generation = self.generation.checked_add(1).ok_or(PortError::Fault)?;
            self.last = Some(stamp);
        }
        for kind in [
            MeasureKind::Pressure,
            MeasureKind::Oxygen,
            MeasureKind::Glucose,
        ] {
            observation.snapshot.measurements[kind as usize] =
                self.log.page(kind).map_err(|_| PortError::Unavailable)?;
        }
        observation.generation = self.generation;
        Ok(observation)
    }
}
pub(super) const fn module(kind: MeasureKind) -> Result<u8, device_health_service::Error> {
    match kind {
        MeasureKind::Pressure => Ok(0x25),
        MeasureKind::Oxygen => Ok(0x26),
        MeasureKind::Glucose => Ok(0x27),
        _ => Err(device_health_service::Error::Invalid),
    }
}
