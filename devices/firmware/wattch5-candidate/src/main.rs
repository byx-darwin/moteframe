//! Actual native offline candidate; experimental hardware qualification remains closed.
#![no_std]
#![no_main]
use ble_controller::{Controller, HciController, RandomAddress, Version};
use ble_controller_syd881x::SydBackend;
use ble_host_trouble::{Resources, build, host::prelude::*};
use board_syd8810::{
    drivers::{Board, PinId, SpiConfig, SpiSpeed},
    hardware::{Mmio, poll_time, reset_display, start_irq_time},
    panel,
};
use core::{
    arch::global_asm,
    cell::Cell,
    future::{Future, poll_fn},
    num::NonZeroU32,
    pin::pin,
    ptr::{read_volatile, write_volatile},
    task::{Context, Poll, Waker},
};
use device_core::DelayMs;
use device_runtime::{PortError, Transport, Work};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use static_cell::StaticCell;
use syd881x_radio::{ConfigurationTable, Radio};
use wattch5_bringup::{LinkStage, ObservedReset, Readiness, ResetGate};
use wattch5_candidate::gatt::{
    Bridge, ControlProfile, ControlUuids, TroubleTransport, drive_connection,
};
mod host_diagnostic;
#[cfg(feature = "native-health")]
mod native_health;
mod physical;
#[cfg(feature = "lab-stack-observer")]
mod stack_observer;
global_asm!(
    ".set WATTCH5_NORMALIZED_RESET, 1",
    include_str!("../../../platforms/syd881x/runtime/startup/wattch5-bringup.s")
);
#[derive(Debug)]
struct TestProfile {
    address: [u8; 6],
    service: u16,
    rx: u16,
    tx: u16,
    name: &'static [u8],
    adv: &'static [u8],
    product_id: u32,
    build_id: u32,
}
include!(concat!(env!("OUT_DIR"), "/bindings.rs"));
#[unsafe(link_section = ".upper_static")]
static BRIDGE: StaticCell<Bridge<1, 1>> = StaticCell::new();
#[unsafe(link_section = ".upper_static")]
static HOST: StaticCell<Resources> = StaticCell::new();

#[unsafe(link_section = ".probe_status")]
static mut STATUS: [u32; 32] = [0; 32];
#[used]
#[unsafe(link_section = ".host_error")]
static mut HOST_ERROR: [u8; 128] = [0; 128];
// ARM type sizes retained in the actual linked image, separately from dynamic future sizes.
#[used]
#[unsafe(link_section = ".candidate_manifest")]
static TYPE_SIZES: [u32; 6] = [
    core::mem::size_of::<Resources>() as u32,
    core::mem::size_of::<Bridge<1, 1>>() as u32,
    core::mem::size_of::<physical::Inputs<'static>>() as u32,
    core::mem::size_of::<device_connection::BoundedControlConnection<'static, 128>>() as u32,
    core::mem::size_of::<device_spi_nor::writable::NorChip<NativeDevice, Clock>>() as u32,
    core::mem::size_of::<
        storage::Storage<
            'static,
            device_spi_nor::writable::WritableNor<'static, NativeDevice, Clock>,
        >,
    >() as u32,
];
type NativeDevice = moteframe_core::spi::Device<
    'static,
    board_syd8810::drivers::Spi<'static, Mmio>,
    Delay,
    device_moteframe::Pin<board_syd8810::drivers::Output<'static, Mmio>>,
>;
// Scoped placement arena: actual owner type size, without a self reference.
#[repr(C, align(8))]
struct OwnerArena(
    [core::mem::MaybeUninit<u8>;
        core::mem::size_of::<
            storage::Storage<
                'static,
                device_spi_nor::writable::WritableNor<'static, NativeDevice, Clock>,
            >,
        >()],
);
static mut OWNER_ARENA: OwnerArena = OwnerArena(
    [core::mem::MaybeUninit::uninit();
        core::mem::size_of::<
            storage::Storage<
                'static,
                device_spi_nor::writable::WritableNor<'static, NativeDevice, Clock>,
            >,
        >()],
);
// Reclaimed directory/cache bytes hold the chip owner within the same RAM budget.
#[repr(C, align(8))]
struct NorArena(
    [core::mem::MaybeUninit<u8>;
        core::mem::size_of::<device_spi_nor::writable::NorChip<NativeDevice, Clock>>()],
);
static mut NOR_ARENA: NorArena = NorArena(
    [core::mem::MaybeUninit::uninit();
        core::mem::size_of::<device_spi_nor::writable::NorChip<NativeDevice, Clock>>()],
);
#[inline(never)]
unsafe fn initialize_nor<S: embedded_hal::spi::SpiDevice>(
    out: *mut device_spi_nor::writable::NorChip<S, Clock>,
    device: S,
) {
    // SAFETY: Caller supplies a correctly sized, aligned, uniquely owned arena.
    unsafe {
        out.write(checked(
            device_spi_nor::writable::NorChip::new_experimental(
                device,
                Clock,
                NOR_POLICY,
                device_spi_nor::writable::Partition {
                    start: 0x200000,
                    length: 0x200000,
                },
            ),
            70,
        ));
    }
}
unsafe extern "C" {
    static __sidata: u8;
    static mut __sdata: u8;
    static mut __edata: u8;
    static mut __sbss: u8;
    static mut __ebss: u8;
    static mut __supper: u8;
    static mut __eupper: u8;
}
fn record(index: usize, value: u32) {
    if index < 32 {
        // SAFETY: Serialized foreground writes a bounded word in the linker-reserved diagnostic region.
        unsafe {
            write_volatile((&raw mut STATUS).cast::<u32>().add(index), value);
        }
    }
}
// The existing diagnostic bank owns these words. The optional stack observer
// reuses26..31, so refresh profiling is excluded from that feature selection.
#[cfg(not(feature = "lab-stack-observer"))]
fn observed(index: usize) -> u32 {
    if index < 32 {
        // SAFETY: Serialized foreground reads a bounded linker-reserved diagnostic word.
        unsafe { read_volatile((&raw const STATUS).cast::<u32>().add(index)) }
    } else {
        0
    }
}
fn checked<T, E>(value: Result<T, E>, code: u32) -> T {
    match value {
        Ok(v) => v,
        Err(_) => fail(code),
    }
}
fn stop_radio() {
    // SAFETY: Exclusive firmware fault boundary disables the native radio advertisement bit without ROM calls.
    unsafe {
        let p = 0x4002_0010 as *mut u8;
        write_volatile(p, read_volatile(p) & !1);
    }
}
fn fail(code: u32) -> ! {
    record(2, code);
    stop_radio();
    loop {
        core::hint::spin_loop();
    }
}
#[unsafe(no_mangle)]
extern "C" fn Fault_Handler() -> ! {
    fail(0xf001)
}
#[unsafe(no_mangle)]
extern "C" fn Default_Handler() -> ! {
    fail(0xf002)
}
#[unsafe(no_mangle)]
unsafe extern "C" fn reset_rust() -> ! {
    // SAFETY: Candidate assembly checked privileged Thread mode, selected MSP,
    // and masked configurable IRQs before this first Rust operation.
    unsafe { syd881x_runtime::prepare_candidate_entry() };
    // SAFETY: Reset owns only the linker-bounded application data/BSS, never lower ROM/shared RAM, diagnostics or stack.
    unsafe {
        syd881x_runtime::initialize_ram(
            &raw const __sidata,
            &raw mut __sdata,
            &raw mut __edata,
            &raw mut __sbss,
            &raw mut __ebss,
        );
    }
    // SAFETY: Candidate-only upper BSS is disjoint from diagnostics/stack and
    // initialized before any StaticCell or IRQ observes it.
    unsafe {
        let mut p = &raw mut __supper;
        while p < (&raw mut __eupper) {
            write_volatile(p, 0);
            p = p.add(1);
        }
    }
    // SAFETY: Reset exclusively owns the256-byte NOLOAD diagnostics and masks unsupported NVIC/SysTick/watchdog before IRQ clock acquisition.
    unsafe {
        for i in 0..32 {
            write_volatile((&raw mut STATUS).cast::<u32>().add(i), 0);
        }
        for i in 0..128 {
            write_volatile((&raw mut HOST_ERROR).cast::<u8>().add(i), 0);
        }
    }
    record(0, 0x5735_4255);
    #[cfg(feature = "lab-stack-observer")]
    record(25, 0x5354_414b);
    record(1, 1);
    candidate_foreground_start()
}
fn settle(us: u32) {
    for _ in 0..us.saturating_mul(64) {
        // SAFETY: Conservative bounded pre-clock delay; NOP changes no memory or processor control state.
        unsafe {
            core::arch::asm!("nop", options(nomem, nostack, preserves_flags));
        }
    }
}
#[derive(Debug)]
struct Delay;
impl DelayMs for Delay {
    fn delay_ms(&mut self, ms: u16) {
        for _ in 0..ms {
            settle(1000);
            poll_time();
        }
    }
}
fn pin_id(n: u8) -> PinId {
    checked(PinId::new(n), 30)
}
fn link(status: &Cell<Readiness>, stage: LinkStage) {
    let mut s = status.get();
    s.set_link(stage);
    status.set(s);
    record(1, stage as u32 + 10);
}

// Observation belongs to the firmware boundary, keeping the reusable UI free
// of SYD registers while recording real navigation during a BLE connection too.
struct ObservedUi<'u, 's, D, I, L>(&'u mut wattch5_candidate::v2_ui::V2Ui<'s, D, I, L>);
impl<
    D: device_core::Display,
    I: wattch5_bringup::InputSource,
    L: FnMut(u8) -> Result<(), PortError>,
> Work for ObservedUi<'_, '_, D, I, L>
{
    fn step(&mut self, now: u64) -> Result<(), PortError> {
        #[cfg(not(feature = "lab-stack-observer"))]
        let started = embassy_time_driver::now();
        #[cfg(not(feature = "lab-stack-observer"))]
        let frames = self.0.frames();
        let result = self.0.step(now);
        #[cfg(not(feature = "lab-stack-observer"))]
        record_ui_refresh(
            started,
            frames != self.0.frames(),
            self.0.next_paint_pixel(),
        );
        record(6, self.0.model().screen() as u32);
        record(7, self.0.frames());
        record(8, u32::from(self.0.model().draft()));
        record(9, self.0.model().appearance().locale as u32);
        result
    }
}
// Timing is a sibling call after rendering; profiling locals cannot extend
// the deepest rendering stack. Units follow the nominal microsecond clock.
#[cfg(not(feature = "lab-stack-observer"))]
#[inline(never)]
fn record_ui_refresh(started: u64, completed: bool, next_pixel: Option<u16>) {
    let finished = embassy_time_driver::now();
    let elapsed = finished.saturating_sub(started).min(u64::from(u32::MAX)) as u32;
    record(22, elapsed);
    record(23, observed(23).max(elapsed));
    let mut frame_start = u64::from(observed(27)) | (u64::from(observed(28)) << 32);
    if (frame_start == 0 && next_pixel.is_some()) || next_pixel == Some(32) {
        frame_start = started;
    }
    if completed && frame_start != 0 {
        record(
            26,
            finished
                .saturating_sub(frame_start)
                .min(u64::from(u32::MAX)) as u32,
        );
        frame_start = 0;
    }
    record(27, frame_start as u32);
    record(28, (frame_start >> 32) as u32);
    record(29, finished as u32);
    record(30, (finished >> 32) as u32);
}
#[inline(never)]
fn control_bridge() -> &'static Bridge<1, 1> {
    BRIDGE.init_with(|| checked(Bridge::new(), 25))
}
#[inline(never)]
fn control_server<'v>(
    p: &TestProfile,
    rx: &'v mut [u8; 20],
    tx: &'v mut [u8; 20],
    data: &'v mut [u8; 20],
) -> (
    AttributeServer<'v, NoopRawMutex, DefaultPacketPool, 13, 1>,
    ControlProfile,
) {
    let mut attributes: AttributeTable<'_, NoopRawMutex, 13> = AttributeTable::new();
    attributes
        .add_service(Service::new(0x1800_u16))
        .add_characteristic_ro(0x2a00_u16, p.name);
    attributes.add_service(Service::new(0x1801_u16));
    let profile = checked(
        ControlProfile::add(
            &mut attributes,
            ControlUuids {
                service: p.service.into(),
                rx: p.rx.into(),
                tx: p.tx.into(),
                data: DATA_UUID.into(),
            },
            rx,
            tx,
            data,
        ),
        26,
    );
    (AttributeServer::new(attributes), profile)
}
#[derive(Debug)]
struct HostMailbox;
impl host_diagnostic::ExitMailbox for HostMailbox {
    fn invalidate(&mut self) {
        // SAFETY: Serialized Host completion exclusively owns this reserved, aligned word.
        unsafe { write_volatile((&raw mut HOST_ERROR).cast::<u32>(), 0) };
    }
    fn write_payload(&mut self, offset: u8, value: u8) {
        // SAFETY: Publisher supplies only4..128 in the exclusively owned128-byte bank.
        unsafe {
            write_volatile(
                (&raw mut HOST_ERROR).cast::<u8>().add(usize::from(offset)),
                value,
            )
        };
    }
    fn commit(&mut self, magic: u32) {
        // SAFETY: Full payload and release fence precede this aligned atomic-width commit.
        unsafe { write_volatile((&raw mut HOST_ERROR).cast::<u32>(), magic) };
    }
}
impl host_diagnostic::BackendDiagnostic
    for wattch5_bringup::ResetError<ble_controller_syd881x::SydError<core::convert::Infallible>>
{
    fn syd_error(
        &self,
    ) -> Result<&ble_controller_syd881x::SydError<core::convert::Infallible>, u8> {
        match self {
            Self::Backend(error) => Ok(error),
            Self::Retired => Err(1),
        }
    }
}
#[inline(never)]
fn record_host_exit(
    result: &Result<
        (),
        host_diagnostic::HostExitError<
            wattch5_bringup::ResetError<
                ble_controller_syd881x::SydError<core::convert::Infallible>,
            >,
        >,
    >,
) {
    host_diagnostic::ExitDiagnostic::publish(result, &mut HostMailbox);
}
#[inline(never)]
#[unsafe(no_mangle)]
fn candidate_foreground_start() -> ! {
    let p = checked(TEST_PROFILE.as_ref().ok_or(()), 1);
    let bytes = checked(BOARD_TABLE.ok_or(()), 2);
    let table = checked(ConfigurationTable::new(bytes), 3);
    if table.header()[1] & 15 != 0 {
        fail(4)
    }
    // SAFETY: Sole MMIO claim after initialized application RAM; selected SYD physical map.
    let mut mmio = checked(unsafe { Mmio::claim() }, 5);
    let gate = ResetGate::new();
    let stack = native_stack(&mut mmio, &gate, table, p.address);
    record(11, core::mem::size_of_val(&stack) as u32);
    let (mut rx_runner, mut control_runner, mut tx_runner) = stack.runner().split();
    let mut peripheral = stack.peripheral();
    let host_exited = Cell::new(false);
    let mut rx_future = pin!(Some(rx_runner.run()));
    let mut control_future = pin!(Some(control_runner.run()));
    let mut tx_future = pin!(Some(tx_runner.run()));
    let runner_future_bytes = core::mem::size_of_val(rx_future.as_ref().get_ref())
        + core::mem::size_of_val(control_future.as_ref().get_ref())
        + core::mem::size_of_val(tx_future.as_ref().get_ref());
    let runners = checked(
        ble_host_trouble::HostRunners::new(
            tx_future.as_mut(),
            rx_future.as_mut(),
            control_future.as_mut(),
        ),
        13,
    );
    let mut host = pin!(async {
        let result = runners.await;
        record_host_exit(&result);
        host_exited.set(true);
        record(19, if result.is_err() { 2 } else { 1 });
        result
    });
    record(
        12,
        (runner_future_bytes + core::mem::size_of_val(host.as_ref().get_ref())) as u32,
    );
    let startup = embassy_time_driver::now();
    let mut cx = Context::from_waker(Waker::noop());
    while !gate.reset_completed() {
        poll_time();
        if host.as_mut().poll(&mut cx).is_ready() {
            fail(13)
        }
        if embassy_time_driver::now().saturating_sub(startup) > 5_000_000 {
            fail(14)
        }
    }
    checked(gate.arm_ui(), 15);
    record(1, 2);
    // No advertising future exists until all physical UI initialization has completed.
    let mut board = Board::new(mmio);
    checked(device_core::Board::initialize(&mut board), 16);
    let select = checked(
        board.output(pin_id(board_syd8810::DISPLAY_PINS.chip_select), true),
        17,
    );
    let dc = checked(
        board.output(pin_id(board_syd8810::DISPLAY_PINS.data_command), true),
        17,
    );
    let mut reset = checked(
        board.output(pin_id(board_syd8810::DISPLAY_PINS.reset), true),
        17,
    );
    let mut light = checked(board.backlight(100), 17);
    checked(light.set_duty(0), 17);
    let config = SpiConfig::new(SpiSpeed::Mhz8, panel::SPI_POLARITY, panel::SPI_PHASE, 4096);
    let nor_select = checked(board.output(pin_id(15), true), 17);
    let shared =
        moteframe_core::spi::SharedBus::new(checked(board.spi_full_duplex(config), 18), Delay);
    let panel_device = checked(shared.device(device_moteframe::Pin(select)), 18);
    let nor_device = checked(shared.device(device_moteframe::Pin(nor_select)), 18);
    // SAFETY: Single reset-time initialization. The shared bus remains alive
    // for this never-returning foreground; this reference does not escape it.
    let nor = unsafe {
        let out = (&raw mut NOR_ARENA).cast::<device_spi_nor::writable::NorChip<_, Clock>>();
        initialize_nor(out, nor_device);
        &*out
    };
    // Initialize the final bridge owner in place; avoid an initialized display copy.
    let mut display = device_moteframe::NativeDisplay::new(moteframe_display::DcsDisplay::new(
        moteframe_display::SharedPanelBus::new(panel_device, device_moteframe::Pin(dc)),
        moteframe_core::Dimensions::new(panel::DIMENSIONS.width, panel::DIMENSIONS.height),
        panel::OFFSET,
    ));
    let mut delay = Delay;
    checked(reset_display(&mut reset, &mut delay), 19);
    const STEPS: [moteframe_display::InitStep<'static>; panel::INITIALIZATION.len()] = {
        let mut source = [device_display::InitStep::DelayMs(0); panel::INITIALIZATION.len()];
        let mut i = 0;
        while i < source.len() {
            source[i] = panel::INITIALIZATION[i];
            i += 1;
        }
        device_moteframe::initialization(source)
    };
    checked(
        display
            .inner_mut()
            .initialize(&STEPS, &mut device_moteframe::Delay(Delay)),
        20,
    );
    let inputs = checked(physical::Inputs::new(&board, &mut delay), 21);
    let status = Cell::new(Readiness::new());
    let mut ui = checked(
        wattch5_candidate::v2_ui::V2Ui::new(
            display,
            inputs,
            |duty| light.set_duty(duty).map_err(|_| PortError::Fault),
            &status,
            &gate,
        ),
        22,
    );
    record(13, core::mem::size_of_val(&ui) as u32);
    record(5, 0x5632_5549); // Full V2 product presentation, distinct from the fallback.

    // Every mutable owner is recovered before the durable boot increment and advertising.
    let mut partitions = storage::partitions(nor);
    let (coordinator, parts) = checked(partitions.split(), 72);
    // SAFETY: Sole reset-time placement. This foreground never returns; nor,
    // partitions and their shared bus remain alive before this lexical owner.
    let storage = unsafe {
        let out = (&raw mut OWNER_ARENA).cast::<storage::Storage<'_, _>>();
        initialize_storage(out, nor, coordinator, parts, || {
            poll_time();
            if host.as_mut().poll(&mut cx).is_ready() {
                fail(13)
            }
            let _ = ui.step(embassy_time_driver::now() / 1000);
        });
        &*out
    };
    let local_access = storage::LocalAccess;
    let local_settings = checked(storage.local_settings(&local_access), 87);
    checked(ui.bind(&local_settings), 87);
    #[cfg(feature = "native-health")]
    let health_log = storage.restore_health(|| {
        poll_time();
        if host.as_mut().poll(&mut cx).is_ready() {
            fail(13)
        }
        let _ = ui.step(embassy_time_driver::now() / 1000);
    });
    #[cfg(feature = "native-health")]
    let health_owner = native_health::owner();
    #[cfg(feature = "native-health")]
    let local_health = wattch5_candidate::ui::LocalHealth::new(&native_health::INPUTS, &health_log);
    #[cfg(feature = "native-health")]
    checked(ui.bind_health(&local_health), 90);
    #[cfg(not(feature = "factory-ui"))]
    let backgrounds = checked(
        wattch5_parity_ui_demo::ImageAssets::open(AssetReader(nor)),
        91,
    );
    #[cfg(feature = "factory-ui")]
    let backgrounds = checked(
        wattch5_parity_ui_demo::FactoryAssets::open(AssetReader(nor)),
        91,
    );
    ui.bind_background(&backgrounds);
    let bridge = control_bridge();
    let mut rx = [0; 20];
    let mut tx = [0; 20];
    let mut data = [0; 20];
    // Borrow the returned owner in place: moving its fields keeps an extra
    // attribute-server temporary in this never-returning foreground frame.
    let control = control_server(p, &mut rx, &mut tx, &mut data);
    let (server, profile) = (&control.0, &control.1);
    let cancel_connection = Cell::new(false);
    let mut gatt = pin!(async {
        loop {
            link(&status, LinkStage::AdvertisingCommand);
            let advertiser = peripheral
                .advertise(
                    &AdvertisementParameters::default(),
                    Advertisement::ConnectableScannableUndirected {
                        adv_data: p.adv,
                        scan_data: &[],
                    },
                )
                .await
                .map_err(|_| {
                    record(20, 31);
                    PortError::Fault
                })?;
            link(&status, LinkStage::Advertising);
            let connection = advertiser
                .accept()
                .await
                .map_err(|_| {
                    record(20, 32);
                    PortError::Fault
                })?
                .with_attribute_server(server)
                .map_err(|_| {
                    record(20, 33);
                    PortError::Fault
                })?;
            link(&status, LinkStage::Connected);
            {
                let mut driver = pin!(drive_connection(&connection, profile, bridge));
                poll_fn(|cx| {
                    if cancel_connection.get() {
                        connection.raw().disconnect();
                        record(21, 1); // Actual local disconnect request, never peer confirmation.
                        return Poll::Ready(Err(PortError::Fault));
                    }
                    driver.as_mut().poll(cx)
                })
                .await
                .inspect_err(|_| {
                    record(20, 34);
                })?;
            }
            link(&status, LinkStage::Disconnected);
            // Clean disconnect: connection drops before next advertise. A bounded timer yields scheduling between reconnects.
            drop(connection);
            embassy_time::Timer::after_millis(25).await;
        }
        #[allow(unreachable_code)]
        Ok::<(), PortError>(())
    });
    record(14, core::mem::size_of_val(gatt.as_ref().get_ref()) as u32);
    let mut transport = TroubleTransport::new(bridge, host.as_mut(), gatt.as_mut());
    #[cfg(not(feature = "native-health"))]
    let mut background = storage.work();
    #[cfg(feature = "native-health")]
    let mut background = storage.work(&health_log);
    let mut ui = ObservedUi(&mut ui);
    let mut session = 1u32;
    loop {
        poll_time();
        let stamp = embassy_time_driver::now() / 1000;
        let polled = transport.poll(stamp, 2);
        record(3, u32::from(ui.step(stamp).is_err()));
        record(4, u32::from(background.step(stamp).is_err()));
        if polled.is_err() {
            link(&status, LinkStage::Fault);
            cancel_connection.set(true);
            terminal(&mut transport, &mut ui, &mut background, &host_exited);
        }
        if let Some(scope) = checked(transport.scope(), 80) {
            if connection_session(
                storage,
                #[cfg(feature = "native-health")]
                &health_log,
                #[cfg(feature = "native-health")]
                &health_owner,
                scope,
                session,
                p,
                &mut transport,
                &mut ui,
                &mut background,
            ) {
                link(&status, LinkStage::Fault);
                cancel_connection.set(true);
                terminal(&mut transport, &mut ui, &mut background, &host_exited);
            }
            session = checked(session.checked_add(1).ok_or(()), 81);
        }
    }
}
fn terminal(
    transport: &mut impl Transport,
    ui: &mut impl Work,
    background: &mut impl Work,
    host_exited: &Cell<bool>,
) -> ! {
    loop {
        poll_time();
        let now = embassy_time_driver::now() / 1000;
        let _ = transport.poll(now, 2);
        record(3, u32::from(ui.step(now).is_err()));
        record(4, u32::from(background.step(now).is_err()));
        if host_exited.get() {
            stop_radio();
        }
    }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    fail(0xffff)
}

#[derive(Debug)]
struct Clock;
impl device_spi_nor::writable::Monotonic for Clock {
    fn now_ms(&mut self) -> u64 {
        embassy_time_driver::now() / 1000
    }
}
impl device_history_store::HistoryClock for Clock {
    fn now_ms(&self) -> Result<u64, device_history_service::HistoryFailure> {
        Ok(embassy_time_driver::now() / 1000)
    }
}
impl wattch5_candidate::history::HistoryEventClock for Clock {
    fn now_ms(&self) -> Result<u64, device_history_service::HistoryFailure> {
        Ok(embassy_time_driver::now() / 1000)
    }
}
impl embedded_hal::delay::DelayNs for Delay {
    fn delay_ns(&mut self, ns: u32) {
        settle(ns.div_ceil(1000));
    }
}
mod storage;
use storage::{connection_session, initialize_storage};

type NativeBackend<'a> = ObservedReset<'a, SydBackend<'static, board_syd8810::hardware::RadioMmio>>;
#[inline(never)]
fn native_stack<'a>(
    mmio: &mut Mmio,
    gate: &'a ResetGate,
    table: ConfigurationTable<'static>,
    address: [u8; 6],
) -> ble_host_trouble::host::Stack<'a, HciController<NativeBackend<'a>>, DefaultPacketPool> {
    let radio = checked(Radio::new(checked(mmio.radio(), 6), 4096), 7);
    let backend = ObservedReset::new(SydBackend::new(radio, table, 8000, settle), gate);
    let mut controller = Controller::new(backend, Version::new(6, 0xffff, 1));
    checked(controller.reset(), 8);
    // SAFETY: sole runtime clock, real forwarded vector and checked nominal64MHz.
    checked(
        unsafe { start_irq_time(mmio, checked(NonZeroU32::new(64_000_000).ok_or(()), 9)) },
        10,
    );
    // SAFETY: only SysTick enabled; NVIC GPIO/radio sources remain masked.
    unsafe {
        core::arch::asm!("cpsie i", options(nostack));
    }
    checked(gate.begin(), 11);
    let hci = HciController::new(controller);
    record(10, core::mem::size_of_val(&hci) as u32);
    build(
        hci,
        HOST.init(Resources::new()),
        checked(RandomAddress::new(address), 12),
    )
}

struct AssetReader<'a, S: embedded_hal::spi::SpiDevice>(
    &'a device_spi_nor::writable::NorChip<S, Clock>,
);
impl<S: embedded_hal::spi::SpiDevice> moteframe_core::ReadAt for AssetReader<'_, S> {
    type Error = PortError;
    fn capacity(&self) -> u32 {
        0x200000
    }
    fn read_exact(&mut self, offset: u32, out: &mut [u8]) -> Result<(), PortError> {
        if offset
            .checked_add(out.len() as u32)
            .is_none_or(|end| end > 0x200000)
        {
            return Err(PortError::InvalidInput);
        }
        self.0.read_asset(offset, out).map_err(|e| match e {
            device_spi_nor::writable::Fault::Busy
            | device_spi_nor::writable::Fault::NeedsRecovery => PortError::Unavailable,
            _ => PortError::Fault,
        })
    }
}

impl<S: embedded_hal::spi::SpiDevice> core::fmt::Debug for AssetReader<'_, S> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("AssetReader").finish_non_exhaustive()
    }
}
