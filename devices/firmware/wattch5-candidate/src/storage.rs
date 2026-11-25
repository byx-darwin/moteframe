//! Candidate-only, boot-owned persistent graph. Blank/corrupt inputs never format.
use super::{Clock, TestProfile, checked};
use core::task::Poll;
use device_configuration::ProductCapabilities;
use device_history_service::{HistoryAccess, HistoryOperation};
use device_history_store::{
    async_paged::{AsyncPagedHistoryStore, JobResult},
    paged::LogConfig,
};
use device_property_store::{
    AsyncPropertyStore, CodecSchemas, ModuleCapabilities, ModuleCatalogue,
};
use device_protocol::ModuleEntry;
use device_runtime::{EventTransport, PortError, Scope, Work};
use device_services::{PropertyAccess, PropertyDescriptor, PropertyOperation};
use device_spi_nor::writable::{NorChip, WritableNor};
use device_storage::{
    incremental::{AsyncMedium, IncrementalSnapshot},
    partitions::{AsyncPartitions, Coordinator, Partition, Range},
    persistence::Layout,
};
use embedded_hal::spi::SpiDevice;
use wattch5_candidate::schema::StrictSchema;
pub(super) type Part<'a, M> = Partition<'a, M>;
pub(super) type History<'a, M> =
    AsyncPagedHistoryStore<Part<'a, M>, Part<'a, M>, StrictSchema, Clock, 6, 255, 474, 464>;
#[cfg(feature = "native-health")]
pub(super) type Health<'a, M> =
    wattch5_product::runtime::legacy_health::HealthHistory<Part<'a, M>, Part<'a, M>, Clock>;
#[cfg(feature = "native-health")]
pub(super) type HealthLog<'a, 'm, M> =
    wattch5_product::runtime::legacy_health::HealthLog<'a, Part<'m, M>, Part<'m, M>, Clock>;
#[cfg(feature = "native-health")]
const PARTITIONS: usize = 6;
#[cfg(not(feature = "native-health"))]
const PARTITIONS: usize = 4;
#[cfg(feature = "native-health")]
const HISTORY_DOMAINS: usize = 9;
#[cfg(not(feature = "native-health"))]
const HISTORY_DOMAINS: usize = 6;
pub(super) struct Storage<'a, M: AsyncMedium> {
    history: History<'a, M>,
    #[cfg(feature = "native-health")]
    health: Health<'a, M>,
    properties: AsyncPropertyStore<'static, Part<'a, M>, CodecSchemas<'static>, 128>,
    boot_counter: u64,
}
const PROPERTIES: [PropertyDescriptor; 3] = [
    PropertyDescriptor {
        key: 3,
        access: 11,
        schema: 1,
        value_max: 1,
    },
    PropertyDescriptor {
        key: 7,
        access: 11,
        schema: 1,
        value_max: 7,
    },
    PropertyDescriptor {
        key: 10,
        access: 11,
        schema: 1,
        value_max: 2,
    },
];
const CATALOGUE: [ModuleCatalogue<'static>; 1] = [ModuleCatalogue {
    module: 4,
    properties: &PROPERTIES,
}];
const LANGUAGES: [&str; 2] = ["zh-CN", "en-US"];
const LEVELS: [u16; 5] = [100, 200, 400, 700, 1000];
const FACTS: [ModuleCapabilities<'static>; 1] = [ModuleCapabilities {
    module: 4,
    capabilities: ProductCapabilities {
        languages: Some(&LANGUAGES),
        normal_levels: Some(&LEVELS),
        ..ProductCapabilities::NONE
    },
}];
#[inline(never)]
pub(super) fn partitions<S: SpiDevice>(
    nor: &NorChip<S, Clock>,
) -> AsyncPartitions<WritableNor<'_, S, Clock>, PARTITIONS> {
    checked(
        AsyncPartitions::new_unrecovered(
            checked(nor.claim_writer(), 71),
            [
                Range {
                    start: 0,
                    length: 0x2000,
                    erase: true,
                },
                Range {
                    start: 0x2000,
                    length: 0x185000,
                    erase: false,
                },
                Range {
                    start: 0x189000,
                    length: 0x4000,
                    erase: true,
                },
                Range {
                    start: 0x199000,
                    length: 0x2000,
                    erase: true,
                },
                #[cfg(feature = "native-health")]
                Range {
                    start: 0x19b000,
                    length: 0x2000,
                    erase: true,
                },
                #[cfg(feature = "native-health")]
                Range {
                    start: 0x19d000,
                    length: 0x62000,
                    erase: false,
                },
            ],
        ),
        72,
    )
}
#[inline(never)]
pub(super) unsafe fn initialize_storage<'a, S: SpiDevice>(
    out: *mut Storage<'a, WritableNor<'a, S, Clock>>,
    nor: &NorChip<S, Clock>,
    mut coordinator: Coordinator<'a, WritableNor<'a, S, Clock>>,
    parts: [Part<'a, WritableNor<'a, S, Clock>>; PARTITIONS],
    mut progress: impl FnMut(),
) {
    // SAFETY: caller grants uninitialized aligned, nonaliasing owner arena until foreground termination.
    let chip_token = checked(nor.begin_recovery(), 73);
    loop {
        progress();
        match coordinator.recover_step() {
            Poll::Pending => (),
            Poll::Ready(result) => {
                checked(result, 74);
                break;
            }
        }
    }
    #[cfg(not(feature = "native-health"))]
    let [meta, log, property, boot] = parts;
    #[cfg(feature = "native-health")]
    let [meta, log, property, boot, health_meta, health_log] = parts;
    let mut boot = checked(
        IncrementalSnapshot::<_, 64>::new_unrecovered(
            boot,
            Layout {
                start: 0,
                bank_bytes: 4096,
            },
        ),
        75,
    );
    recover(&mut boot, &mut progress);
    let mut property = checked(
        IncrementalSnapshot::<_, 128>::new_unrecovered(
            property,
            Layout {
                start: 0,
                bank_bytes: 8192,
            },
        ),
        75,
    );
    recover(&mut property, &mut progress);
    // SAFETY: out is a granted uninitialized, aligned owner arena. Fields never
    // point back into it; helpers finish placement before any shared view is made.
    unsafe {
        initialize_properties(core::ptr::addr_of_mut!((*out).properties), property);
        initialize_history(core::ptr::addr_of_mut!((*out).history), meta, log);
        #[cfg(feature = "native-health")]
        initialize_health(
            core::ptr::addr_of_mut!((*out).health),
            health_meta,
            health_log,
        );
    }
    // SAFETY: both owner fields have just been initialized exactly once.
    let properties = unsafe { &*core::ptr::addr_of!((*out).properties) };
    // SAFETY: history field placement completed above and this borrow cannot outlive the arena.
    let history = unsafe { &*core::ptr::addr_of!((*out).history) };
    checked(properties.revision(), 76);
    let token = checked(history.begin_recover(), 77);
    loop {
        progress();
        checked(history.step(), 77);
        match checked(history.result(token), 77) {
            Poll::Pending => (),
            Poll::Ready(Ok(JobResult::Recovered { initialized: true })) => break,
            _ => super::fail(78),
        }
    }
    let adapter = checked(history.adapter(), 78);
    let directory = checked(device_history_service::RecordStore::directory(&adapter), 78);
    if directory.dataset != super::DATASET || directory.domains.len() != 6 {
        super::fail(78)
    }
    for (index, domain) in directory.domains.iter().enumerate() {
        if domain.domain != [0, 1, 2, 5, 6, 7][index] || domain.epoch != super::DOMAIN_EPOCHS[index]
        {
            super::fail(78)
        }
    }
    #[cfg(feature = "native-health")]
    {
        // SAFETY: health placement above completed before any shared observation.
        let health = unsafe { &*core::ptr::addr_of!((*out).health) };
        let token = checked(health.begin_recover(), 88);
        loop {
            progress();
            checked(health.step(), 88);
            match checked(health.result(token), 88) {
                Poll::Pending => (),
                Poll::Ready(Ok(JobResult::Recovered { initialized: true })) => break,
                _ => super::fail(88),
            }
        }
        let adapter = checked(health.adapter(), 88);
        let directory = checked(device_history_service::RecordStore::directory(&adapter), 88);
        if directory.dataset != super::DATASET || directory.domains.len() != 3 {
            super::fail(88)
        }
        for (index, domain) in directory.domains.iter().enumerate() {
            if domain.domain != [3, 4, 8][index]
                || domain.epoch != super::HEALTH_DOMAIN_EPOCHS[index]
            {
                super::fail(88)
            }
        }
    }
    // Every granted owner recovered; reserved unbound regions have no handles.
    checked(nor.attest_recovery_complete(chip_token), 79);
    checked(coordinator.attest_normal(), 79);
    let counter = advance_boot(&mut boot, &mut progress);
    // Drop the boot snapshot/cache only after its durable verified counter receipt.

    // SAFETY: Every field is initialized once; neither field references this arena.
    unsafe {
        core::ptr::addr_of_mut!((*out).boot_counter).write(counter);
    }
}
fn recover<M: AsyncMedium, const N: usize>(
    engine: &mut IncrementalSnapshot<M, N>,
    progress: &mut impl FnMut(),
) {
    checked(engine.begin_recover(), 75);
    loop {
        progress();
        match engine.poll_recover() {
            Poll::Pending => (),
            Poll::Ready(result) => {
                checked(result, 75);
                break;
            }
        }
    }
}
fn advance_boot<M: AsyncMedium>(
    engine: &mut IncrementalSnapshot<M, 64>,
    progress: &mut impl FnMut(),
) -> u64 {
    let mut bytes = [0; 64];
    let snapshot = checked(checked(engine.load_committed(&mut bytes), 82).ok_or(()), 82);
    let (bytes, counter) = checked(
        wattch5_candidate::boot::advance(snapshot, &bytes, super::PROVISIONING, super::NAMESPACE),
        83,
    );
    let token = checked(engine.begin_commit(snapshot.revision, &bytes), 84);
    loop {
        progress();
        match engine.poll_commit(token) {
            Poll::Pending => (),
            Poll::Ready(result) => {
                let receipt = checked(result, 84);
                if receipt.snapshot.length != 64 {
                    super::fail(84)
                }
                break;
            }
        }
    }
    counter
}
pub(super) struct Background<'a, 'm, M: AsyncMedium> {
    store: &'a Storage<'m, M>,
    #[cfg(feature = "native-health")]
    health: &'a HealthLog<'a, 'm, M>,
}
impl<'m, M: AsyncMedium> Storage<'m, M> {
    #[cfg(not(feature = "native-health"))]
    pub(super) fn work(&self) -> Background<'_, 'm, M> {
        Background { store: self }
    }
    #[cfg(feature = "native-health")]
    pub(super) fn work<'a>(&'a self, health: &'a HealthLog<'a, 'm, M>) -> Background<'a, 'm, M> {
        Background {
            store: self,
            health,
        }
    }
    #[cfg(feature = "native-health")]
    #[inline(never)]
    pub(super) fn restore_health(&self, progress: impl FnMut()) -> HealthLog<'_, 'm, M> {
        checked(
            wattch5_product::runtime::legacy_health::HealthLog::restore(
                &self.health,
                || device_history_store::HistoryClock::now_ms(&Clock),
                progress,
            ),
            89,
        )
    }
}
impl<M: AsyncMedium> Work for Background<'_, '_, M> {
    fn step(&mut self, _: u64) -> Result<(), PortError> {
        let mut scratch = [0; 256];
        #[cfg(feature = "native-health")]
        let health = self
            .health
            .step_with_scratch(&mut scratch)
            .map_err(|_| PortError::Uncertain);
        // Always advance both independently accepted jobs, even if one reports
        // uncertainty. The transfer page has no retained bytes between slices.
        let history = self
            .store
            .history
            .step_with_scratch(&mut scratch)
            .map_err(|_| PortError::Uncertain);
        let properties = self
            .store
            .properties
            .step_with_scratch(&mut scratch)
            .map_err(|_| PortError::Uncertain);
        let base = history.and(properties);
        #[cfg(feature = "native-health")]
        return base.and(health);
        #[cfg(not(feature = "native-health"))]
        base
    }
}
/// Explicit laboratory policy; never an authentication result or production role.
#[derive(Debug)]
struct LabAccess {
    dataset: [u8; 16],
}
impl PropertyAccess for LabAccess {
    fn allowed(&self, module: u8, _: PropertyOperation, key: Option<u16>) -> bool {
        module == 4 && key.is_none_or(|k| matches!(k, 3 | 7 | 10))
    }
}
impl HistoryAccess for LabAccess {
    fn module_allowed(&self) -> bool {
        true
    }
    fn allowed(&self, dataset: [u8; 16], domain: Option<u8>, _: HistoryOperation) -> bool {
        dataset == self.dataset
            && domain.is_none_or(|d| {
                matches!(d, 0 | 1 | 2 | 5 | 6 | 7)
                    || (cfg!(feature = "native-health") && matches!(d, 3 | 4 | 8))
            })
    }
}
impl device_services::TrustedAccessProvider for LabAccess {
    fn access_snapshot(&self) -> device_services::AccessSnapshot {
        device_services::AccessSnapshot {
            state: device_services::AccessState::Experimental,
            reason: 0,
        }
    }
}
#[cfg(not(feature = "native-health"))]
const SOURCE_IDS: [u8; 17] = [
    0, 1, 2, 3, 4, 5, 6, 0x10, 0x20, 0x21, 0x22, 0x23, 0x28, 0x30, 0x31, 0x32, 0x33,
];
#[cfg(feature = "native-health")]
const SOURCE_IDS: [u8; 20] = [
    0, 1, 2, 3, 4, 5, 6, 0x10, 0x20, 0x21, 0x22, 0x23, 0x25, 0x26, 0x27, 0x28, 0x30, 0x31, 0x32,
    0x33,
];
const fn modules() -> [ModuleEntry; SOURCE_IDS.len()] {
    let mut entries = [ModuleEntry {
        module: 0,
        major: 1,
        minor: 0,
        availability: 0,
        features: 0,
        input_max: 0,
        output_max: 0,
    }; SOURCE_IDS.len()];
    let mut n = 0;
    while n < SOURCE_IDS.len() {
        let id = SOURCE_IDS[n];
        entries[n].module = id;
        if id == 0 || id == 0x10 {
            entries[n].minor = 2;
        }
        if id == 0
            || id == 4
            || id == 0x10
            || (cfg!(feature = "native-health") && id >= 0x25 && id <= 0x27)
        {
            entries[n].availability = 1;
            entries[n].input_max = 128;
            entries[n].output_max = if id == 0x10 { 512 } else { 128 };
        }
        n += 1;
    }
    entries
}
const MODULES: [ModuleEntry; SOURCE_IDS.len()] = modules();
#[cfg(feature = "native-health")]
#[inline(never)]
fn join_history<
    'a,
    L: device_history_service::RecordStore,
    R: device_history_service::RecordStore,
>(
    left: &'a mut L,
    right: &'a mut R,
) -> device_history_service::TypedJoinedHistory<'a, L, R, 9> {
    checked(
        device_history_service::TypedJoinedHistory::new(left, right),
        85,
    )
}
#[inline(never)]
fn history_actor<'a, R: device_history_service::RecordStore>(
    store: R,
    access: &'a dyn HistoryAccess,
    lane: &'a device_protocol::SharedDataLane,
) -> device_application::ServiceCell<
    device_history_service::OwnedHistoryHandler<'a, R, HISTORY_DOMAINS, 474>,
> {
    device_application::ServiceCell::new(checked(
        device_history_service::OwnedHistoryHandler::new(
            MODULES[7], store, access, lane, 512, true,
        ),
        85,
    ))
}
// Constructors run before the connection loop. Keep their Result/CONTROL
// initialization copies in a released call frame rather than beside every
// subsequent READ, DATA, UI and interrupt stack.
#[inline(never)]
fn construct<T>(make: impl FnOnce() -> T) -> T {
    make()
}
#[inline(never)]
#[allow(clippy::too_many_arguments)] // Explicit connection ports and independently owned health state.
pub(super) fn connection_session<M: AsyncMedium>(
    store: &Storage<'_, M>,
    #[cfg(feature = "native-health")] health_log: &HealthLog<'_, '_, M>,
    #[cfg(feature = "native-health")] health_owner: &super::native_health::Owner,
    scope: Scope,
    session: u32,
    p: &TestProfile,
    transport: &mut impl EventTransport,
    ui: &mut impl Work,
    background: &mut impl Work,
) -> bool {
    use device_application::ResourceRouter;
    use device_services::{CoreResourceHandler, TypedHandlerTable};
    use wattch5_candidate::history::{OwnedHistoryEvents, OwnedHistoryPort};
    super::record(24, store.boot_counter as u32);
    let access = LabAccess {
        dataset: super::DATASET,
    };
    let lane = device_protocol::SharedDataLane::new(checked(
        core::num::NonZeroU64::new(u64::from(session)).ok_or(()),
        85,
    ));
    #[cfg(not(feature = "native-health"))]
    let mut history_store = checked(store.history.adapter(), 85);
    #[cfg(feature = "native-health")]
    let mut original = construct(|| checked(store.history.adapter(), 85));
    #[cfg(feature = "native-health")]
    let mut additional = construct(|| checked(store.health.adapter(), 85));
    #[cfg(feature = "native-health")]
    let mut history_store = join_history(&mut original, &mut additional);
    let actor = history_actor(&mut history_store, &access, &lane);
    let mut history = actor.handle();
    let mut credit = OwnedHistoryPort::new(&actor);
    let mut dataset = OwnedHistoryPort::new(&actor);
    let mut resources = ResourceRouter::new(&access, &mut credit, &mut dataset);
    let mut core = checked(CoreResourceHandler::new(MODULES[0], &mut resources), 86);
    let mut settings = checked(
        device_property_store::AsyncPropertyHandler::<_, _, 128, 128, 128>::new(
            &store.properties,
            4,
            &access,
            (1, 0),
            128,
            128,
        ),
        86,
    );
    #[cfg(feature = "native-health")]
    let product = wattch5_product::runtime::legacy_health::LegacyProduct::new(
        &super::native_health::INPUTS,
        health_log,
    );
    #[cfg(feature = "native-health")]
    let mut latest = health_log.latest();
    #[cfg(feature = "native-health")]
    let inner = checked(
        device_health_service::HealthHandler::<_, 3, 128>::new_bounded(
            health_owner,
            u64::from(session),
            &MODULES[12..15],
            &access,
            &Clock,
            &product,
            &mut latest,
            None,
        ),
        90,
    );
    #[cfg(feature = "native-health")]
    let mut health = checked(
        device_health_service::ImportHealthHandler::new(
            inner,
            u64::from(session),
            health_log,
            &access,
        ),
        90,
    );
    #[cfg(not(feature = "native-health"))]
    let handler = checked(
        TypedHandlerTable::new(&MODULES, (&mut core, &mut settings, &mut history)),
        86,
    );
    #[cfg(feature = "native-health")]
    let handler = checked(
        TypedHandlerTable::new(
            &MODULES,
            (&mut core, &mut settings, &mut history, &mut health),
        ),
        86,
    );
    let channel = device_connection::SharedControlChannel::new();
    let events = OwnedHistoryEvents::new(&actor, &Clock);
    let mut runtime = construct(|| {
        // Build Config inside the released constructor frame; runtime owns its copy.
        let config = device_services::Config {
            session,
            capability_rev: 1,
            profile: device_services::BuildProfile::Development,
            info: device_services::DeviceInfo {
                product_id: p.product_id,
                fw_major: 0,
                fw_minor: 1,
                fw_patch: 0,
                build_id: p.build_id,
            },
            modules: &MODULES,
            limits: device_protocol::ProtocolLimits {
                rx_frame_max: 20,
                tx_frame_max: 20,
                control_max: 128,
                data_max: 512,
                idle_ms: 2000,
                fragment_ms: 100,
                response_ms: 10000,
            },
        };
        checked(
        device_runtime::EventRuntime::<_, _, 128, 20, device_connection::RetainedBody>::new_retained(
            config,
            scope,
            &channel,
            handler,
            events,
            checked(device_runtime::Budgets::new(1, 1, 2), 86),
        ),
        86,
    )
    });
    loop {
        board_syd8810::hardware::poll_time();
        let now = embassy_time_driver::now() / 1000;
        let report = runtime.step(now, transport, ui, background);
        super::record(3, u32::from(report.ui.is_err()));
        super::record(4, u32::from(report.background.is_err()));
        if let Err(error) = report.connection {
            // Numeric diagnostic before cleanup reuses the connection frame.
            super::record(
                31,
                match error {
                    device_runtime::EventRuntimeError::Control(
                        device_runtime::RuntimeError::Connection(
                            device_connection::ConnectionError::Protocol(
                                device_protocol::ProtocolError::Timeout,
                            ),
                        ),
                    ) => 1,
                    device_runtime::EventRuntimeError::Control(
                        device_runtime::RuntimeError::StaleScope,
                    ) => 2,
                    _ => 3,
                },
            );
            let cleanup = runtime.shutdown(transport);
            return cleanup.is_err()
                || !matches!(
                    error,
                    device_runtime::EventRuntimeError::Control(
                        device_runtime::RuntimeError::StaleScope
                    )
                );
        }
    }
}

#[inline(never)]
unsafe fn initialize_history<'a, M: AsyncMedium>(
    out: *mut History<'a, M>,
    meta: Part<'a, M>,
    log: Part<'a, M>,
) {
    // SAFETY: unique caller-granted destination, correct type/alignment and capacity.
    unsafe {
        out.write(checked(
            AsyncPagedHistoryStore::new_exclusive_unrecovered(
                (
                    meta,
                    Layout {
                        start: 0,
                        bank_bytes: 4096,
                    },
                ),
                log,
                StrictSchema,
                Clock,
                LogConfig {
                    namespace: super::NAMESPACE,
                    log_start: 0x202000,
                    slots_per_domain: 259,
                },
                300,
            ),
            77,
        ));
    }
}
#[inline(never)]
unsafe fn initialize_properties<'a, M: AsyncMedium>(
    out: *mut AsyncPropertyStore<'static, Part<'a, M>, CodecSchemas<'static>, 128>,
    engine: IncrementalSnapshot<Part<'a, M>, 128>,
) {
    // SAFETY: unique caller-granted destination, correct type/alignment and capacity.
    unsafe {
        out.write(checked(
            AsyncPropertyStore::open(engine, &CATALOGUE, CodecSchemas::new(&FACTS)),
            76,
        ));
    }
}

#[derive(Debug)]
pub(super) struct LocalAccess;
impl PropertyAccess for LocalAccess {
    fn allowed(&self, module: u8, _: PropertyOperation, key: Option<u16>) -> bool {
        module == 4 && key.is_none_or(|k| matches!(k, 3 | 7 | 10))
    }
}
impl<'m, M: AsyncMedium> Storage<'m, M> {
    #[inline(never)]
    pub(super) fn local_settings<'a>(
        &'a self,
        access: &'a dyn PropertyAccess,
    ) -> Result<wattch5_candidate::ui::LocalSettings<'a, 'static, Part<'m, M>, 128>, PortError>
    {
        wattch5_candidate::ui::LocalSettings::new(
            &self.properties,
            wattch5_product::runtime::Dictionary {
                normal_levels: LEVELS,
                languages: LANGUAGES,
            },
            access,
        )
    }
}

#[cfg(feature = "native-health")]
impl device_health_service::HealthAccess for LabAccess {
    fn allowed(&self, module: u8, _: device_health_service::Operation, _: Option<u32>) -> bool {
        // Same explicit laboratory policy as the history/settings routes; never production authentication.
        matches!(module, 0x25..=0x27)
    }
}
#[cfg(feature = "native-health")]
#[inline(never)]
unsafe fn initialize_health<'a, M: AsyncMedium>(
    out: *mut Health<'a, M>,
    meta: Part<'a, M>,
    log: Part<'a, M>,
) {
    // SAFETY: exclusive aligned arena field; no field borrows this destination.
    unsafe {
        out.write(checked(
            AsyncPagedHistoryStore::new_exclusive_unrecovered(
                (
                    meta,
                    Layout {
                        start: 0,
                        bank_bytes: 4096,
                    },
                ),
                log,
                wattch5_product::runtime::legacy_health::HealthSchema,
                Clock,
                LogConfig {
                    namespace: super::NAMESPACE,
                    log_start: 0x39d000,
                    slots_per_domain: 259,
                },
                300,
            ),
            88,
        ));
    }
}
