//! Existing module01 request/response contract through the durable adapter.
mod common;
use common::{Access, Nor, Source, must, store};
use core::cell::Cell;
use device_protocol::{Frame, Header, Kind, ModuleEntry};
use device_services::{ServiceHandler, SessionError, TimeAccess, UtcMapping};
use device_time_store::{DurableTimeHandler, Error, TimeStore};

fn entry() -> ModuleEntry {
    ModuleEntry {
        module: 1,
        major: 1,
        minor: 0,
        availability: 1,
        features: 0,
        input_max: 12,
        output_max: 25,
    }
}
fn request(opcode: u8, payload: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module: 1,
            opcode,
            message_id: 1,
            offset: 0,
            total_length: must(u16::try_from(payload.len())),
        },
        payload,
    }
}
fn payload(utc: i64, uncertainty: u32) -> [u8; 12] {
    let mut bytes = [0; 12];
    bytes[..8].copy_from_slice(&utc.to_le_bytes());
    bytes[8..].copy_from_slice(&uncertainty.to_le_bytes());
    bytes
}
struct CallbackAccess<F: Fn(bool) -> bool>(F);
impl<F: Fn(bool) -> bool> TimeAccess for CallbackAccess<F> {
    fn allowed(&self, set: bool) -> bool {
        (self.0)(set)
    }
}

fn assert_revocation_after_dispatch_entry(opcode: u8) {
    let media = Nor::blank();
    let source = Source::new(0);
    let owner = must(TimeStore::open(store(media.clone()), source.clone(), 0));
    must(owner.synchronize(100, 1, &Access::all()));
    let before = media.0.borrow().bytes;
    media.heal();
    source.at(100);
    let calls = Cell::new(0);
    let access = CallbackAccess(|_| {
        let call = calls.get() + 1;
        calls.set(call);
        call != 2
    });
    let mut handler = must(DurableTimeHandler::new(entry(), &owner, &access));
    let set_payload = payload(200, 2);
    let bytes = if opcode == 2 { &set_payload[..] } else { &[] };
    let mut out = [0xa5; 64];
    assert_eq!(
        handler.dispatch(request(opcode, bytes), &mut out),
        Err(SessionError::AuthorizationRevoked)
    );
    assert_eq!(out, [0; 64]);
    assert_eq!(must(owner.revision()), 1);
    let media = media.0.borrow();
    assert_eq!(media.bytes, before);
    assert_eq!((media.effects, media.syncs), (0, 0));
}

#[test]
fn test_should_revoke_get_and_wipe_output_when_scope_changes_after_dispatch_entry() {
    assert_revocation_after_dispatch_entry(1);
}

#[test]
fn test_should_revoke_set_and_wipe_output_when_scope_changes_after_dispatch_entry() {
    assert_revocation_after_dispatch_entry(2);
}

#[test]
fn test_should_preserve_ordinary_denial_when_dispatch_entry_scope_is_absent() {
    for opcode in [1, 2] {
        let owner = must(TimeStore::open(store(Nor::blank()), Source::new(0), 0));
        let access = CallbackAccess(|_| false);
        let mut handler = must(DurableTimeHandler::new(entry(), &owner, &access));
        let set_payload = payload(200, 2);
        let bytes = if opcode == 2 { &set_payload[..] } else { &[] };
        let mut out = [0xa5; 64];
        assert_eq!(handler.dispatch(request(opcode, bytes), &mut out), Ok(4));
        assert_eq!(&out[..4], &[5, 0, 3, 255]);
        assert_eq!(&out[4..], &[0xa5; 60]);
        assert_eq!(must(owner.revision()), 0);
    }
}

#[test]
fn test_should_preserve_exact_wire_get_set_and_actual_source_time() {
    let source = Source::new(123);
    let owner = must(TimeStore::open(store(Nor::blank()), source.clone(), 0));
    let access = Access::all();
    let mut handler = must(DurableTimeHandler::new(entry(), &owner, &access));
    let mut out = [0xa5; 64];
    assert_eq!(handler.dispatch(request(1, &[]), &mut out), Ok(25));
    assert_eq!(&out[..4], &[0; 4]);
    let mapping = must(UtcMapping::decode(&out[4..25]));
    assert_eq!(
        (mapping.utc_ms, mapping.mono_us, mapping.valid),
        (0, 123, false)
    );
    for utc in [0, -1_000, 1_000] {
        assert_eq!(
            handler.dispatch(request(2, &payload(utc, 7)), &mut out),
            Ok(25)
        );
        assert_eq!(&out[..4], &[0; 4]);
        let mapping = must(UtcMapping::decode(&out[4..25]));
        assert_eq!(
            (
                mapping.utc_ms,
                mapping.mono_us,
                mapping.uncertainty_ms,
                mapping.valid
            ),
            (utc, 123, 7, true)
        );
    }
    source.at(1_123);
    must(handler.dispatch(request(1, &[]), &mut out));
    assert_eq!(must(UtcMapping::decode(&out[4..25])).utc_ms, 1_001);
}

#[test]
fn test_should_reject_metadata_frames_and_capacity_before_mutation() {
    let media = Nor::blank();
    let source = Source::new(100);
    let owner = must(TimeStore::open(store(media.clone()), source.clone(), 0));
    let access = Access::all();
    for field in 0..9 {
        let mut metadata = entry();
        match field {
            0 => metadata.module = 2,
            1 => metadata.major = 2,
            2 => metadata.minor = 1,
            3 => metadata.availability = 0,
            4 => metadata.features = 1,
            5 => metadata.input_max = 11,
            6 => metadata.output_max = 24,
            7 => metadata.input_max = 513,
            _ => metadata.output_max = 513,
        }
        assert!(DurableTimeHandler::new(metadata, &owner, &access).is_err());
    }
    let mut handler = must(DurableTimeHandler::new(entry(), &owner, &access));
    assert_eq!(
        handler.dispatch(request(2, &payload(1, 0)), &mut [0; 24]),
        Err(SessionError::Capacity)
    );
    source.at(99);
    assert!(!must(owner.read(&access)).valid);
    let mut out = [0; 25];
    assert_eq!(handler.dispatch(request(2, &[0; 13]), &mut out), Ok(4));
    assert_eq!(out[0], 1);
    for field in 0..5 {
        let mut invalid = request(1, &[]);
        match field {
            0 => invalid.header.message_id = 0,
            1 => invalid.header.kind = Kind::Response,
            2 => invalid.header.module = 2,
            3 => invalid.header.offset = 1,
            _ => invalid.header.total_length = 1,
        }
        assert_eq!(
            handler.dispatch(invalid, &mut out),
            Err(SessionError::InvalidHandlerResponse)
        );
    }
    assert_eq!(handler.dispatch(request(3, &[]), &mut out), Ok(4));
    assert_eq!(out[0], 3);
    assert_eq!(must(owner.revision()), 0);
    assert_eq!(media.0.borrow().bytes, [255; 512]);
}

#[test]
fn test_should_keep_set_and_read_scopes_independent() {
    let owner = must(TimeStore::open(store(Nor::blank()), Source::new(0), 0));
    let access = Access::all();
    access.read.set(false);
    let mut handler = must(DurableTimeHandler::new(entry(), &owner, &access));
    let mut out = [0; 25];
    assert_eq!(handler.dispatch(request(1, &[]), &mut out), Ok(4));
    assert_eq!(&out[..4], &[5, 0, 3, 255]);
    assert_eq!(
        handler.dispatch(request(2, &payload(0, 0)), &mut out),
        Ok(25)
    );
    access.set.set(false);
    access.read.set(true);
    assert_eq!(
        handler.dispatch(request(2, &payload(10, 0)), &mut out),
        Ok(4)
    );
    assert_eq!(handler.dispatch(request(1, &[]), &mut out), Ok(25));
    assert_eq!(must(owner.revision()), 1);
}

#[test]
fn test_should_hold_owner_borrow_during_every_handler_access_callback() {
    let owner = must(TimeStore::open(store(Nor::blank()), Source::new(0), 0));
    let all_busy = Cell::new(true);
    let access = CallbackAccess(|_| {
        all_busy
            .set(all_busy.get() && owner.synchronize(99, 0, &Access::all()) == Err(Error::Busy));
        true
    });
    let mut handler = must(DurableTimeHandler::new(entry(), &owner, &access));
    assert!(handler.authorized(request(1, &[])));
    must(handler.dispatch(request(2, &payload(0, 0)), &mut [0; 25]));
    assert!(
        all_busy.get(),
        "An access callback escaped the owner borrow"
    );
    assert_eq!(must(owner.revision()), 1);
}

#[test]
fn test_should_wipe_revoked_response_after_committed_write_without_rolling_back() {
    let owner = must(TimeStore::open(store(Nor::blank()), Source::new(0), 0));
    let calls = Cell::new(0);
    let access = CallbackAccess(|_| {
        let now = calls.get() + 1;
        calls.set(now);
        now < 5
    });
    let mut handler = must(DurableTimeHandler::new(entry(), &owner, &access));
    let mut out = [0xa5; 25];
    assert_eq!(
        handler.dispatch(request(2, &payload(10, 2)), &mut out),
        Err(SessionError::AuthorizationRevoked)
    );
    assert_eq!(out, [0; 25]);
    assert_eq!(must(owner.revision()), 1);
    assert_eq!(must(owner.read(&Access::all())).utc_ms, 10);
}

#[test]
fn test_should_not_commit_when_scope_is_revoked_at_commit_boundary() {
    let owner = must(TimeStore::open(store(Nor::blank()), Source::new(0), 0));
    let calls = Cell::new(0);
    let access = CallbackAccess(|_| {
        let now = calls.get() + 1;
        calls.set(now);
        now < 4
    });
    let mut handler = must(DurableTimeHandler::new(entry(), &owner, &access));
    let mut out = [0xa5; 25];
    assert_eq!(
        handler.dispatch(request(2, &payload(10, 0)), &mut out),
        Err(SessionError::AuthorizationRevoked)
    );
    assert_eq!(out, [0; 25]);
    assert_eq!(must(owner.revision()), 0);
}

#[test]
fn test_should_expose_clock_and_storage_failures_without_success_payload() {
    use device_services::ClockSourceError;
    use device_storage::persistence::{Error as StorageError, Snapshot, SnapshotStore};
    struct FailingStore;
    impl SnapshotStore for FailingStore {
        fn load(&self, _out: &mut [u8]) -> Result<Option<Snapshot>, StorageError> {
            Ok(None)
        }
        fn commit(&mut self, _expected: u64, _bytes: &[u8]) -> Result<Snapshot, StorageError> {
            Err(StorageError::Io)
        }
        fn recover(&mut self) -> Result<(), StorageError> {
            Ok(())
        }
    }
    let source = Source::new(0);
    let owner = must(TimeStore::open(FailingStore, source.clone(), 0));
    let access = Access::all();
    let mut handler = must(DurableTimeHandler::new(entry(), &owner, &access));
    let mut out = [0xa5; 25];
    source.0.set(Err(ClockSourceError::Unavailable));
    assert_eq!(handler.dispatch(request(1, &[]), &mut out), Ok(4));
    assert_eq!(&out[..4], &[14, 0, 0, 0]);
    source.at(0);
    assert_eq!(
        handler.dispatch(request(2, &payload(10, 0)), &mut out),
        Ok(4)
    );
    assert_eq!(&out[..4], &[9, 0, 0, 0]);
    assert_eq!(
        owner.read(&access),
        Err(Error::Storage(StorageError::NeedsRecovery))
    );
}
