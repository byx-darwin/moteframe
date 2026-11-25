//! A real committed-generation port must remain authoritative through final queue admission.
#![allow(clippy::expect_used)]
use core::cell::Cell;
use device_application::{CommonEventPort, CommonEvents, ControlEvent, EventSource, SourceFailure};
struct Port<'a> {
    revision: &'a Cell<u32>,
    admitted: &'a Cell<u32>,
    closes: &'a Cell<u8>,
}
impl CommonEventPort for Port<'_> {
    type Token = u32;
    fn pending(&self) -> Result<Option<(u32, ControlEvent)>, SourceFailure> {
        Ok(Some((
            self.revision.get(),
            ControlEvent::PropertyChanged {
                module: 4,
                key: 1,
                revision: self.revision.get(),
            },
        )))
    }
    fn authorized(&self, token: u32) -> bool {
        token == self.revision.get()
    }
    fn must_end_session(&self) -> bool {
        false
    }
    fn admitted(&mut self, token: u32) -> Result<(), SourceFailure> {
        self.admitted.set(token);
        Ok(())
    }
    fn connection_closed(&mut self) -> Result<(), SourceFailure> {
        self.closes.set(self.closes.get() + 1);
        Ok(())
    }
}
#[test]
fn test_should_reject_stale_real_generation_and_acknowledge_only_whole_event() {
    let revision = Cell::new(2);
    let admitted = Cell::new(0);
    let closes = Cell::new(0);
    let mut source = CommonEvents::new(Port {
        revision: &revision,
        admitted: &admitted,
        closes: &closes,
    });
    assert_eq!(source.route().expect("route").expect("ready").opcode, 0x90);
    let mut out = [0; 16];
    let offer = source.prepare(&mut out).expect("prepare");
    assert_eq!(offer.length, 6);
    assert_eq!(&out[..6], &[1, 0, 2, 0, 0, 0]);
    assert_eq!(admitted.get(), 0);
    revision.set(3);
    assert!(!source.authorized(offer.token));
    assert_eq!(source.admitted(offer.token), Err(SourceFailure::Denied));
    assert_eq!(admitted.get(), 0);
    source.connection_closed().expect("close");
    assert!(source.must_end_session());
    assert!(source.route().is_err());
    source.connection_closed().expect("idempotent close");
    assert_eq!(closes.get(), 1);
}
#[test]
fn test_should_never_publish_unvalidated_or_short_event_and_not_drop_actual_port_event() {
    let revision = Cell::new(2);
    let admitted = Cell::new(0);
    let closes = Cell::new(0);
    let mut source = CommonEvents::new(Port {
        revision: &revision,
        admitted: &admitted,
        closes: &closes,
    });
    source.route().expect("route");
    assert!(source.prepare(&mut [0; 5]).is_err());
    assert_eq!(admitted.get(), 0);
    source.route().expect("retry route");
    let offer = source.prepare(&mut [0; 6]).expect("full");
    source.admitted(offer.token).expect("whole");
    assert_eq!(admitted.get(), 2);
}
