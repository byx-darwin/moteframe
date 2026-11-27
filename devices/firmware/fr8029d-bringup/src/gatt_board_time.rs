//! Foreground execution and board clock for the experimental GATT image.

use fr8029d_bringup::advertising::CLOCK_MASK;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
enum ClockFault {
    LatchTimeout = 1,
    Discontinuity = 2,
}

#[derive(Debug)]
struct CoarseTime {
    previous: Option<u32>,
    ticks: u64,
    fault: Option<ClockFault>,
}

impl CoarseTime {
    const fn new() -> Self {
        Self {
            previous: None,
            ticks: 0,
            fault: None,
        }
    }

    fn rebase(&mut self, reading: u32) {
        if self.previous.replace(reading).is_some() {
            // The last fine reading may have been near the end of its tick.
            self.ticks = self.ticks.saturating_add(1);
        }
    }

    fn sample(&mut self, reading: Option<u32>) -> u64 {
        if self.fault.is_none() {
            match (self.previous, reading) {
                (None, sample) => self.previous = sample,
                (Some(previous), Some(raw)) => {
                    let elapsed = raw.wrapping_sub(previous) & CLOCK_MASK;
                    if elapsed <= CLOCK_MASK / 2 {
                        self.ticks = self.ticks.saturating_add(u64::from(elapsed));
                        self.previous = Some(raw);
                    } else {
                        self.fault = Some(ClockFault::Discontinuity);
                    }
                }
                (Some(_), None) => self.fault = Some(ClockFault::LatchTimeout),
            }
        }
        // Two coarse ticks are exactly 625 microseconds. Convert the total
        // to retain the half microsecond across any number of samples.
        (self.ticks / 2)
            .saturating_mul(625)
            .saturating_add((self.ticks % 2) * 312)
    }
}

#[cfg(target_arch = "arm")]
pub(super) use arm::{rebase_clock, run};

#[cfg(target_arch = "arm")]
mod arm {
    use super::{ClockFault, CoarseTime};
    #[cfg(not(feature = "ble-gatt-event-driven"))]
    use crate::clock;
    use crate::{diagnostic, gatt_board_radio::GATT_ADDRESS};
    use ble_controller::{ControllerError, HciError, LinkBackend, RandomAddress, Version};
    use ble_host_trouble::host::prelude::BleHostError;
    use core::{
        arch::asm,
        cell::RefCell,
        future::Future,
        hint::spin_loop,
        pin::pin,
        task::{Context, Poll, Waker},
    };
    use critical_section::Mutex;
    use embassy_time_driver::Driver;
    #[cfg(not(feature = "ble-gatt-event-driven"))]
    use fr8029d_bringup::gatt_runner::run as run_gatt;
    #[cfg(all(feature = "ble-gatt-event-driven", not(feature = "ble-gatt-ota")))]
    use fr8029d_bringup::gatt_runner::run_event_driven as run_gatt;
    use fr8029d_bringup::gatt_runner::{GattIdentity, GattReport, GattRunError, Resources};
    #[cfg(feature = "ble-gatt-event-driven")]
    use {
        crate::gatt_board_wake as wake,
        core::cell::Cell,
        fr8029d_bringup::{
            connection::PacketTime,
            wake::{RadioDeadline, next_delay_us},
        },
    };

    struct SingleCore;
    critical_section::set_impl!(SingleCore);

    // SAFETY: The probe uses one Cortex-M33 core. PRIMASK masks configurable
    // exceptions and restores the prior state for nested critical sections.
    unsafe impl critical_section::Impl for SingleCore {
        unsafe fn acquire() -> critical_section::RawRestoreState {
            let previous: u32;
            // SAFETY: The startup keeps this probe in privileged Thread mode.
            // The absence of nomem keeps protected memory inside the boundary.
            unsafe {
                asm!("mrs {old}, PRIMASK", "cpsid i", old = out(reg) previous,
                    options(nostack, preserves_flags));
            }
            previous & 1 != 0
        }

        unsafe fn release(previous: critical_section::RawRestoreState) {
            // SAFETY: The matching acquire supplied the previous PRIMASK bit.
            unsafe {
                asm!("msr PRIMASK, {old}", old = in(reg) u32::from(previous),
                    options(nostack, preserves_flags));
            }
        }
    }

    struct ForegroundTime(Mutex<RefCell<CoarseTime>>);
    embassy_time_driver::time_driver_impl!(
        static TIME: ForegroundTime = ForegroundTime(Mutex::new(RefCell::new(CoarseTime::new())))
    );
    #[cfg(feature = "ble-gatt-event-driven")]
    static HOST_DEADLINE: Mutex<Cell<Option<u64>>> = Mutex::new(Cell::new(None));

    pub(crate) fn rebase_clock(reading: u32) {
        critical_section::with(|cs| TIME.0.borrow(cs).borrow_mut().rebase(reading));
    }

    impl ForegroundTime {
        #[cfg(feature = "ble-gatt-event-driven")]
        fn sample_precise(&self, reading: Option<PacketTime>) -> (u64, Option<ClockFault>) {
            critical_section::with(|cs| {
                let mut state = self.0.borrow(cs).borrow_mut();
                let coarse_us = state.sample(reading.map(|now| now.coarse));
                let fine_half_us = reading.map_or(0, PacketTime::elapsed_half_us);
                // Respect the half microsecond of the extended tick count.
                let fine_us = u64::midpoint(u64::from(fine_half_us), state.ticks % 2);
                let time = coarse_us.saturating_add(fine_us);
                (time, state.fault)
            })
        }

        fn sample(&self) -> (u64, Option<ClockFault>) {
            #[cfg(feature = "ble-gatt-event-driven")]
            return self.sample_precise(wake::snapshot());
            #[cfg(not(feature = "ble-gatt-event-driven"))]
            critical_section::with(|cs| {
                let mut state = self.0.borrow(cs).borrow_mut();
                let time = state.sample(clock());
                (time, state.fault)
            })
        }
    }

    impl Driver for ForegroundTime {
        fn now(&self) -> u64 {
            self.sample().0
        }

        fn schedule_wake(&self, at: u64, waker: &Waker) {
            #[cfg(feature = "ble-gatt-event-driven")]
            critical_section::with(|cs| {
                // One executor polls every lane on each wake. All timers in
                // this image share its waker, so only the earliest is needed.
                let deadline = HOST_DEADLINE.borrow(cs);
                deadline.set(Some(deadline.get().map_or(at, |earliest| earliest.min(at))));
            });
            #[cfg(not(feature = "ble-gatt-event-driven"))]
            waker.wake_by_ref();
            let _ = (at, waker);
        }
    }

    const PHASE: usize = 40;
    const WRITES: usize = 41;
    const VALUE_LOW: usize = 42;
    const VALUE_HIGH: usize = 43;
    const VALUE_HANDLE: usize = 44;
    const DISCONNECTS: usize = 45;
    const APP_FAULT: usize = 46;
    const CLOCK_FAULT: usize = 47;
    const DIAGNOSTIC_VERSION: usize = 63;
    const FAULTED: u32 = 0x8000_0000;
    const MAC_PROBE: bool = cfg!(all(
        feature = "ble-gatt-mac-probe",
        not(feature = "ble-gatt-xip")
    ));
    const IDENTITY: (&str, &[u8; 14]) = if cfg!(feature = "ble-gatt-freertos") {
        let advertising_data = if MAC_PROBE {
            b"\x02\x01\x06\x0a\xff\xff\xffMFR29R1"
        } else {
            b"\x02\x01\x06\x0a\x09FR29-RTOS"
        };
        ("FR29-RTOS", advertising_data)
    } else if cfg!(feature = "ble-gatt-xip") {
        ("FR29-XN01", b"\x02\x01\x06\x0a\x09FR29-XN01")
    } else {
        let advertising_data = if MAC_PROBE {
            b"\x02\x01\x06\x0a\xff\xff\xffMFR29A1"
        } else {
            b"\x02\x01\x06\x0a\x09FR29-GAT2"
        };
        ("FR29-GAT2", advertising_data)
    };

    #[derive(Default)]
    struct Report {
        writes: u32,
        disconnects: u32,
    }

    impl Report {
        fn record(&mut self, event: GattReport) {
            let phase = match event {
                GattReport::Advertising => 2,
                GattReport::Connected => 3,
                GattReport::Committed(value) => {
                    let bytes = value.bytes;
                    diagnostic(
                        VALUE_LOW,
                        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
                    );
                    diagnostic(
                        VALUE_HIGH,
                        u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
                    );
                    diagnostic(VALUE_HANDLE, u32::from(value.handle));
                    self.writes = self.writes.saturating_add(1);
                    diagnostic(WRITES, self.writes);
                    4
                }
                GattReport::Disconnected => {
                    self.disconnects = self.disconnects.saturating_add(1);
                    diagnostic(DISCONNECTS, self.disconnects);
                    5
                }
            };
            diagnostic(PHASE, phase);
        }
    }

    fn app_fault(error: &GattRunError<u32>) -> u32 {
        match error {
            GattRunError::Gap(_) => 2,
            GattRunError::Profile => 3,
            GattRunError::Host(BleHostError::Controller(error)) => {
                if let HciError::Controller(ControllerError::Backend(code)) = error {
                    diagnostic(39, *code);
                }
                4
            }
            GattRunError::Host(BleHostError::BleHost(_)) => 5,
            GattRunError::Gatt(_) => 6,
            GattRunError::WriteDelivery(_) => 7,
            GattRunError::MissingWrite => 8,
            GattRunError::HostStopped => 9,
        }
    }

    #[inline(never)]
    pub(crate) fn run<B: LinkBackend<Error = u32>>(backend: B) -> ! {
        for index in PHASE..=CLOCK_FAULT {
            diagnostic(index, 0);
        }
        diagnostic(DIAGNOSTIC_VERSION, 1);
        diagnostic(PHASE, 1);

        if let Ok(address) = RandomAddress::new(GATT_ADDRESS) {
            let identity = GattIdentity {
                address,
                version: Version::new(
                    if crate::gatt_board_radio::ADAPTIVE_PHY {
                        0x09
                    } else {
                        0x08
                    },
                    0xffff,
                    1,
                ),
                name: IDENTITY.0,
                advertising_data: IDENTITY.1,
            };
            let mut resources = Resources::new();
            let mut report = Report::default();
            #[cfg(feature = "ble-gatt-event-driven")]
            wake::init();
            #[cfg(feature = "ble-gatt-ota")]
            // SAFETY: This maintenance image exclusively owns QSPI; acquisition is disabled.
            let Ok(mut ota) = (unsafe { crate::qspi_slots::ota_receiver() }) else {
                diagnostic(APP_FAULT, 8);
                loop {
                    spin_loop();
                }
            };
            #[cfg(feature = "ble-gatt-ota")]
            let mut receive = |frame: Option<&[u8]>| {
                if let Some(frame) = frame {
                    ota.receive(frame).is_ok()
                } else {
                    ota.abort();
                    true
                }
            };
            #[cfg(feature = "ble-gatt-ota")]
            let mut app = pin!(fr8029d_bringup::gatt_runner::run_event_driven_ota(
                backend,
                &mut resources,
                identity,
                |event| report.record(event),
                &mut receive
            ));
            #[cfg(not(feature = "ble-gatt-ota"))]
            let mut app = pin!(run_gatt(backend, &mut resources, identity, |event| {
                report.record(event);
            }));
            #[cfg(feature = "ble-gatt-event-driven")]
            let waker = wake::waker();
            #[cfg(feature = "ble-gatt-event-driven")]
            let mut context = Context::from_waker(&waker);
            #[cfg(not(feature = "ble-gatt-event-driven"))]
            let mut context = Context::from_waker(Waker::noop());
            loop {
                #[cfg(feature = "ble-gatt-event-driven")]
                {
                    wake::begin_pass();
                    critical_section::with(|cs| HOST_DEADLINE.borrow(cs).set(None));
                }
                if let Poll::Ready(error) = app.as_mut().poll(&mut context) {
                    diagnostic(APP_FAULT, app_fault(&error));
                    break;
                }
                // Also sample when the idle Host performs no time queries.
                #[cfg(feature = "ble-gatt-event-driven")]
                let snapshot = wake::snapshot();
                #[cfg(feature = "ble-gatt-event-driven")]
                let (now_us, fault) = TIME.sample_precise(snapshot);
                #[cfg(not(feature = "ble-gatt-event-driven"))]
                let (_, fault) = TIME.sample();
                if let Some(fault) = fault {
                    diagnostic(CLOCK_FAULT, fault as u32);
                    break;
                }
                #[cfg(feature = "ble-gatt-event-driven")]
                if let Some(now) = snapshot {
                    let radio = wake::radio_deadline();
                    let host = critical_section::with(|cs| HOST_DEADLINE.borrow(cs).get());
                    wake::wait(
                        next_delay_us(now, now_us, radio, host),
                        matches!(radio, Some(RadioDeadline::Completion { .. })),
                    );
                }
            }
        } else {
            diagnostic(APP_FAULT, 1);
        }
        diagnostic(PHASE, FAULTED);
        loop {
            spin_loop();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ClockFault, CoarseTime};

    #[test]
    fn test_should_extend_the_coarse_clock_across_wrap_with_half_tick_precision() {
        let mut time = CoarseTime::new();
        assert_eq!(time.sample(None), 0);
        assert_eq!(time.sample(Some(0x0fff_fffe)), 0);
        assert_eq!(time.sample(Some(0x0fff_ffff)), 312);
        assert_eq!(time.sample(Some(0)), 625);
        assert_eq!(time.sample(Some(1)), 937);
        assert_eq!(time.sample(Some(2)), 1250);
        assert_eq!(time.fault, None);
    }

    #[test]
    fn test_should_hold_time_on_a_backward_clock_sample() {
        let mut time = CoarseTime::new();
        assert_eq!(time.sample(Some(32)), 0);
        assert_eq!(time.sample(Some(35)), 937);
        assert_eq!(time.sample(Some(34)), 937);
        assert_eq!(time.fault, Some(ClockFault::Discontinuity));
        assert_eq!(time.sample(Some(36)), 937);
    }

    #[test]
    fn test_should_hold_time_on_a_latch_timeout_after_the_first_sample() {
        let mut time = CoarseTime::new();
        assert_eq!(time.sample(Some(12)), 0);
        assert_eq!(time.sample(Some(14)), 625);
        assert_eq!(time.sample(None), 625);
        assert_eq!(time.fault, Some(ClockFault::LatchTimeout));
    }

    #[test]
    fn time_keeps_progress_after_the_baseband_clock_restarts() {
        let mut time = CoarseTime::new();
        assert_eq!(time.sample(Some(1_000)), 0);
        assert_eq!(time.sample(Some(1_004)), 1_250);
        time.rebase(2);
        assert_eq!(time.sample(Some(2)), 1_562);
        assert_eq!(time.sample(Some(3)), 1_875);
        assert_eq!(time.fault, None);
    }
}
