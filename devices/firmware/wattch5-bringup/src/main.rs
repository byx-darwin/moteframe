//! Dedicated native CONTROL-only diagnostic image; hardware qualification remains closed.
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
use device_runtime::{Budgets, PortError, Runtime, Transport, Work};
use embassy_sync::blocking_mutex::raw::NoopRawMutex;
use static_cell::StaticCell;
use syd881x_radio::{ConfigurationTable, Radio};
use wattch5_bringup::{
    CoreFactory, LinkStage, ObservedReset, Readiness, ReadinessUi, ResetGate, UnboundStorage,
};
use wattch5_product::runtime::gatt::{
    Bridge, ControlProfile, ControlUuids, TroubleTransport, drive_connection,
};
mod physical;
global_asm!(include_str!(
    "../../../platforms/syd881x/runtime/startup/wattch5-bringup.s"
));
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
static BRIDGE: StaticCell<Bridge<1, 1>> = StaticCell::new();
static HOST: StaticCell<Resources> = StaticCell::new();
static FACTORY: StaticCell<CoreFactory> = StaticCell::new();
static RUNTIME: StaticCell<Runtime<'static, CoreFactory>> = StaticCell::new();
#[unsafe(link_section = ".probe_status")]
static mut STATUS: [u32; 32] = [0; 32];
#[used]
#[unsafe(link_section = ".host_error")]
static mut HOST_ERROR: [u8; 128] = [0; 128];
// ARM type sizes retained in the actual linked image, separately from dynamic future sizes.
#[used]
#[unsafe(link_section = ".candidate_manifest")]
static TYPE_SIZES: [u32; 6] = [
    core::mem::size_of::<CoreFactory>() as u32,
    core::mem::size_of::<Runtime<'static, CoreFactory>>() as u32,
    core::mem::size_of::<Bridge<1, 1>>() as u32,
    core::mem::size_of::<Resources>() as u32,
    core::mem::size_of::<AttributeServer<'static, NoopRawMutex, DefaultPacketPool, 10, 1>>() as u32,
    core::mem::size_of::<physical::Inputs<'static>>() as u32,
];
unsafe extern "C" {
    static __sidata: u8;
    static mut __sdata: u8;
    static mut __edata: u8;
    static mut __sbss: u8;
    static mut __ebss: u8;
}
fn record(index: usize, value: u32) {
    if index < 32 {
        // SAFETY: Serialized foreground writes a bounded word in the linker-reserved diagnostic region.
        unsafe {
            write_volatile((&raw mut STATUS).cast::<u32>().add(index), value);
        }
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
    // SAFETY: Reset exclusively owns the256-byte NOLOAD diagnostics and masks unsupported NVIC/SysTick/watchdog before IRQ clock acquisition.
    unsafe {
        for i in 0..32 {
            write_volatile((&raw mut STATUS).cast::<u32>().add(i), 0);
        }
        for i in 0..128 {
            write_volatile((&raw mut HOST_ERROR).cast::<u8>().add(i), 0);
        }
        write_volatile(0xe000_e180 as *mut u32, u32::MAX);
        write_volatile(0xe000_e280 as *mut u32, u32::MAX);
        write_volatile(0xe000_e010 as *mut u32, 0);
        let watchdog = 0x5000_10a0 as *mut u8;
        write_volatile(watchdog, read_volatile(watchdog) & !3);
    }
    record(0, 0x5735_4255);
    record(1, 1);
    run()
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
#[inline(never)]
fn control_bridge() -> &'static Bridge<1, 1> {
    BRIDGE.init_with(|| checked(Bridge::new(), 25))
}
#[inline(never)]
fn control_server<'v>(
    p: &TestProfile,
    rx: &'v mut [u8; 20],
    tx: &'v mut [u8; 20],
) -> (
    AttributeServer<'v, NoopRawMutex, DefaultPacketPool, 10, 1>,
    ControlProfile,
) {
    let mut attributes: AttributeTable<'_, NoopRawMutex, 10> = AttributeTable::new();
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
            },
            rx,
            tx,
        ),
        26,
    );
    (AttributeServer::new(attributes), profile)
}
#[inline(never)]
fn foreground_step<T: Transport, U: Work>(
    runtime: &mut Runtime<'_, CoreFactory>,
    transport: &mut T,
    ui: &mut U,
    background: &mut UnboundStorage,
    stamp: u64,
) -> device_runtime::StepReport {
    runtime.step(stamp, transport, ui, background)
}
#[inline(never)]
fn run() -> ! {
    let p = checked(TEST_PROFILE.ok_or(()), 1);
    let bytes = checked(BOARD_TABLE.ok_or(()), 2);
    let table = checked(ConfigurationTable::new(bytes), 3);
    if table.header()[1] & 15 != 0 {
        fail(4)
    }
    // SAFETY: Sole MMIO claim after initialized application RAM; selected SYD physical map.
    let mut mmio = checked(unsafe { Mmio::claim() }, 5);
    let radio = checked(Radio::new(checked(mmio.radio(), 6), 4096), 7);
    let gate = ResetGate::new();
    let backend = ObservedReset::new(SydBackend::new(radio, table, 8000, settle), &gate);
    // Explicit controller test metadata;0xffff asserts no allocated company identity.
    let mut controller = Controller::new(backend, Version::new(6, 0xffff, 1));
    checked(controller.reset(), 8);
    // SAFETY: Real forwarded vector15 points to the sole runtime SysTick handler; NVIC remains fully masked. Nominal64MHz is checked above; no sleep/clock changes occur.
    checked(
        unsafe { start_irq_time(&mut mmio, checked(NonZeroU32::new(64_000_000).ok_or(()), 9)) },
        10,
    );
    // SAFETY: Complete nonzero vectors and initialized exclusive time state now permit only SysTick; every NVIC source stays disabled, including GPIO/LL/Timer3/LLC2.
    unsafe {
        core::arch::asm!("cpsie i", options(nostack));
    }
    checked(gate.begin(), 11); // Clear the first reset observation, requiring the actual Host reset.
    let hci = HciController::new(controller);
    record(10, core::mem::size_of_val(&hci) as u32);
    let stack = build(
        hci,
        HOST.init(Resources::new()),
        checked(RandomAddress::new(p.address), 12),
    );
    record(11, core::mem::size_of_val(&stack) as u32);
    let mut runner = stack.runner();
    let mut peripheral = stack.peripheral();
    let host_exited = Cell::new(false);
    let mut host = pin!(async {
        let result = runner.run().await;
        host_exited.set(true);
        record(19, if result.is_err() { 2 } else { 1 });
        result
    });
    record(12, core::mem::size_of_val(host.as_ref().get_ref()) as u32);
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
    let mut display = device_display::DcsDisplay::new(
        device_display::SpiPanelBus::new(checked(board.spi(config), 18), select, dc),
        panel::DIMENSIONS,
        panel::OFFSET,
    );
    let mut delay = Delay;
    checked(reset_display(&mut reset, &mut delay), 19);
    checked(display.initialize(panel::INITIALIZATION, &mut delay), 20);
    let inputs = checked(physical::Inputs::new(&board, &mut delay), 21);
    let status = Cell::new(Readiness::new());
    let mut ui = checked(ReadinessUi::new(display, inputs, &status, &gate), 22);
    record(13, core::mem::size_of_val(&ui) as u32);
    checked(light.set_duty(50), 23);
    let factory = FACTORY.init_with(|| {
        checked(
            CoreFactory::new(device_services::DeviceInfo {
                product_id: p.product_id,
                fw_major: 0,
                fw_minor: 1,
                fw_patch: 0,
                build_id: p.build_id,
            }),
            24,
        )
    });
    let budgets = checked(Budgets::new(1, 1, 2), 24);
    let runtime = RUNTIME.init_with(|| Runtime::new(factory, budgets));
    let bridge = control_bridge();
    let mut rx = [0; 20];
    let mut tx = [0; 20];
    let (server, profile) = control_server(&p, &mut rx, &mut tx);
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
                .with_attribute_server(&server)
                .map_err(|_| {
                    record(20, 33);
                    PortError::Fault
                })?;
            link(&status, LinkStage::Connected);
            {
                let mut driver = pin!(drive_connection(&connection, &profile, bridge));
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
    let mut background = UnboundStorage;
    loop {
        poll_time();
        let stamp = embassy_time_driver::now() / 1000;
        let report = foreground_step(runtime, &mut transport, &mut ui, &mut background, stamp);
        record(3, u32::from(report.ui.is_err()));
        record(
            4,
            u32::from(report.background == Err(PortError::Unavailable)),
        );
        if !gate.ui_allowed() {
            fail(27)
        }
        if report.connection.is_err() {
            record(5, 1);
            link(&status, LinkStage::Fault);
            let cleanup = runtime.shutdown(&mut transport);
            record(6, u32::from(cleanup.is_err()));
            cancel_connection.set(true);
            // Terminal BLE lane: preserve visible diagnostics; never replace a failed Host or claim reconnect recovery.
            loop {
                poll_time();
                // Continue bounded Host progress so a healthy Host can execute the real disconnect request. Completed/failed futures are never replaced.
                let result = transport.poll(embassy_time_driver::now() / 1000, 2);
                record(22, u32::from(result.is_err()));
                if host_exited.get() {
                    stop_radio();
                }
                if !gate.ui_allowed() {
                    fail(27)
                }
                let result = ui.step(embassy_time_driver::now() / 1000);
                record(3, u32::from(result.is_err()));
            }
        }
    }
}
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    fail(0xffff)
}
