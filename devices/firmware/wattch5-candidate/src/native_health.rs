//! Native source boundary: no actual HR/profile/UTC owner is bound in this candidate.
use wattch5_product::runtime::legacy_health::{LegacyBackend, LegacyInputSource, LegacyInputs};
#[derive(Debug)]
pub(super) struct Inputs;
pub(super) static INPUTS: Inputs = Inputs;
impl LegacyInputSource for Inputs {
    fn inputs(&self, _: u8) -> Result<LegacyInputs, device_health::Error> {
        // Do not manufacture HR, age, sex or local time from uptime or import timestamps.
        Err(device_health::Error::Unavailable)
    }
    fn timestamp(&self) -> device_health::Timestamp {
        device_health::Timestamp::unknown()
    }
}
pub(super) type Owner = device_health_service::SharedHealthOwner<LegacyBackend<'static, Inputs>, 3>;
#[inline(never)]
pub(super) fn owner() -> Owner {
    device_health_service::SharedHealthOwner::new(
        super::checked(device_health::JobManager::new(1), 90),
        LegacyBackend::new(&INPUTS),
    )
}
impl device_health_service::HealthClock for super::Clock {
    fn now_ms(&self) -> Result<u64, device_health_service::Error> {
        Ok(embassy_time_driver::now() / 1000)
    }
}
