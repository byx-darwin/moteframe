//! Mandatory actual-port observers retain failures hidden by a frozen engine's idempotent close.
use crate::Error;
use core::cell::Cell;

/// Single-producer physical-cleanup evidence, held outside the frozen engine.
/// Use one fresh trace for one observed port lifetime. There is no reset/clear operation:
/// constructing a later view never proves that a previous failed cleanup succeeded.
pub struct CleanupTrace {
    bound: Cell<bool>,
    failure: Cell<Option<device_maintenance::Error>>,
}
impl core::fmt::Debug for CleanupTrace {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("CleanupTrace").finish_non_exhaustive()
    }
}
impl Default for CleanupTrace {
    fn default() -> Self {
        Self::new()
    }
}
impl CleanupTrace {
    /// Create an unbound evidence cell; this performs no physical operation.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            bound: Cell::new(false),
            failure: Cell::new(None),
        }
    }
    fn bind(&self) -> Result<(), Error> {
        if self.bound.replace(true) {
            return Err(Error::Busy);
        }
        Ok(())
    }
    fn record(&self, result: Result<(), device_maintenance::Error>) {
        if let Err(error) = result
            && self.failure.get().is_none()
        {
            self.failure.set(Some(error));
        }
    }
    pub(crate) fn failure(&self) -> Option<Error> {
        self.failure.get().map(Error::from)
    }
}

#[cfg(feature = "raw-data")]
/// Exclusive real capture-port view whose every actual stop result reaches the same trace.
/// Source/channel/sample data are forwarded unchanged; no backend is simulated.
pub struct RawPortObserver<'a> {
    actual: &'a mut dyn device_maintenance::raw::RawPort,
    pub(crate) trace: &'a CleanupTrace,
}
#[cfg(feature = "raw-data")]
impl core::fmt::Debug for RawPortObserver<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("RawPortObserver").finish_non_exhaustive()
    }
}
#[cfg(feature = "raw-data")]
impl<'a> RawPortObserver<'a> {
    /// Bind actual capture work to a single-producer cleanup trace without calling hardware.
    /// # Errors
    /// Returns Busy if another port has already bound this trace; traces cannot be recycled.
    pub fn new(
        actual: &'a mut dyn device_maintenance::raw::RawPort,
        trace: &'a CleanupTrace,
    ) -> Result<Self, Error> {
        trace.bind()?;
        Ok(Self { actual, trace })
    }
}
#[cfg(feature = "raw-data")]
impl device_maintenance::raw::RawPort for RawPortObserver<'_> {
    fn source_count(&self) -> usize {
        self.actual.source_count()
    }
    fn source(&self, index: usize) -> Option<device_maintenance::raw::Source> {
        self.actual.source(index)
    }
    fn channel(
        &self,
        source: u16,
        index: u8,
    ) -> Result<device_maintenance::raw::Channel, device_maintenance::Error> {
        self.actual.channel(source, index)
    }
    fn configure(
        &mut self,
        config: device_maintenance::raw::Config,
    ) -> Result<device_maintenance::raw::Accepted, device_maintenance::Error> {
        self.actual.configure(config)
    }
    fn start(&mut self, source: u16) -> Result<u64, device_maintenance::Error> {
        self.actual.start(source)
    }
    fn poll(
        &mut self,
        out: &mut [u8],
        max_samples: u16,
        remaining: u32,
    ) -> Result<Option<device_maintenance::raw::Polled>, device_maintenance::Error> {
        self.actual.poll(out, max_samples, remaining)
    }
    fn stop(&mut self) -> Result<(), device_maintenance::Error> {
        let result = self.actual.stop();
        self.trace.record(result);
        result
    }
}

#[cfg(feature = "self-test")]
/// Exclusive real controlled-test port view retaining every actual failed cancel independently.
pub struct SelfTestPortObserver<'a> {
    actual: &'a mut dyn device_maintenance::self_test::TestPort,
    pub(crate) trace: &'a CleanupTrace,
}
#[cfg(feature = "self-test")]
impl core::fmt::Debug for SelfTestPortObserver<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SelfTestPortObserver")
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "self-test")]
impl<'a> SelfTestPortObserver<'a> {
    /// Bind the actual test backend to independent, nonrecyclable cleanup evidence.
    /// # Errors
    /// Returns Busy for a trace already bound to another real port.
    pub fn new(
        actual: &'a mut dyn device_maintenance::self_test::TestPort,
        trace: &'a CleanupTrace,
    ) -> Result<Self, Error> {
        trace.bind()?;
        Ok(Self { actual, trace })
    }
}
#[cfg(feature = "self-test")]
impl device_maintenance::self_test::TestPort for SelfTestPortObserver<'_> {
    fn supports(&self, test: u16) -> bool {
        self.actual.supports(test)
    }
    fn start(
        &mut self,
        test: u16,
        criterion_rev: u16,
    ) -> Result<device_maintenance::self_test::Phase, device_maintenance::Error> {
        self.actual.start(test, criterion_rev)
    }
    fn poll(
        &mut self,
        test: u16,
        out: &mut [device_maintenance::self_test::Metric; 8],
    ) -> Result<device_maintenance::self_test::Observation, device_maintenance::Error> {
        self.actual.poll(test, out)
    }
    fn cancel(&mut self, test: u16) -> Result<(), device_maintenance::Error> {
        let result = self.actual.cancel(test);
        self.trace.record(result);
        result
    }
}
