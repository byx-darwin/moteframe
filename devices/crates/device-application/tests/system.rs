//! Device-control acceptance and actual execution have distinct admission boundaries.
#![cfg(feature = "system")]
#![allow(clippy::unwrap_used)]
use core::cell::Cell;
use device_actions::{Action, ActionError, SystemBackend, SystemScope};
use device_application::{SystemAccess, SystemFailure, SystemHandler, SystemPort};
use device_protocol::{Frame, Header, Kind, ModuleEntry};
use device_services::{ServiceHandler, SessionError};
#[derive(Debug)]
struct Access(Cell<bool>);
impl SystemAccess for Access {
    fn allowed(&self, _: u8) -> bool {
        self.0.get()
    }
}
#[derive(Debug, Default)]
struct Backend {
    scheduled: std::rc::Rc<Cell<u32>>,
    names: u32,
    closed: u32,
}
impl SystemBackend for Backend {
    fn schedule(&mut self, _: bool, _: u32) -> Result<(), ActionError> {
        self.scheduled.set(self.scheduled.get() + 1);
        Ok(())
    }
}
impl SystemPort for Backend {
    fn available(&self, _: Action) -> Result<(), SystemFailure> {
        Ok(())
    }
    fn restore_default_name(&mut self, access: &dyn SystemScope) -> Result<(), SystemFailure> {
        if !access.allowed() {
            return Err(SystemFailure::Denied);
        }
        self.names += 1;
        Ok(())
    }
    fn connection_closed(&mut self) -> Result<(), SystemFailure> {
        self.closed += 1;
        Ok(())
    }
}
const ENTRY: ModuleEntry = ModuleEntry {
    module: 2,
    major: 1,
    minor: 0,
    availability: 1,
    features: 0,
    input_max: 128,
    output_max: 128,
};
fn req(op: u8, id: u16, payload: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module: 2,
            opcode: op,
            message_id: id,
            offset: 0,
            total_length: u16::try_from(payload.len()).unwrap(),
        },
        payload,
    }
}
#[test]
fn test_should_execute_system_action_once_only_after_the_actual_response_was_admitted() {
    let access = Access(Cell::new(true));
    let mut backend = Backend::default();
    let scheduled = std::rc::Rc::clone(&backend.scheduled);
    let mut handler = SystemHandler::new(ENTRY, &access, &mut backend).unwrap();
    let mut out = [0; 128];
    let r = req(0x30, 2, &[3, 0, 0, 0]);
    assert_eq!(handler.dispatch(r, &mut out), Ok(4));
    assert_eq!(&out[..4], &[0; 4]);
    assert_eq!(scheduled.get(), 0);
    assert_eq!(
        handler.response_admitted(r, &[1, 0, 0, 0]),
        Err(SessionError::InvalidHandlerResponse)
    );
    assert_eq!(handler.response_admitted(r, &[0; 4]), Ok(()));
    assert_eq!(handler.response_admitted(r, &[0; 4]), Ok(()));
    assert_eq!(backend.scheduled.get(), 1);
}
#[test]
fn test_should_discard_unadmitted_action_and_fail_closed_on_withdrawn_scope() {
    let access = Access(Cell::new(true));
    let mut backend = Backend::default();
    let mut handler = SystemHandler::new(ENTRY, &access, &mut backend).unwrap();
    let mut out = [0; 128];
    let r = req(0x31, 2, &[1, 0, 0, 0]);
    assert_eq!(handler.dispatch(r, &mut out), Ok(4));
    access.0.set(false);
    assert_eq!(
        handler.response_admitted(r, &[0; 4]),
        Err(SessionError::AuthorizationRevoked)
    );
    assert_eq!(handler.connection_closed(), Ok(()));
    assert_eq!(handler.connection_closed(), Ok(()));
    assert_eq!((backend.scheduled.get(), backend.closed), (0, 1));
}
#[test]
fn test_should_commit_trusted_default_name_only_after_schema_scope_and_capacity_checks() {
    let access = Access(Cell::new(true));
    let mut backend = Backend::default();
    let mut handler = SystemHandler::new(ENTRY, &access, &mut backend).unwrap();
    let r = req(0x32, 2, &[]);
    let mut out = [0; 128];
    assert_eq!(
        handler.dispatch(r, &mut [0; 3]),
        Err(SessionError::Capacity)
    );
    assert_eq!(handler.dispatch(req(0x32, 2, &[0]), &mut out), Ok(4));
    assert_eq!(out[0], 1);
    assert_eq!(handler.dispatch(r, &mut out), Ok(4));
    assert_eq!(out[0], 0);
    assert_eq!(backend.names, 1);
}
