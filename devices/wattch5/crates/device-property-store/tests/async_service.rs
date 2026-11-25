//! Persistent async actor through actual Session and `ControlConnection` lifecycles.
#![allow(
    clippy::expect_used,
    reason = "Test fixtures and assertions panic on failure."
)]
mod async_support;
mod support;
use async_support::{Nor, Owner, initialized};
use core::cell::Cell;
use device_connection::{ConnectionError, ControlConnection, Receive};
use device_property_store::AsyncPropertyHandler;
use device_protocol::{Frame, Header, Kind, ModuleEntry, ProtocolLimits};
use device_services::{
    BuildProfile, Config, DeviceInfo, PropertyAccess, PropertyOperation, ServiceHandler, Session,
    SessionError, Status,
};
const MODULES: [ModuleEntry; 2] = [
    ModuleEntry {
        module: 0,
        major: 1,
        minor: 2,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    },
    ModuleEntry {
        module: 4,
        major: 1,
        minor: 2,
        availability: 1,
        features: 0,
        input_max: 128,
        output_max: 128,
    },
];
#[derive(Debug)]
struct Access(Cell<bool>);
impl PropertyAccess for Access {
    fn allowed(&self, _: u8, _: PropertyOperation, _: Option<u16>) -> bool {
        self.0.get()
    }
}
fn config(modules: &[ModuleEntry]) -> Config<'_> {
    Config {
        session: 1,
        capability_rev: 1,
        profile: BuildProfile::Commercial,
        info: DeviceInfo {
            product_id: 5,
            fw_major: 0,
            fw_minor: 1,
            fw_patch: 0,
            build_id: 1,
        },
        modules,
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
fn frame(opcode: u8, id: u16, payload: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module: 4,
            opcode,
            message_id: id,
            offset: 0,
            total_length: u16::try_from(payload.len()).expect("bounded"),
        },
        payload,
    }
}
fn set(expected: u32, value: u8) -> [u8; 9] {
    let r = expected.to_le_bytes();
    [1, 0, r[0], r[1], r[2], r[3], 1, 0, value]
}
fn actor<'a>(
    owner: &'a Owner,
    access: &'a Access,
) -> AsyncPropertyHandler<'a, 'a, 'static, Nor, async_support::Schema, 128> {
    AsyncPropertyHandler::new(owner, 4, access, (1, 2), 128, 128).expect("actor")
}
fn hello_session(session: &mut Session<'_>) {
    session
        .control_notifications_enabled()
        .expect("notifications");
    let payload = [1, 1, 128, 0, 0, 0, 3, 0];
    let mut request = frame(1, 1, &payload);
    request.header.module = 0;
    session.dispatch(request, &mut [0; 128]).expect("hello");
}
fn drive(owner: &Owner) {
    for _ in 0..2000 {
        let _ = owner.step();
    }
}
#[test]
fn test_should_wait_for_durable_background_and_replay_without_another_mutation() {
    let nor = Nor::new();
    let owner = initialized(nor.clone());
    let access = Access(Cell::new(true));
    let mut handler = actor(&owner, &access);
    let mut session = Session::new(config(&MODULES)).expect("session");
    hello_session(&mut session);
    let payload = set(1, 1);
    let request = frame(0x11, 2, &payload);
    let mut out = [0xa5; 128];
    let before = nor.actions();
    for _ in 0..8 {
        assert_eq!(
            session.dispatch_with_handler(request, &mut handler, &mut out),
            Err(SessionError::Deferred)
        );
        assert_eq!(out, [0; 128]);
    }
    assert_eq!(nor.actions(), before);
    assert_eq!(owner.revision(), Ok(1));
    drive(&owner);
    assert_eq!(handler.response_ready(request), Ok(true));
    assert_eq!(handler.response_ready(request), Ok(true));
    let n = session
        .dispatch_with_handler(request, &mut handler, &mut out)
        .expect("durable reply");
    assert_eq!(&out[..n], &[0, 0, 0, 0, 2, 0, 0, 0]);
    assert_eq!(owner.revision(), Ok(2));
    let after = nor.actions();
    for _ in 0..8 {
        assert_eq!(
            session.dispatch_with_handler(request, &mut handler, &mut out),
            Ok(8)
        );
        assert_eq!(&out[..8], &[0, 0, 0, 0, 2, 0, 0, 0]);
    }
    assert_eq!(nor.actions(), after);
    let token = owner
        .begin_set(4, 1, 2, &[0], &access)
        .expect("later UI mutation");
    async_support::settle(&owner, token).expect("UI durable");
    owner.take_result(token).expect("token").expect("result");
    assert_eq!(
        session.dispatch_with_handler(request, &mut handler, &mut out),
        Ok(8)
    );
    assert_eq!(&out[..8], &[0, 0, 0, 0, 2, 0, 0, 0]);
    let n = session
        .dispatch_with_handler(frame(0x10, 3, &[1, 0]), &mut handler, &mut out)
        .expect("GET");
    assert_eq!(&out[..n], &[0, 0, 0, 0, 3, 0, 0, 0, 1, 0, 1, 0, 0]);
}
#[test]
fn test_should_prevent_admission_for_short_configured_output_and_return_existing_failures() {
    let nor = Nor::new();
    let owner = initialized(nor.clone());
    let access = Access(Cell::new(true));
    let mut modules = MODULES;
    modules[1].output_max = 4;
    let mut handler = AsyncPropertyHandler::<_, _, 128>::new(&owner, 4, &access, (1, 2), 128, 4)
        .expect("short actor");
    let mut session = Session::new(config(&modules)).expect("session");
    hello_session(&mut session);
    let mut out = [0; 128];
    let before = nor.actions();
    let n = session
        .dispatch_with_handler(frame(0x11, 2, &set(1, 1)), &mut handler, &mut out)
        .expect("limit status");
    assert_eq!(n, 4);
    assert_eq!(&out[..2], &(Status::LimitExceeded as u16).to_le_bytes());
    assert_eq!(nor.actions(), before);
    assert_eq!(owner.revision(), Ok(1));
    assert_eq!(
        session.dispatch_with_handler(frame(0x11, 3, &[1]), &mut handler, &mut out),
        Ok(4)
    );
    assert_eq!(&out[..2], &(Status::InvalidArgument as u16).to_le_bytes());
    let mut handler = actor(&owner, &access);
    let mut session = Session::new(config(&MODULES)).expect("session");
    hello_session(&mut session);
    for (id, payload, status) in [
        (2, set(1, 2), Status::InvalidArgument),
        (3, set(9, 1), Status::Conflict),
    ] {
        assert_eq!(
            session.dispatch_with_handler(frame(0x11, id, &payload), &mut handler, &mut out),
            Ok(4)
        );
        assert_eq!(&out[..2], &(status as u16).to_le_bytes());
    }
    assert_eq!(
        session.dispatch_with_handler(frame(0x11, 4, &[1]), &mut handler, &mut out),
        Ok(4)
    );
    assert_eq!(&out[..2], &(Status::InvalidArgument as u16).to_le_bytes());
    access.0.set(false);
    assert_eq!(
        session.dispatch_with_handler(frame(0x11, 5, &set(1, 1)), &mut handler, &mut out),
        Ok(4)
    );
    assert_eq!(&out[..2], &(Status::AccessDenied as u16).to_le_bytes());
    assert_eq!(nor.actions(), before);
}
#[test]
fn test_should_reject_mismatched_declared_output_before_accepting_storage() {
    let nor = Nor::new();
    let owner = initialized(nor.clone());
    let access = Access(Cell::new(true));
    let mut modules = MODULES;
    modules[1].output_max = 4;
    let mut handler = actor(&owner, &access);
    let mut session = Session::new(config(&modules)).expect("session");
    hello_session(&mut session);
    let before = nor.actions();
    let mut out = [0; 128];
    assert_eq!(
        session.dispatch_with_handler(frame(0x11, 2, &set(1, 1)), &mut handler, &mut out),
        Ok(4)
    );
    assert_eq!(&out[..2], &(Status::UnsupportedOpcode as u16).to_le_bytes());
    assert_eq!(nor.actions(), before);
    assert_eq!(owner.revision(), Ok(1));
}
fn drain(connection: &mut ControlConnection<'_>, handler: &mut impl ServiceHandler) {
    while connection.response_pending() {
        connection
            .next_frame(handler, &mut [0; 20])
            .expect("fragment");
        connection.admit_frame(handler).expect("admit");
    }
}
fn connection(handler: &mut impl ServiceHandler) -> ControlConnection<'static> {
    let mut c = ControlConnection::new(config(&MODULES)).expect("connection");
    c.notifications_enabled().expect("notifications");
    c.receive(
        &[0x10, 0, 1, 0, 1, 0, 0, 0, 8, 0, 1, 1, 128, 0, 0, 0, 3, 0],
        0,
        handler,
    )
    .expect("hello");
    drain(&mut c, handler);
    c
}
fn send_set(connection: &mut ControlConnection<'_>, handler: &mut impl ServiceHandler) -> Receive {
    let payload = set(1, 1);
    let mut bytes = [0; 20];
    let n = frame(0x11, 2, &payload).encode(&mut bytes).expect("frame");
    connection.receive(&bytes[..n], 1, handler).expect("SET")
}
#[test]
fn test_should_retry_exact_transport_response_without_double_commit_and_suppress_revoked_send() {
    let nor = Nor::new();
    let owner = initialized(nor.clone());
    let access = Access(Cell::new(true));
    let mut handler = actor(&owner, &access);
    let mut c = connection(&mut handler);
    assert_eq!(send_set(&mut c, &mut handler), Receive::Deferred);
    assert!(!c.response_pending());
    for now in 2..9 {
        assert_eq!(c.poll_pending(now, &mut handler), Ok(Receive::Deferred));
    }
    drive(&owner);
    assert_eq!(c.poll_pending(9, &mut handler), Ok(Receive::ResponseReady));
    let before = nor.actions();
    let mut first = [0; 20];
    let n = c.next_frame(&mut handler, &mut first).expect("response");
    assert_eq!(&first[10..n], &[0, 0, 0, 0, 2, 0, 0, 0]);
    for _ in 0..8 {
        let mut again = [0; 20];
        assert_eq!(c.next_frame(&mut handler, &mut again), Ok(n));
        assert_eq!(first, again);
    }
    assert_eq!(nor.actions(), before);
    access.0.set(false);
    assert_eq!(
        c.admit_frame(&mut handler),
        Err(ConnectionError::Session(SessionError::AuthorizationRevoked))
    );
    assert!(handler.must_end_session());
    assert_eq!(owner.revision(), Ok(2));
}
#[test]
fn test_should_detach_on_close_timeout_revoke_or_drop_and_finish_accepted_work() {
    for end in 0..4 {
        let owner = initialized(Nor::new());
        let access = Access(Cell::new(true));
        let mut handler = actor(&owner, &access);
        let mut c = connection(&mut handler);
        assert_eq!(send_set(&mut c, &mut handler), Receive::Deferred);
        match end {
            0 => {
                c.close(&mut handler).expect("close");
                c.close(&mut handler).expect("idempotent close");
            }
            1 => {
                assert!(c.tick(10002, &mut handler).is_err());
            }
            2 => {
                access.0.set(false);
                assert!(c.poll_pending(2, &mut handler).is_err());
            }
            _ => {}
        }
        drop(handler);
        drive(&owner);
        assert_eq!(owner.revision(), Ok(2));
        access.0.set(true);
        let token = owner
            .begin_set(4, 1, 2, &[0], &access)
            .expect("old actor releases delivery");
        owner.detach(token).expect("cleanup");
        drive(&owner);
        assert_eq!(owner.revision(), Ok(3));
    }
}
#[test]
fn test_should_reject_changed_pending_request_and_actor_capacity_without_repreparing() {
    let nor = Nor::new();
    let owner = initialized(nor.clone());
    let access = Access(Cell::new(true));
    let mut handler = actor(&owner, &access);
    assert!(AsyncPropertyHandler::<_, _, 128>::new(&owner, 4, &access, (1, 2), 256, 128).is_err());
    assert_eq!(
        handler.response_ready(frame(0x11, 2, &set(1, 1))),
        Ok(false)
    );
    assert_eq!(
        handler.response_ready(frame(0x11, 2, &set(1, 0))),
        Err(SessionError::InvalidHandlerResponse)
    );
    drop(handler);
    drive(&owner);
    assert_eq!(owner.revision(), Ok(2));
}
#[test]
fn test_should_release_delivery_when_actor_drops_inside_a_schema_callback_holding_owner_borrow() {
    use device_property_store::{AsyncPropertyStore, DefaultValue, Error, PropertySchema};
    use device_services::{PropertyBackend, PropertyDescriptor};
    use std::{cell::RefCell, rc::Rc};
    type Hook = Rc<RefCell<Option<Box<dyn Fn()>>>>;
    struct HookSchema(Hook);
    impl PropertySchema for HookSchema {
        fn descriptor(&self, m: u8, k: u16) -> Result<PropertyDescriptor, Error> {
            async_support::Schema.descriptor(m, k)
        }
        fn validate(&self, m: u8, k: u16, value: &[u8]) -> Result<(), Error> {
            if let Some(callback) = self.0.borrow().as_ref() {
                callback();
            }
            async_support::Schema.validate(m, k, value)
        }
    }
    let hook: Hook = Rc::new(RefCell::new(None));
    let owner = Box::leak(Box::new(
        AsyncPropertyStore::open(
            async_support::engine(Nor::new()),
            async_support::CATALOGUE,
            HookSchema(hook.clone()),
        )
        .expect("owner"),
    ));
    let access = Box::leak(Box::new(Access(Cell::new(true))));
    let token = owner
        .begin_initialize(&[DefaultValue {
            module: 4,
            key: 1,
            value: &[0],
        }])
        .expect("initialize");
    async_support::settle(owner, token).expect("durable");
    owner.take_result(token).expect("token").expect("result");
    let mut handler =
        AsyncPropertyHandler::<_, _, 128>::new(owner, 4, access, (1, 2), 128, 128).expect("actor");
    assert_eq!(
        handler.response_ready(frame(0x11, 2, &set(1, 1))),
        Ok(false)
    );
    let held = Rc::new(RefCell::new(Some(handler)));
    let drop_slot = held.clone();
    *hook.borrow_mut() = Some(Box::new(move || {
        drop(drop_slot.borrow_mut().take());
    }));
    assert_eq!(
        owner
            .adapter(4)
            .expect("adapter")
            .read(1, &mut [0; 8])
            .expect("old confirmed")
            .revision,
        1
    );
    assert!(held.borrow().is_none());
    *hook.borrow_mut() = None;
    for _ in 0..2000 {
        owner.step().expect("background");
    }
    assert_eq!(owner.revision(), Ok(2));
    let token = owner
        .begin_set(4, 1, 2, &[0], access)
        .expect("no stranded terminal slot");
    owner.detach(token).expect("cleanup");
}
#[test]
fn test_should_return_busy_for_ble_while_ui_owns_the_shared_slot() {
    let nor = Nor::new();
    let owner = initialized(nor.clone());
    let access = Access(Cell::new(true));
    let ui = owner
        .begin_set(4, 1, 1, &[1], &access)
        .expect("UI admission");
    let mut handler = actor(&owner, &access);
    let mut session = Session::new(config(&MODULES)).expect("session");
    hello_session(&mut session);
    let before = nor.actions();
    let mut out = [0; 128];
    assert_eq!(
        session.dispatch_with_handler(frame(0x11, 2, &set(1, 0)), &mut handler, &mut out),
        Ok(4)
    );
    assert_eq!(&out[..2], &(Status::Busy as u16).to_le_bytes());
    assert_eq!(nor.actions(), before);
    async_support::settle(&owner, ui).expect("UI durable");
    owner.take_result(ui).expect("token").expect("result");
    assert_eq!(owner.revision(), Ok(2));
    assert_eq!(
        session.dispatch_with_handler(frame(0x11, 3, &set(1, 0)), &mut handler, &mut out),
        Ok(4)
    );
    assert_eq!(&out[..2], &(Status::Conflict as u16).to_le_bytes());
}
#[test]
fn test_should_fault_changed_session_request_and_keep_the_accepted_mutation_owned() {
    let owner = initialized(Nor::new());
    let access = Access(Cell::new(true));
    let mut handler = actor(&owner, &access);
    let mut session = Session::new(config(&MODULES)).expect("session");
    hello_session(&mut session);
    let mut out = [0; 128];
    assert_eq!(
        session.dispatch_with_handler(frame(0x11, 2, &set(1, 1)), &mut handler, &mut out),
        Err(SessionError::Deferred)
    );
    assert_eq!(
        session.dispatch_with_handler(frame(0x11, 2, &set(1, 0)), &mut handler, &mut out),
        Err(SessionError::RequestConflict)
    );
    handler
        .connection_closed()
        .expect("connection owner cleanup");
    drive(&owner);
    assert_eq!(owner.revision(), Ok(2));
    let token = owner
        .begin_set(4, 1, 2, &[0], &access)
        .expect("fresh actor can mutate");
    owner.detach(token).expect("cleanup");
}

#[test]
fn test_should_preserve_legacy_get_for_small_value_with_large_declared_schema() {
    use device_property_store::{AsyncPropertyStore, Error, ModuleCatalogue, PropertySchema};
    use device_services::PropertyDescriptor;
    struct Wide;
    const D: PropertyDescriptor = PropertyDescriptor {
        value_max: 512,
        ..async_support::D
    };
    const C: &[ModuleCatalogue<'_>] = &[ModuleCatalogue {
        module: 4,
        properties: &[D],
    }];
    impl PropertySchema for Wide {
        fn descriptor(&self, _: u8, _: u16) -> Result<PropertyDescriptor, Error> {
            Ok(D)
        }
        fn validate(&self, _: u8, _: u16, value: &[u8]) -> Result<(), Error> {
            if value == [0] {
                Ok(())
            } else {
                Err(Error::InvalidValue)
            }
        }
    }
    let owner =
        AsyncPropertyStore::open(async_support::engine(Nor::new()), C, Wide).expect("owner");
    let token = owner
        .begin_initialize(async_support::DEFAULTS)
        .expect("initialize");
    async_support::settle(&owner, token).expect("commit");
    let access = Access(Cell::new(true));
    let mut actor = AsyncPropertyHandler::<_, _, 128>::new(&owner, 4, &access, (1, 2), 128, 128)
        .expect("legacy actor");
    let mut out = [0xa5; 128];
    assert_eq!(actor.dispatch(frame(0x10, 1, &[1, 0]), &mut out), Ok(13));
    assert_eq!(&out[..13], &[0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 1, 0, 0]);
}

#[test]
fn test_should_keep_large_objects_out_of_the_scalar_persistence_owner() {
    use device_property_store::{AsyncPropertyStore, Error, ModuleCatalogue};
    use device_services::PropertyDescriptor;
    const OBJECT: PropertyDescriptor = PropertyDescriptor {
        key: 20,
        access: 7,
        schema: 1,
        value_max: 4096,
    };
    const C: &[ModuleCatalogue<'_>] = &[ModuleCatalogue {
        module: 4,
        properties: &[async_support::D, OBJECT],
    }];
    // This owner has always required access=11 scalar entries. Object values
    // have a separate owner; restoring service metadata must not expand it.
    assert!(matches!(
        AsyncPropertyStore::open(async_support::engine(Nor::new()), C, async_support::Schema),
        Err(Error::InvalidCatalogue)
    ));
}
