//! Module01 command test uses an exact test clock, never a hardware-frequency claim.
use core::cell::{Cell, RefCell};
use device_protocol::{Frame, Header, Kind, ModuleEntry};
use device_services::{
    ClockSource, ClockSourceError, ServiceHandler, SoftwareClock, TimeAccess, TimeHandler,
};
struct Source {
    now: Cell<u64>,
    available: Cell<bool>,
}
impl ClockSource for Source {
    fn monotonic_us(&self) -> Result<u64, ClockSourceError> {
        if self.available.get() {
            Ok(self.now.get())
        } else {
            Err(ClockSourceError::Unavailable)
        }
    }
}
struct Access(Cell<bool>);
impl TimeAccess for Access {
    fn allowed(&self, _set: bool) -> bool {
        self.0.get()
    }
}
fn entry() -> ModuleEntry {
    ModuleEntry {
        module: 1,
        major: 1,
        minor: 0,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    }
}
fn req(op: u8, bytes: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module: 1,
            opcode: op,
            message_id: 1,
            offset: 0,
            total_length: u16::try_from(bytes.len()).unwrap_or_else(|e| panic!("{e:?}")),
        },
        payload: bytes,
    }
}
#[test]
fn test_should_dispatch_real_mapping_and_set_time_without_rewinding_monotonic_source() {
    let clock = RefCell::new(SoftwareClock::new(0));
    let source = Source {
        now: Cell::new(10_000),
        available: Cell::new(true),
    };
    let access = Access(Cell::new(true));
    let mut h =
        TimeHandler::new(entry(), &clock, &source, &access).unwrap_or_else(|e| panic!("{e:?}"));
    let mut out = [0; 128];
    assert_eq!(
        h.dispatch(req(2, &[0xe8, 3, 0, 0, 0, 0, 0, 0, 7, 0, 0, 0]), &mut out),
        Ok(25)
    );
    assert_eq!(&out[..6], &[0, 0, 0, 0, 0xe8, 3]);
    assert_eq!(out[24], 1);
    source.now.set(11_000);
    h.dispatch(req(1, &[]), &mut out)
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(&out[4..6], &[0xe9, 3]);
    assert_eq!(out[12..14], [0xf8, 0x2a]);
    assert_eq!(h.dispatch(req(2, &[0; 13]), &mut out), Ok(4));
    assert_eq!(out[0], 1);
    source.available.set(false);
    assert_eq!(h.dispatch(req(1, &[]), &mut out), Ok(4));
    assert_eq!(out[0], 14);
}
#[test]
fn test_should_reject_short_output_before_sync_and_deny_untrusted_time_source() {
    let clock = RefCell::new(SoftwareClock::new(0));
    let source = Source {
        now: Cell::new(100),
        available: Cell::new(true),
    };
    let access = Access(Cell::new(true));
    let mut h =
        TimeHandler::new(entry(), &clock, &source, &access).unwrap_or_else(|e| panic!("{e:?}"));
    assert!(h.dispatch(req(2, &[0; 12]), &mut [0; 24]).is_err());
    assert!(
        !clock
            .borrow_mut()
            .read(100)
            .unwrap_or_else(|e| panic!("{e:?}"))
            .valid
    );
    access.0.set(false);
    let mut out = [0xa5; 128];
    assert_eq!(h.dispatch(req(2, &[0; 12]), &mut out), Ok(4));
    assert_eq!(&out[..4], &[5, 0, 3, 255]);
    assert!(
        !clock
            .borrow_mut()
            .read(100)
            .unwrap_or_else(|e| panic!("{e:?}"))
            .valid
    );
}
