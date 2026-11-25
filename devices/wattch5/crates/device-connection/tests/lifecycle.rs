//! Permanent offline regressions from the independent final application review.
#![allow(clippy::unwrap_used)]
use core::cell::Cell;
use device_connection::{ConnectionError, ControlConnection};
use device_protocol::{Frame, ModuleEntry, ProtocolLimits};
use device_services::{BuildProfile, Config, DeviceInfo, ServiceHandler, SessionError};
struct Handler<'a> {
    calls: &'a Cell<u32>,
    fail: &'a Cell<bool>,
    advanced: &'a Cell<u64>,
}
impl ServiceHandler for Handler<'_> {
    fn required_features(&self, _: &ModuleEntry, _: u8) -> Option<u32> {
        None
    }
    fn authorized(&self, _: Frame<'_>) -> bool {
        false
    }
    fn dispatch(&mut self, _: Frame<'_>, _: &mut [u8]) -> Result<usize, SessionError> {
        Err(SessionError::InvalidHandlerResponse)
    }
    fn advance(&mut self, now: u64) -> Result<(), SessionError> {
        self.advanced.set(now);
        Ok(())
    }
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        self.calls.set(self.calls.get() + 1);
        if self.fail.get() {
            Err(SessionError::InvalidHandlerResponse)
        } else {
            Ok(())
        }
    }
}
fn config() -> Config<'static> {
    static ENTRIES: [ModuleEntry; 1] = [ModuleEntry {
        module: 0,
        major: 1,
        minor: 2,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    }];
    Config {
        session: 1,
        capability_rev: 1,
        profile: BuildProfile::Commercial,
        info: DeviceInfo {
            product_id: 0,
            fw_major: 1,
            fw_minor: 0,
            fw_patch: 0,
            build_id: 0,
        },
        modules: &ENTRIES,
        limits: ProtocolLimits {
            rx_frame_max: 20,
            tx_frame_max: 20,
            control_max: 128,
            data_max: 0,
            idle_ms: 2000,
            fragment_ms: 100,
            response_ms: 10000,
        },
    }
}
#[test]
fn failed_control_cleanup_is_retained_and_each_explicit_close_retries_actual_owner() {
    let (calls, fail, advanced) = (Cell::new(0), Cell::new(true), Cell::new(0));
    let mut handler = Handler {
        calls: &calls,
        fail: &fail,
        advanced: &advanced,
    };
    let mut control = ControlConnection::new(config()).unwrap();
    let expected = Err(ConnectionError::CleanupFailed(
        SessionError::InvalidHandlerResponse,
    ));
    assert_eq!(control.close(&mut handler), expected);
    assert_eq!(control.close(&mut handler), expected);
    assert_eq!(calls.get(), 2);
    fail.set(false);
    assert_eq!(control.close(&mut handler), Ok(()));
    assert_eq!(calls.get(), 3);
    assert_eq!(control.close(&mut handler), Ok(()));
    assert_eq!(calls.get(), 3);
    assert_eq!(
        control.notifications_enabled(),
        Err(ConnectionError::Closed)
    );
}
#[test]
fn tick_advances_owned_handlers_with_actual_time_and_rejects_backwards_input_first() {
    let (calls, fail, advanced) = (Cell::new(0), Cell::new(false), Cell::new(0));
    let mut handler = Handler {
        calls: &calls,
        fail: &fail,
        advanced: &advanced,
    };
    let mut control = ControlConnection::new(config()).unwrap();
    assert_eq!(control.tick(60000, &mut handler), Ok(()));
    assert_eq!(advanced.get(), 60000);
    assert!(control.tick(59999, &mut handler).is_err());
    assert_eq!(advanced.get(), 60000);
    assert_eq!(calls.get(), 1);
}
