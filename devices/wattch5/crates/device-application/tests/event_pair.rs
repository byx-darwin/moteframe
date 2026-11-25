//! Independently owned producers share one pump without substituting pending-source authority.
#![allow(clippy::expect_used)]
use core::cell::Cell;
use device_application::{
    EventOffer, EventPair, EventPlane, EventRoute, EventSource, SourceFailure,
};
struct Source<'a> {
    module: u8,
    ready: &'a Cell<bool>,
    authorized: &'a Cell<bool>,
    closes: &'a Cell<u8>,
    cleanup_fails: bool,
}
impl EventSource for Source<'_> {
    type Token = u8;
    fn route(&self) -> Result<Option<EventRoute>, SourceFailure> {
        Ok(self.ready.get().then_some(EventRoute {
            module: self.module,
            opcode: 0x80,
            plane: EventPlane::Control,
        }))
    }
    fn prepare(&mut self, out: &mut [u8]) -> Result<EventOffer<u8>, SourceFailure> {
        if !self.ready.get() {
            return Err(SourceFailure::NotReady);
        }
        if out.is_empty() {
            return Err(SourceFailure::Capacity);
        }
        out[0] = self.module;
        Ok(EventOffer {
            token: self.module,
            length: 1,
            data_message: None,
        })
    }
    fn authorized(&self, token: u8) -> bool {
        self.authorized.get() && token == self.module
    }
    fn must_end_session(&self) -> bool {
        false
    }
    fn admitted(&mut self, token: u8) -> Result<(), SourceFailure> {
        assert_eq!(token, self.module);
        self.ready.set(false);
        Ok(())
    }
    fn connection_closed(&mut self) -> Result<(), SourceFailure> {
        self.closes.set(self.closes.get() + 1);
        if self.cleanup_fails {
            Err(SourceFailure::Backend)
        } else {
            Ok(())
        }
    }
}
#[test]
fn test_should_route_and_authorize_exact_child_and_rotate_only_after_admission() {
    let a = Cell::new(true);
    let b = Cell::new(true);
    let a_authorized = Cell::new(true);
    let b_authorized = Cell::new(true);
    let ca = Cell::new(0);
    let cb = Cell::new(0);
    let mut pair = EventPair::new(
        Source {
            module: 0x20,
            ready: &a,
            authorized: &a_authorized,
            closes: &ca,
            cleanup_fails: false,
        },
        Source {
            module: 0x22,
            ready: &b,
            authorized: &b_authorized,
            closes: &cb,
            cleanup_fails: false,
        },
    );
    assert_eq!(pair.route().expect("route").expect("ready").module, 0x20);
    let mut out = [0; 8];
    let first = pair.prepare(&mut out).expect("prepare");
    a_authorized.set(false);
    assert!(!pair.authorized(first.token));
    assert!(b_authorized.get());
    a_authorized.set(true);
    pair.admitted(first.token).expect("admitted");
    a.set(true);
    assert_eq!(pair.route().expect("route").expect("ready").module, 0x22);
    let second = pair.prepare(&mut out).expect("prepare");
    assert!(
        pair.admitted(first.token).is_err(),
        "stale first token cannot acknowledge the selected second source"
    );
    pair.admitted(second.token).expect("admitted");
    assert!(!b.get());
}
#[test]
fn test_should_attempt_both_cleanups_retain_failure_and_retry_only_failed_child() {
    let a = Cell::new(true);
    let b = Cell::new(true);
    let auth = Cell::new(true);
    let ca = Cell::new(0);
    let cb = Cell::new(0);
    let mut pair = EventPair::new(
        Source {
            module: 0x20,
            ready: &a,
            authorized: &auth,
            closes: &ca,
            cleanup_fails: true,
        },
        Source {
            module: 0x22,
            ready: &b,
            authorized: &auth,
            closes: &cb,
            cleanup_fails: false,
        },
    );
    assert_eq!(pair.connection_closed(), Err(SourceFailure::Backend));
    assert_eq!((ca.get(), cb.get()), (1, 1));
    assert!(pair.must_end_session());
    assert!(pair.route().is_err());
    assert_eq!(pair.connection_closed(), Err(SourceFailure::Backend));
    assert_eq!((ca.get(), cb.get()), (2, 1));
}
#[test]
fn test_should_skip_a_temporarily_unready_source_without_starving_other_module() {
    struct Waiting;
    impl EventSource for Waiting {
        type Token = ();
        fn route(&self) -> Result<Option<EventRoute>, SourceFailure> {
            Err(SourceFailure::NotReady)
        }
        fn prepare(&mut self, _: &mut [u8]) -> Result<EventOffer<()>, SourceFailure> {
            Err(SourceFailure::NotReady)
        }
        fn authorized(&self, (): ()) -> bool {
            false
        }
        fn must_end_session(&self) -> bool {
            false
        }
        fn admitted(&mut self, (): ()) -> Result<(), SourceFailure> {
            Err(SourceFailure::Invalid)
        }
        fn connection_closed(&mut self) -> Result<(), SourceFailure> {
            Ok(())
        }
    }
    let ready = Cell::new(true);
    let auth = Cell::new(true);
    let closes = Cell::new(0);
    let source = Source {
        module: 0x22,
        ready: &ready,
        authorized: &auth,
        closes: &closes,
        cleanup_fails: false,
    };
    let pair = EventPair::new(Waiting, source);
    assert_eq!(
        pair.route()
            .expect("other ready")
            .expect("ready route")
            .module,
        0x22
    );
}
#[test]
fn test_should_probe_another_source_after_selected_preparation_becomes_unready() {
    let first_ready = Cell::new(true);
    let second_ready = Cell::new(true);
    let authorized = Cell::new(true);
    let closes = Cell::new(0);
    let mut pair = EventPair::new(
        Source {
            module: 0x20,
            ready: &first_ready,
            authorized: &authorized,
            closes: &closes,
            cleanup_fails: false,
        },
        Source {
            module: 0x22,
            ready: &second_ready,
            authorized: &authorized,
            closes: &closes,
            cleanup_fails: false,
        },
    );
    assert_eq!(pair.route().expect("route").expect("first").module, 0x20);
    first_ready.set(false);
    assert_eq!(pair.prepare(&mut [0; 8]), Err(SourceFailure::NotReady));
    first_ready.set(true);
    assert_eq!(
        pair.route().expect("next route").expect("second").module,
        0x22
    );
    let offer = pair.prepare(&mut [0; 8]).expect("second ready");
    pair.admitted(offer.token).expect("second complete");
    assert!(!second_ready.get());
    assert!(first_ready.get());
}
