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
pub(super) use arm::run;

#[cfg(target_arch = "arm")]
mod arm {
    use super::{ClockFault, CoarseTime};
    use crate::{clock, diagnostic, gatt_board_radio::GATT_ADDRESS};
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
    use fr8029d_bringup::gatt_runner::{self, GattIdentity, GattReport, GattRunError, Resources};

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

    impl ForegroundTime {
        fn sample(&self) -> (u64, Option<ClockFault>) {
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

        fn schedule_wake(&self, _at: u64, waker: &Waker) {
            // The sole foreground executor polls every lane on every pass.
            // Its no-op waker requires neither timer interrupts nor a queue.
            waker.wake_by_ref();
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
    const FAULTED: u32 = 0x8000_0000;
    const IDENTITY: (&str, &[u8; 14]) = if cfg!(feature = "ble-gatt-freertos") {
        ("FR29-RTOS", b"\x02\x01\x06\x0a\x09FR29-RTOS")
    } else if cfg!(feature = "ble-gatt-xip") {
        ("FR29-XN01", b"\x02\x01\x06\x0a\x09FR29-XN01")
    } else {
        ("FR29-GAT2", b"\x02\x01\x06\x0a\x09FR29-GAT2")
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
        diagnostic(PHASE, 1);

        if let Ok(address) = RandomAddress::new(GATT_ADDRESS) {
            let identity = GattIdentity {
                address,
                version: Version::new(0x08, 0xffff, 1),
                name: IDENTITY.0,
                advertising_data: IDENTITY.1,
            };
            let mut resources = Resources::new();
            let mut report = Report::default();
            let mut app = pin!(gatt_runner::run(
                backend,
                &mut resources,
                identity,
                |event| {
                    report.record(event);
                }
            ));
            let mut context = Context::from_waker(Waker::noop());
            loop {
                if let Poll::Ready(error) = app.as_mut().poll(&mut context) {
                    diagnostic(APP_FAULT, app_fault(&error));
                    break;
                }
                // Also sample when the idle Host performs no time queries.
                if let Some(fault) = TIME.sample().1 {
                    diagnostic(CLOCK_FAULT, fault as u32);
                    break;
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
}
