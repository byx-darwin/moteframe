//! Settings-only async CONTROL composition; no time or observation providers implied.
use core::cell::Cell;
use device_property_store::{AsyncPropertyHandler, AsyncPropertyStore, CodecSchemas};
use device_runtime::{Budgets, PortError, Runtime, Scope, SessionFactory};
use device_services::{Config, PropertyAccess, Session};
use device_storage::incremental::AsyncMedium;
/// Boot-owned settings-only factory with one persistent actor per subscription.
/// Only core 00 and settings 04 may be available. Physical bindings remain caller-owned.
pub struct SettingsFactory<'a, 'c, M: AsyncMedium, const N: usize> {
    owner: &'a AsyncPropertyStore<'c, M, CodecSchemas<'c>, N>,
    access: &'a dyn PropertyAccess,
    template: Config<'a>,
    next: Cell<u32>,
}
impl<M: AsyncMedium, const N: usize> core::fmt::Debug for SettingsFactory<'_, '_, M, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SettingsFactory")
            .field("next_session", &self.next)
            .finish_non_exhaustive()
    }
}
impl<'a, 'c, M: AsyncMedium, const N: usize> SettingsFactory<'a, 'c, M, N> {
    /// Bind actual ready settings, live BLE policy and truthful supplied metadata.
    /// # Errors
    /// Reject unbound modules, unsupported versions/features, limits or unready storage.
    pub fn new(
        owner: &'a AsyncPropertyStore<'c, M, CodecSchemas<'c>, N>,
        access: &'a dyn PropertyAccess,
        template: Config<'a>,
    ) -> Result<Self, PortError> {
        Session::validate_config(&template).map_err(|_| PortError::InvalidInput)?;
        if template.limits.control_max != 128 || template.limits.data_max != 0 {
            return Err(PortError::InvalidInput);
        }
        let settings = template
            .modules
            .iter()
            .find(|e| e.module == 4)
            .ok_or(PortError::InvalidInput)?;
        if settings.availability != 1
            || (settings.major, settings.minor) != (1, 0)
            || settings.features != 0
            || settings.input_max != 128
            || settings.output_max != 128
        {
            return Err(PortError::InvalidInput);
        }
        for entry in template.modules {
            if entry.module != 0
                && entry.module != 4
                && (entry.availability == 1
                    || entry.features != 0
                    || entry.input_max != 0
                    || entry.output_max != 0)
            {
                return Err(PortError::InvalidInput);
            }
        }
        owner.revision().map_err(|_| PortError::Unavailable)?;
        let _ = AsyncPropertyHandler::<_, _, N, 128>::new(owner, 4, access, (1, 0), 128, 128)
            .map_err(|_| PortError::InvalidInput)?;
        Ok(Self {
            owner,
            access,
            template,
            next: Cell::new(template.session),
        })
    }
    /// Construct the actual cooperative Runtime over this factory.
    pub const fn runtime(&self, budgets: Budgets) -> Runtime<'_, Self> {
        Runtime::new(self, budgets)
    }
}
impl<M: AsyncMedium, const N: usize> SessionFactory for SettingsFactory<'_, '_, M, N> {
    type Handler<'a>
        = AsyncPropertyHandler<'a, 'a, 'a, M, CodecSchemas<'a>, N, 128>
    where
        Self: 'a;
    fn open(&self, scope: Scope) -> Result<(Config<'_>, Self::Handler<'_>), PortError> {
        let session = self.next.get();
        let next = session.checked_add(1).ok_or(PortError::Unavailable)?;
        let mut config = self.template;
        config.session = session;
        config.limits.rx_frame_max = config.limits.rx_frame_max.min(scope.frame_max());
        config.limits.tx_frame_max = config.limits.tx_frame_max.min(scope.frame_max());
        Session::validate_config(&config).map_err(|_| PortError::InvalidInput)?;
        let handler = AsyncPropertyHandler::new(self.owner, 4, self.access, (1, 0), 128, 128)
            .map_err(|_| PortError::InvalidInput)?;
        self.next.set(next);
        Ok((config, handler))
    }
}
