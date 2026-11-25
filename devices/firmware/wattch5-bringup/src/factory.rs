use core::cell::Cell;
use device_protocol::{Frame, ModuleEntry, ProtocolLimits};
use device_runtime::{PortError, Scope, SessionFactory, Work};
use device_services::{BuildProfile, Config, DeviceInfo, ServiceHandler, Session, SessionError};
const MODULES: [ModuleEntry; 1] = [ModuleEntry {
    module: 0,
    major: 1,
    minor: 2,
    availability: 1,
    features: 0,
    input_max: 8,
    output_max: 23,
}];
/// Stateless anonymous handler; core operations are dispatched by the real Session.
#[derive(Debug)]
pub struct CoreOnly;
impl ServiceHandler for CoreOnly {
    fn required_features(&self, _: &ModuleEntry, _: u8) -> Option<u32> {
        None
    }
    fn authorized(&self, _: Frame<'_>) -> bool {
        false
    }
    fn dispatch(&mut self, _: Frame<'_>, _: &mut [u8]) -> Result<usize, SessionError> {
        Err(SessionError::InvalidHandlerResponse)
    }
}
/// Boot-local subscription sequence; never a durable identity or authority.
#[derive(Debug)]
pub struct CoreFactory {
    template: Config<'static>,
    next: Cell<u32>,
}
impl CoreFactory {
    /// Validate the explicitly supplied unassigned test metadata.
    /// # Errors
    /// Returns InvalidInput for invalid core protocol configuration.
    pub fn new(info: DeviceInfo) -> Result<Self, PortError> {
        let template = Config {
            session: 1,
            capability_rev: 1,
            profile: BuildProfile::Development,
            info,
            modules: &MODULES,
            limits: ProtocolLimits {
                rx_frame_max: 20,
                tx_frame_max: 20,
                control_max: 128,
                data_max: 0,
                idle_ms: 2000,
                fragment_ms: 100,
                response_ms: 10000,
            },
        };
        Session::validate_config(&template).map_err(|_| PortError::InvalidInput)?;
        Ok(Self {
            template,
            next: Cell::new(1),
        })
    }
}
impl SessionFactory for CoreFactory {
    type Handler<'a> = CoreOnly;
    fn open(&self, scope: Scope) -> Result<(Config<'_>, CoreOnly), PortError> {
        let id = self.next.get();
        let next = id.checked_add(1).ok_or(PortError::Unavailable)?;
        let mut c = self.template;
        c.session = id;
        c.limits.rx_frame_max = c.limits.rx_frame_max.min(scope.frame_max());
        c.limits.tx_frame_max = c.limits.tx_frame_max.min(scope.frame_max());
        self.next.set(next);
        Ok((c, CoreOnly))
    }
}
/// Storage is deliberately unbound; no successful background operation is claimed.
#[derive(Debug)]
pub struct UnboundStorage;
impl Work for UnboundStorage {
    fn step(&mut self, _: u64) -> Result<(), PortError> {
        Err(PortError::Unavailable)
    }
}
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    #[test]
    fn test_should_open_config_accepted_by_validation_and_actual_session() {
        let factory = CoreFactory::new(DeviceInfo {
            product_id: 0,
            fw_major: 0,
            fw_minor: 1,
            fw_patch: 0,
            build_id: 1,
        })
        .unwrap();
        for (id, frame_max) in [(1, 20), (2, 128), (3, 512)] {
            let (config, _) = factory.open(Scope::new(id, frame_max).unwrap()).unwrap();
            assert_eq!(config.session, u32::try_from(id).unwrap());
            assert_eq!(
                (config.limits.rx_frame_max, config.limits.tx_frame_max),
                (20, 20)
            );
            assert_eq!(Session::validate_config(&config), Ok(()));
            let session = Session::new(config).unwrap();
            assert_eq!(
                session.state(),
                device_services::SessionState::AwaitingNotifications
            );
        }
    }
    #[test]
    fn test_should_reject_exhaustion_without_reusing_a_session() {
        let f = CoreFactory::new(DeviceInfo {
            product_id: 0,
            fw_major: 0,
            fw_minor: 1,
            fw_patch: 0,
            build_id: 1,
        })
        .unwrap();
        f.next.set(u32::MAX - 1);
        let s = Scope::new(1, 20).unwrap();
        assert_eq!(f.open(s).unwrap().0.session, u32::MAX - 1);
        assert_eq!(f.open(s).unwrap_err(), PortError::Unavailable);
        assert_eq!(f.open(s).unwrap_err(), PortError::Unavailable);
    }
}
