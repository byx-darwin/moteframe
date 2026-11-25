//! A start-only authorization boundary around the same actual shared backend.
use crate::{Error, HealthAccess, HealthProduct, Operation};
use device_health::{
    Backend, BackendError, HealthStart, JobId, Outcome, ResourceSet, ResultInfo, StopProgress,
};

pub(crate) struct GuardedBackend<'a, B> {
    backend: &'a mut B,
    access: &'a dyn HealthAccess,
    product: &'a dyn HealthProduct,
    denied: bool,
    validation_failure: Option<Error>,
}
impl<'a, B> GuardedBackend<'a, B> {
    pub(crate) fn new(
        backend: &'a mut B,
        access: &'a dyn HealthAccess,
        product: &'a dyn HealthProduct,
    ) -> Self {
        Self {
            backend,
            access,
            product,
            denied: false,
            validation_failure: None,
        }
    }
    pub(crate) const fn denied(&self) -> bool {
        self.denied
    }
    pub(crate) const fn validation_failure(&self) -> Option<Error> {
        self.validation_failure
    }
}
impl<B: Backend> Backend for GuardedBackend<'_, B> {
    fn resources(&self, request: HealthStart) -> Result<ResourceSet, BackendError> {
        self.backend.resources(request)
    }
    fn context_is_valid(&self, context: u32) -> bool {
        self.backend.context_is_valid(context)
    }
    fn start(&mut self, job: JobId, request: HealthStart) -> Result<(), BackendError> {
        // A capability/context/product-binding callback may have replaced a device provider
        // with an import source. Validate the actual declaration again before physical work.
        if let Err(error) = crate::handler::validate_start(self.product, request) {
            self.validation_failure = Some(error);
            return Err(BackendError::Unsupported);
        }
        // Product binding, context and resource callbacks precede this physical boundary and can
        // change live authority. Check both scopes using the handler's actual authority here.
        let module = request.module().wire();
        if !self.access.allowed(module, Operation::Start, None)
            || !self
                .access
                .allowed(module, Operation::Start, Some(job.get()))
        {
            self.denied = true;
            return Err(BackendError::Unavailable);
        }
        self.backend.start(job, request)
    }
    fn stop(&mut self, job: JobId) -> Result<StopProgress, BackendError> {
        self.backend.stop(job)
    }
    fn completion(&mut self, job: JobId) -> Result<Option<Outcome>, BackendError> {
        self.backend.completion(job)
    }
    fn has_pending_result(&self, job: JobId) -> Result<bool, BackendError> {
        self.backend.has_pending_result(job)
    }
    fn result(
        &mut self,
        job: JobId,
        output: &mut [u8],
    ) -> Result<Option<ResultInfo>, BackendError> {
        self.backend.result(job, output)
    }
    fn acknowledge_result(&mut self, job: JobId, sample: u64) -> Result<(), BackendError> {
        self.backend.acknowledge_result(job, sample)
    }
}
