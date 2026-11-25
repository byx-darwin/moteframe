//! Heterogeneous handlers share one Session cache and cannot shadow one another.
use device_protocol::{Frame, Header, Kind, ModuleEntry};
use device_services::{HandlerTable, ServiceHandler, SessionError};
#[derive(Debug)]
struct Handler {
    module: u8,
    calls: u32,
    allowed: bool,
    revoke: bool,
}
impl ServiceHandler for Handler {
    fn required_features(&self, entry: &ModuleEntry, opcode: u8) -> Option<u32> {
        (entry.module == self.module && opcode == 0x30).then_some(0)
    }
    fn authorized(&self, _: Frame<'_>) -> bool {
        self.allowed
    }
    fn dispatch(&mut self, _: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        if out.len() < 4 {
            return Err(SessionError::Capacity);
        }
        self.calls += 1;
        out[..4].fill(0);
        if self.revoke {
            self.allowed = false;
        }
        Ok(4)
    }
}
fn entry(module: u8) -> ModuleEntry {
    ModuleEntry {
        module,
        major: 1,
        minor: 2,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    }
}
fn req(module: u8) -> Frame<'static> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module,
            opcode: 0x30,
            message_id: 1,
            offset: 0,
            total_length: 0,
        },
        payload: &[],
    }
}
#[test]
fn test_should_dispatch_unique_handler_and_reject_ambiguous_or_unregistered_routes() {
    let entries = [entry(2), entry(3)];
    let mut a = Handler {
        module: 2,
        calls: 0,
        allowed: true,
        revoke: false,
    };
    let mut b = Handler {
        module: 3,
        calls: 0,
        allowed: true,
        revoke: false,
    };
    {
        let mut handlers: [&mut dyn ServiceHandler; 2] = [&mut a, &mut b];
        let mut router =
            HandlerTable::new(&entries, &mut handlers).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(router.required_features(&entries[0], 0x30), Some(0));
        assert!(router.authorized(req(2)));
        assert_eq!(router.dispatch(req(2), &mut [0; 128]), Ok(4));
        assert!(!router.authorized(req(4)));
    }
    assert_eq!(a.calls, 1);
    assert_eq!(b.calls, 0);
    b.module = 2;
    {
        let mut handlers: [&mut dyn ServiceHandler; 2] = [&mut a, &mut b];
        let mut router =
            HandlerTable::new(&entries, &mut handlers).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(router.required_features(&entries[0], 0x30), None);
        assert!(router.dispatch(req(2), &mut [0; 128]).is_err());
    }
    assert_eq!(a.calls, 1);
    assert_eq!(b.calls, 0);
}
#[test]
fn test_should_bind_registered_metadata_and_clear_output_if_permission_is_revoked() {
    let entries = [entry(2)];
    let mut a = Handler {
        module: 2,
        calls: 0,
        allowed: true,
        revoke: true,
    };
    let mut handlers: [&mut dyn ServiceHandler; 1] = [&mut a];
    let mut router = HandlerTable::new(&entries, &mut handlers).unwrap_or_else(|e| panic!("{e:?}"));
    let mut different = entries[0];
    different.minor = 3;
    assert_eq!(router.required_features(&different, 0x30), None);
    let mut out = [0xa5; 128];
    assert_eq!(
        router.dispatch(req(2), &mut out),
        Err(SessionError::AuthorizationRevoked)
    );
    assert!(out.iter().all(|b| *b == 0));
}
#[derive(Debug, Default)]
struct DeferredLifecycle {
    admissions: u32,
    closes: u32,
}
impl ServiceHandler for DeferredLifecycle {
    fn response_ready(&mut self, _: Frame<'_>) -> Result<bool, SessionError> {
        Ok(false)
    }
    fn response_admitted(&mut self, _: Frame<'_>, _: &[u8]) -> Result<(), SessionError> {
        self.admissions += 1;
        Ok(())
    }
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        self.closes += 1;
        Ok(())
    }
    fn required_features(&self, e: &ModuleEntry, op: u8) -> Option<u32> {
        (e.module == 2 && op == 0x30).then_some(0)
    }
    fn authorized(&self, _: Frame<'_>) -> bool {
        true
    }
    fn dispatch(&mut self, _: Frame<'_>, _: &mut [u8]) -> Result<usize, SessionError> {
        Err(SessionError::InvalidHandlerResponse)
    }
}
#[test]
fn test_should_forward_deferred_gate_and_admission_cleanup_through_composition() {
    let entries = [entry(2)];
    let mut h = DeferredLifecycle::default();
    {
        let mut handlers: [&mut dyn ServiceHandler; 1] = [&mut h];
        let mut table =
            HandlerTable::new(&entries, &mut handlers).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(table.response_ready(req(2)), Ok(false));
        assert_eq!(table.response_admitted(req(2), &[0; 4]), Ok(()));
        assert_eq!(table.connection_closed(), Ok(()));
    }
    assert_eq!(h.admissions, 1);
    assert_eq!(h.closes, 1);
}

#[test]
fn advance_attempts_every_handler_and_retains_first_actual_failure() {
    use core::cell::Cell;
    struct Time<'a> {
        seen: &'a Cell<u64>,
        error: Option<device_services::SessionError>,
    }
    impl device_services::ServiceHandler for Time<'_> {
        fn advance(&mut self, now: u64) -> Result<(), device_services::SessionError> {
            self.seen.set(now);
            self.error.map_or(Ok(()), Err)
        }
        fn required_features(&self, _: &device_protocol::ModuleEntry, _: u8) -> Option<u32> {
            None
        }
        fn authorized(&self, _: device_protocol::Frame<'_>) -> bool {
            false
        }
        fn dispatch(
            &mut self,
            _: device_protocol::Frame<'_>,
            _: &mut [u8],
        ) -> Result<usize, device_services::SessionError> {
            Err(device_services::SessionError::InvalidHandlerResponse)
        }
    }
    let (a, b) = (Cell::new(0), Cell::new(0));
    let mut first = Time {
        seen: &a,
        error: Some(device_services::SessionError::InvalidHandlerResponse),
    };
    let mut second = Time {
        seen: &b,
        error: Some(device_services::SessionError::AuthorizationRevoked),
    };
    let mut routes: [&mut dyn device_services::ServiceHandler; 2] = [&mut first, &mut second];
    let mut table =
        device_services::HandlerTable::new(&[], &mut routes).unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        device_services::ServiceHandler::advance(&mut table, 60000),
        Err(device_services::SessionError::InvalidHandlerResponse)
    );
    assert_eq!((a.get(), b.get()), (60000, 60000));
}
