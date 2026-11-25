//! Fresh per-subscription actors over borrowed durable owners.
use core::cell::Cell;
use device_property_store::{CodecSchemas, PropertyStore};
use device_protocol::{Frame, ModuleEntry};
use device_runtime::{PortError, Scope, SessionFactory};
use device_services::{
    Config, HandlerTable, PropertyAccess, PropertyHandler, PropertyService, ServiceHandler,
    Session, SessionError, TimeAccess,
};
use device_storage::persistence::SnapshotStore;
use device_time_store::{DurableTimeHandler, TimeStore, TrustedClock};
/// BLE scopes supplied by actual trusted link/product policy.
#[derive(Clone, Copy)]
pub struct BleAuthority<'a> {
    /// Current ordinary-property authority.
    pub properties: &'a dyn PropertyAccess,
    /// Current durable UTC authority.
    pub time: &'a dyn TimeAccess,
}
impl core::fmt::Debug for BleAuthority<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BleAuthority").finish_non_exhaustive()
    }
}
/// Optional fresh actor construction; durable providers remain outside each actor.
pub trait Extensions {
    /// Actual fresh per-subscription handler, including all retained resource state.
    type Handler<'a>: ServiceHandler
    where
        Self: 'a;
    /// Construct against the actual subscription scope; clean up partial failures.
    /// # Errors
    /// Unavailable providers, policy and backend failures are explicit.
    fn open(&self, scope: Scope) -> Result<Self::Handler<'_>, PortError>;
}
/// No optional business bindings. This does not imply any physical backend.
#[derive(Debug)]
pub struct NoExtensions;
/// Explicit unsupported handler used only when no optional actor is bound.
#[derive(Debug)]
pub struct Unsupported;
impl ServiceHandler for Unsupported {
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
impl Extensions for NoExtensions {
    type Handler<'a> = Unsupported;
    fn open(&self, _: Scope) -> Result<Self::Handler<'_>, PortError> {
        Ok(Unsupported)
    }
}
/// Boot-owned factory; sessions never own or initialize these stores.
pub struct ProductFactory<
    'a,
    'c,
    S: SnapshotStore,
    TS: SnapshotStore,
    K: TrustedClock,
    E: Extensions,
    const N: usize,
> {
    properties: &'a PropertyStore<'c, S, CodecSchemas<'c>, N>,
    time: &'a TimeStore<TS, K>,
    authority: BleAuthority<'a>,
    template: Config<'a>,
    extensions: &'a E,
    next: Cell<u32>,
}
impl<S: SnapshotStore, TS: SnapshotStore, K: TrustedClock, E: Extensions, const N: usize>
    core::fmt::Debug for ProductFactory<'_, '_, S, TS, K, E, N>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ProductFactory")
            .field("next_session", &self.next)
            .finish_non_exhaustive()
    }
}
impl<'a, 'c, S: SnapshotStore, TS: SnapshotStore, K: TrustedClock, E: Extensions, const N: usize>
    ProductFactory<'a, 'c, S, TS, K, E, N>
{
    /// Create the actual cooperative entry borrowing this product factory.
    /// Caller supplies real transport/UI/background ports to `Runtime::step`.
    pub const fn runtime(
        &self,
        budgets: device_runtime::Budgets,
    ) -> device_runtime::Runtime<'_, Self> {
        device_runtime::Runtime::new(self, budgets)
    }
    /// Bind actual metadata, independent BLE scopes and boot-owned stores.
    /// Config.session is the first fresh session; subsequent sessions increment without wrapping.
    /// # Errors
    /// Reject unsupported/untruthful metadata, data producers or invalid selected owners.
    pub fn new(
        properties: &'a PropertyStore<'c, S, CodecSchemas<'c>, N>,
        time: &'a TimeStore<TS, K>,
        authority: BleAuthority<'a>,
        template: Config<'a>,
        extensions: &'a E,
    ) -> Result<Self, PortError> {
        if template.limits.data_max != 0 {
            return Err(PortError::InvalidInput);
        }
        Session::validate_config(&template).map_err(|_| PortError::InvalidInput)?;
        let settings = template
            .modules
            .iter()
            .find(|e| e.module == 4)
            .ok_or(PortError::InvalidInput)?;
        if settings.availability != 1
            || settings.major != 1
            || settings.minor != 0
            || settings.features != 0
            || settings.input_max < 128
            || settings.output_max < 128
        {
            return Err(PortError::InvalidInput);
        }
        let entry = *template
            .modules
            .iter()
            .find(|e| e.module == 1)
            .ok_or(PortError::InvalidInput)?;
        DurableTimeHandler::new(entry, time, authority.time)
            .map_err(|_| PortError::InvalidInput)?;
        properties.revision().map_err(|_| PortError::Unavailable)?;
        let route = PropertyRoute {
            owner: properties,
            access: authority.properties,
            maximum: usize::from(template.limits.control_max),
        };
        route.with(|_| ()).map_err(|_| PortError::InvalidInput)?;
        Ok(Self {
            properties,
            time,
            authority,
            template,
            extensions,
            next: Cell::new(template.session),
        })
    }
}
impl<S: SnapshotStore, TS: SnapshotStore, K: TrustedClock, E: Extensions, const N: usize>
    SessionFactory for ProductFactory<'_, '_, S, TS, K, E, N>
{
    type Handler<'a>
        = ProductHandler<'a, 'a, S, TS, K, E::Handler<'a>, N>
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
        let entry = *config
            .modules
            .iter()
            .find(|e| e.module == 1)
            .ok_or(PortError::InvalidInput)?;
        let time = DurableTimeHandler::new(entry, self.time, self.authority.time)
            .map_err(|_| PortError::InvalidInput)?;
        let properties = PropertyRoute {
            owner: self.properties,
            access: self.authority.properties,
            maximum: usize::from(config.limits.control_max),
        };
        let extension = self.extensions.open(scope)?;
        self.next.set(next);
        let mut handler = ProductHandler {
            properties,
            time,
            extension,
            entries: config.modules,
            valid: false,
        };
        handler.valid = handler.validate_bindings();
        // Return actor ownership even on invalid bindings: Runtime::advance fails before RX,
        // and its explicit cleanup path retains any fallible extension cleanup ownership.
        Ok((config, handler))
    }
}
struct PropertyRoute<'a, 'c, S: SnapshotStore, const N: usize> {
    owner: &'a PropertyStore<'c, S, CodecSchemas<'c>, N>,
    access: &'a dyn PropertyAccess,
    maximum: usize,
}
impl<S: SnapshotStore, const N: usize> core::fmt::Debug for PropertyRoute<'_, '_, S, N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("PropertyRoute")
            .field("module", &4_u8)
            .finish_non_exhaustive()
    }
}
impl<S: SnapshotStore, const N: usize> PropertyRoute<'_, '_, S, N> {
    fn with<T>(&self, f: impl FnOnce(&mut dyn ServiceHandler) -> T) -> Result<T, SessionError> {
        let mut adapter = self
            .owner
            .adapter(4)
            .map_err(|_| SessionError::InvalidHandlerResponse)?;
        let service = PropertyService::new(4, &mut adapter, self.maximum)
            .map_err(|_| SessionError::InvalidHandlerResponse)?;
        let mut handler = PropertyHandler::new(service, self.access, (1, 0));
        Ok(f(&mut handler))
    }
}
impl<S: SnapshotStore, const N: usize> ServiceHandler for PropertyRoute<'_, '_, S, N> {
    fn advance(&mut self, now: u64) -> Result<(), SessionError> {
        self.with(|h| h.advance(now))?
    }
    fn informational_access(&self) -> Option<device_services::AccessSnapshot> {
        self.with(|h| h.informational_access()).ok().flatten()
    }
    fn replay_valid(&self, r: Frame<'_>, response: &[u8]) -> bool {
        self.with(|h| h.replay_valid(r, response)).unwrap_or(false)
    }
    fn response_admitted(&mut self, r: Frame<'_>, response: &[u8]) -> Result<(), SessionError> {
        self.with(|h| h.response_admitted(r, response))?
    }
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        self.with(|h| h.connection_closed())?
    }
    fn response_ready(&mut self, r: Frame<'_>) -> Result<bool, SessionError> {
        self.with(|h| h.response_ready(r))?
    }
    fn must_end_session(&self) -> bool {
        self.with(|h| h.must_end_session()).unwrap_or(true)
    }
    fn required_features(&self, e: &ModuleEntry, op: u8) -> Option<u32> {
        self.with(|h| h.required_features(e, op)).ok().flatten()
    }
    fn authorized(&self, r: Frame<'_>) -> bool {
        self.with(|h| h.authorized(r)).unwrap_or(false)
    }
    fn dispatch(&mut self, r: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        self.with(|h| h.dispatch(r, out))?
    }
}
/// Fresh per-subscription product actor. Its extension retains its complete lifecycle state.
pub struct ProductHandler<
    'a,
    'c,
    S: SnapshotStore,
    TS: SnapshotStore,
    K: TrustedClock,
    H: ServiceHandler,
    const N: usize,
> {
    properties: PropertyRoute<'a, 'c, S, N>,
    time: DurableTimeHandler<'a, TS, K>,
    extension: H,
    entries: &'a [ModuleEntry],
    valid: bool,
}
impl<S: SnapshotStore, TS: SnapshotStore, K: TrustedClock, H: ServiceHandler, const N: usize>
    core::fmt::Debug for ProductHandler<'_, '_, S, TS, K, H, N>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ProductHandler")
            .field("entries", &self.entries.len())
            .finish_non_exhaustive()
    }
}
impl<S: SnapshotStore, TS: SnapshotStore, K: TrustedClock, H: ServiceHandler, const N: usize>
    ProductHandler<'_, '_, S, TS, K, H, N>
{
    fn validate_bindings(&self) -> bool {
        for entry in self
            .entries
            .iter()
            .filter(|e| e.availability == 1 && e.module != 0)
        {
            let mut supported = false;
            for opcode in 0..=u8::MAX {
                let mut owners = 0;
                for handler in [
                    &self.properties as &dyn ServiceHandler,
                    &self.time,
                    &self.extension,
                ] {
                    if let Some(bits) = handler.required_features(entry, opcode) {
                        owners += 1;
                        if bits & !entry.features != 0 {
                            return false;
                        }
                    }
                }
                if owners > 1 {
                    return false;
                }
                supported |= owners == 1;
            }
            if !supported {
                return false;
            }
        }
        true
    }
    fn with<T>(&mut self, f: impl FnOnce(&mut dyn ServiceHandler) -> T) -> Result<T, SessionError> {
        let mut handlers: [&mut dyn ServiceHandler; 3] =
            [&mut self.properties, &mut self.time, &mut self.extension];
        let mut table = HandlerTable::new(self.entries, &mut handlers)
            .map_err(|_| SessionError::InvalidHandlerResponse)?;
        Ok(f(&mut table))
    }
    fn selected(&self, e: &ModuleEntry, op: u8) -> Option<(&dyn ServiceHandler, u32)> {
        if !self.entries.contains(e) || e.availability != 1 {
            return None;
        }
        let handlers: [&dyn ServiceHandler; 3] = [&self.properties, &self.time, &self.extension];
        let mut selected = None;
        for handler in handlers {
            if let Some(bits) = handler.required_features(e, op) {
                if selected.is_some() || bits & !e.features != 0 {
                    return None;
                }
                selected = Some((handler, bits));
            }
        }
        selected
    }
    fn request_handler(&self, r: Frame<'_>) -> Option<&dyn ServiceHandler> {
        self.entries
            .iter()
            .find(|e| e.module == r.header.module)
            .and_then(|e| self.selected(e, r.header.opcode))
            .map(|(h, _)| h)
    }
}
impl<S: SnapshotStore, TS: SnapshotStore, K: TrustedClock, H: ServiceHandler, const N: usize>
    ServiceHandler for ProductHandler<'_, '_, S, TS, K, H, N>
{
    fn advance(&mut self, now: u64) -> Result<(), SessionError> {
        if !self.valid {
            return Err(SessionError::InvalidHandlerResponse);
        }
        self.with(|h| h.advance(now))?
    }
    fn informational_access(&self) -> Option<device_services::AccessSnapshot> {
        let mut access = None;
        for h in [
            &self.properties as &dyn ServiceHandler,
            &self.time,
            &self.extension,
        ] {
            if let Some(next) = h.informational_access() {
                if access.is_some_and(|old| old != next) {
                    return None;
                }
                access = Some(next);
            }
        }
        access
    }
    fn replay_valid(&self, r: Frame<'_>, response: &[u8]) -> bool {
        self.request_handler(r)
            .is_some_and(|h| h.replay_valid(r, response))
    }
    fn response_admitted(&mut self, r: Frame<'_>, response: &[u8]) -> Result<(), SessionError> {
        self.with(|h| h.response_admitted(r, response))?
    }
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        self.with(|h| h.connection_closed())?
    }
    fn response_ready(&mut self, r: Frame<'_>) -> Result<bool, SessionError> {
        self.with(|h| h.response_ready(r))?
    }
    fn must_end_session(&self) -> bool {
        !self.valid
            || self.properties.must_end_session()
            || self.time.must_end_session()
            || self.extension.must_end_session()
    }
    fn required_features(&self, e: &ModuleEntry, op: u8) -> Option<u32> {
        self.selected(e, op).map(|(_, bits)| bits)
    }
    fn authorized(&self, r: Frame<'_>) -> bool {
        !self.must_end_session() && self.request_handler(r).is_some_and(|h| h.authorized(r))
    }
    fn dispatch(&mut self, r: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        self.with(|h| h.dispatch(r, out))?
    }
}
