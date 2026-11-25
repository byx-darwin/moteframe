//! Actual property dispatch against an in-memory, test-only atomic backend.
#![allow(clippy::unwrap_used)]

use core::cell::Cell;
use device_protocol::{Frame, Header, Kind};
use device_services::{
    PropertyAccess, PropertyBackend, PropertyDescriptor, PropertyDispatchError, PropertyFailure,
    PropertyHandler, PropertyOperation, PropertyService, PropertyValue, RevisionChange,
};

const CATALOGUE: [PropertyDescriptor; 3] = [
    PropertyDescriptor {
        key: 10,
        access: 3,
        schema: 1,
        value_max: 1,
    },
    PropertyDescriptor {
        key: 20,
        access: 1,
        schema: 1,
        value_max: 1,
    },
    PropertyDescriptor {
        key: 30,
        access: 3,
        schema: 1,
        value_max: 1,
    },
];

#[derive(Debug)]
struct Access(Cell<bool>);
impl PropertyAccess for Access {
    fn allowed(&self, _module: u8, _operation: PropertyOperation, _key: Option<u16>) -> bool {
        self.0.get()
    }
}

// This in-memory backend exists only in tests. No production property field or
// persistence claim is introduced by it.
#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)]
struct Backend<'a> {
    access: &'a Access,
    value: u8,
    revision: u32,
    reads: Cell<u32>,
    writes: u32,
    revoke_read: bool,
    revoke_validate: bool,
    deny_commit: bool,
    storage_fail: bool,
    directory: &'a [PropertyDescriptor],
}
impl<'a> Backend<'a> {
    fn new(access: &'a Access) -> Self {
        Self {
            access,
            value: 1,
            revision: 7,
            reads: Cell::new(0),
            writes: 0,
            revoke_read: false,
            revoke_validate: false,
            deny_commit: false,
            storage_fail: false,
            directory: &CATALOGUE,
        }
    }
}
impl PropertyBackend for Backend<'_> {
    fn catalogue(&self) -> &[PropertyDescriptor] {
        self.directory
    }
    fn read(&self, _key: u16, out: &mut [u8]) -> Result<PropertyValue, PropertyFailure> {
        self.reads.set(self.reads.get() + 1);
        if self.storage_fail {
            return Err(PropertyFailure::StorageFailure);
        }
        out[0] = self.value;
        if self.revoke_read {
            self.access.0.set(false);
        }
        Ok(PropertyValue {
            revision: self.revision,
            length: 1,
        })
    }
    fn validate_value(&self, _key: u16, value: &[u8]) -> Result<(), PropertyFailure> {
        if self.revoke_validate {
            self.access.0.set(false);
        }
        if value.len() == 1 && value[0] <= 1 {
            Ok(())
        } else {
            Err(PropertyFailure::InvalidValue)
        }
    }
    fn commit_atomic(
        &mut self,
        module: u8,
        key: u16,
        expected: u32,
        value: &[u8],
        access: &dyn PropertyAccess,
    ) -> Result<RevisionChange, PropertyFailure> {
        // Authorization/schema/CAS and the mutation share this exclusive call.
        if self.deny_commit || !access.allowed(module, PropertyOperation::Set, Some(key)) {
            return Err(PropertyFailure::AccessDenied);
        }
        self.validate_value(key, value)?;
        if expected != 0 && expected != self.revision {
            return Err(PropertyFailure::Conflict);
        }
        let next = self
            .revision
            .checked_add(1)
            .ok_or(PropertyFailure::RevisionExhausted)?;
        if self.storage_fail {
            return Err(PropertyFailure::StorageFailure);
        }
        let previous = self.revision;
        self.value = value[0];
        self.revision = next;
        self.writes += 1;
        Ok(RevisionChange {
            previous,
            current: next,
        })
    }
}

fn frame(opcode: u8, payload: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module: 4,
            opcode,
            message_id: 1,
            offset: 0,
            total_length: u16::try_from(payload.len()).unwrap(),
        },
        payload,
    }
}
fn set(expected: u32, value: u8) -> [u8; 9] {
    let bytes = expected.to_le_bytes();
    [10, 0, bytes[0], bytes[1], bytes[2], bytes[3], 1, 0, value]
}

#[test]
fn get_returns_exact_revision_key_length_and_value() {
    let access = Access(Cell::new(true));
    let mut backend = Backend::new(&access);
    let mut service = PropertyService::new(4, &mut backend, 128).unwrap();
    let mut out = [0xaa; 128];
    let n = service
        .dispatch(frame(0x10, &[10, 0]), &access, &mut out)
        .unwrap();
    assert_eq!(&out[..n], &[0, 0, 0, 0, 7, 0, 0, 0, 10, 0, 1, 0, 1]);
    assert_eq!(out[n], 0xaa);
}

#[test]
fn test_should_bound_property_staging_without_reducing_logical_capacity() {
    let access = Access(Cell::new(true));
    let mut backend = Backend::new(&access);
    let mut service = PropertyService::new(4, &mut backend, 128).unwrap();
    let request = frame(0x10, &[10, 0]);
    let mut out = [0xaa; 128];
    assert_eq!(
        service.dispatch_bounded::<64>(request, &access, &mut out),
        Err(PropertyDispatchError::Capacity)
    );
    assert_eq!(out, [0xaa; 128]);
    let n = service
        .dispatch_bounded::<128>(request, &access, &mut out)
        .unwrap();
    assert_eq!(&out[..n], &[0, 0, 0, 0, 7, 0, 0, 0, 10, 0, 1, 0, 1]);
    assert_eq!(backend.reads.get(), 1);
}

#[test]
fn set_validates_schema_and_commits_cas_before_success() {
    let access = Access(Cell::new(true));
    let mut backend = Backend::new(&access);
    {
        let mut service = PropertyService::new(4, &mut backend, 128).unwrap();
        let mut out = [0xaa; 128];
        let n = service
            .dispatch(frame(0x11, &set(7, 0)), &access, &mut out)
            .unwrap();
        assert_eq!(&out[..n], &[0, 0, 0, 0, 8, 0, 0, 0]);
        assert_eq!(out[n], 0xaa);
    }
    assert_eq!((backend.value, backend.revision, backend.writes), (0, 8, 1));
}

#[test]
fn blind_set_is_passed_to_atomic_backend_without_a_fake_revision() {
    let access = Access(Cell::new(true));
    let mut backend = Backend::new(&access);
    let mut service = PropertyService::new(4, &mut backend, 128).unwrap();
    let mut out = [0; 128];
    let n = service
        .dispatch(frame(0x11, &set(0, 0)), &access, &mut out)
        .unwrap();
    assert_eq!(&out[..n], &[0, 0, 0, 0, 8, 0, 0, 0]);
}

#[test]
fn bad_schema_stale_revision_and_storage_failure_leave_old_value() {
    for (body, storage_fail, expected) in [
        (set(7, 2), false, [1, 0, 0, 0]),
        (set(6, 0), false, [11, 0, 0, 0]),
        (set(7, 0), true, [9, 0, 0, 0]),
    ] {
        let access = Access(Cell::new(true));
        let mut backend = Backend::new(&access);
        backend.storage_fail = storage_fail;
        {
            let mut service = PropertyService::new(4, &mut backend, 128).unwrap();
            let mut out = [0xaa; 128];
            let n = service
                .dispatch(frame(0x11, &body), &access, &mut out)
                .unwrap();
            assert_eq!(&out[..n], &expected);
            assert_eq!(out[n], 0xaa);
        }
        assert_eq!((backend.value, backend.revision, backend.writes), (1, 7, 0));
    }
}

#[test]
fn readonly_and_unknown_keys_never_reach_commit() {
    let access = Access(Cell::new(true));
    let mut backend = Backend::new(&access);
    {
        let mut service = PropertyService::new(4, &mut backend, 128).unwrap();
        let mut out = [0xaa; 128];
        let mut body = set(7, 0);
        body[0] = 20;
        let n = service
            .dispatch(frame(0x11, &body), &access, &mut out)
            .unwrap();
        assert_eq!(&out[..n], &[5, 0, 3, 255]);
        body[0] = 40;
        let n = service
            .dispatch(frame(0x11, &body), &access, &mut out)
            .unwrap();
        assert_eq!(&out[..n], &[8, 0, 0, 0]);
    }
    assert_eq!(backend.writes, 0);
}

#[test]
fn describe_cannot_reveal_an_unauthorized_next_key() {
    struct RestrictedScope;
    impl PropertyAccess for RestrictedScope {
        fn allowed(&self, _module: u8, _operation: PropertyOperation, key: Option<u16>) -> bool {
            key != Some(20)
        }
    }
    let access = Access(Cell::new(true));
    let mut backend = Backend::new(&access);
    let mut service = PropertyService::new(4, &mut backend, 128).unwrap();
    let mut out = [0xaa; 128];
    let n = service
        .dispatch(frame(0x12, &[0, 0, 1]), &RestrictedScope, &mut out)
        .unwrap();
    assert_eq!(&out[..n], &[5, 0, 3, 255]);
    assert_eq!(out[n], 0xaa);
}

#[test]
fn invalid_directory_and_limit_are_rejected_before_service_creation() {
    let access = Access(Cell::new(true));
    let mut backend = Backend::new(&access);
    for limit in [0, 127, 513] {
        assert!(PropertyService::new(4, &mut backend, limit).is_err());
    }
    for field in 0..4 {
        let mut bad = CATALOGUE;
        match field {
            0 => bad[0].key = 20,
            1 => bad[0].key = u16::MAX,
            2 => bad[0].access = 16,
            _ => bad[0].value_max = 513,
        }
        let mut backend = Backend::new(&access);
        backend.directory = &bad;
        assert!(PropertyService::new(4, &mut backend, 128).is_err());
    }
}

#[test]
fn object_directory_entry_cannot_be_used_as_scalar_get_or_set() {
    let access = Access(Cell::new(true));
    let mut backend = Backend::new(&access);
    let mut objects = CATALOGUE;
    objects[0].access = 7;
    backend.directory = &objects;
    {
        let mut service = PropertyService::new(4, &mut backend, 128).unwrap();
        let mut out = [0xaa; 128];
        for (opcode, body) in [(0x10, &[10, 0][..]), (0x11, &set(7, 0)[..])] {
            let n = service
                .dispatch(frame(opcode, body), &access, &mut out)
                .unwrap();
            assert_eq!(&out[..n], &[1, 0, 0, 0]);
        }
    }
    assert_eq!(backend.reads.get(), 0);
    assert_eq!(backend.writes, 0);
}

#[test]
fn describe_uses_inclusive_sparse_keys_first_omitted_next_and_exact_count() {
    let access = Access(Cell::new(true));
    let mut backend = Backend::new(&access);
    let mut service = PropertyService::new(4, &mut backend, 128).unwrap();
    let mut out = [0xaa; 128];
    let n = service
        .dispatch(frame(0x12, &[0, 0, 2]), &access, &mut out)
        .unwrap();
    assert_eq!(
        &out[..n],
        &[
            0, 0, 0, 0, 30, 0, 2, 10, 0, 3, 1, 0, 1, 0, 20, 0, 1, 1, 0, 1, 0
        ]
    );
    assert_eq!(out[n], 0xaa);
    for start in [21, 30] {
        let n = service
            .dispatch(frame(0x12, &[start, 0, 2]), &access, &mut out)
            .unwrap();
        assert_eq!(&out[..n], &[0, 0, 0, 0, 255, 255, 1, 30, 0, 3, 1, 0, 1, 0]);
    }
    let n = service
        .dispatch(frame(0x12, &[31, 0, 2]), &access, &mut out)
        .unwrap();
    assert_eq!(&out[..n], &[0, 0, 0, 0, 255, 255, 0]);
}

#[test]
fn describe_respects_capacity_and_rejects_sentinel_zero_or_overlarge_pages() {
    let access = Access(Cell::new(true));
    let mut backend = Backend::new(&access);
    let mut service = PropertyService::new(4, &mut backend, 128).unwrap();
    let mut out = [0; 128];
    for body in [[255, 255, 1], [0, 0, 0], [0, 0, 17]] {
        let n = service
            .dispatch(frame(0x12, &body), &access, &mut out)
            .unwrap();
        assert_eq!(&out[..n], &[1, 0, 0, 0]);
    }
    let n = service
        .dispatch(frame(0x12, &[0, 0, 16]), &access, &mut out[..13])
        .unwrap();
    assert_eq!(&out[..n], &[7, 0, 0, 0]);
    let n = service
        .dispatch(frame(0x12, &[0, 0, 16]), &access, &mut out[..14])
        .unwrap();
    assert_eq!(&out[..n], &[0, 0, 0, 0, 20, 0, 1, 10, 0, 3, 1, 0, 1, 0]);
}

#[test]
fn unauthorized_requests_reveal_neither_unknown_keys_nor_catalogue() {
    let access = Access(Cell::new(false));
    let mut backend = Backend::new(&access);
    {
        let mut service = PropertyService::new(4, &mut backend, 128).unwrap();
        let mut out = [0xaa; 128];
        for (opcode, body) in [
            (0x10, &[40, 0][..]),
            (0x12, &[0, 0, 16][..]),
            (0x11, &set(7, 0)[..]),
        ] {
            let n = service
                .dispatch(frame(opcode, body), &access, &mut out)
                .unwrap();
            assert_eq!(&out[..n], &[5, 0, 3, 255]);
            assert_eq!(out[n], 0xaa);
        }
    }
    assert_eq!(backend.reads.get(), 0);
    assert_eq!(backend.writes, 0);
}

#[test]
fn revoked_read_or_precommit_access_clears_pending_output_and_requires_close() {
    for revoke_read in [true, false] {
        let access = Access(Cell::new(true));
        let mut backend = Backend::new(&access);
        backend.revoke_read = revoke_read;
        backend.revoke_validate = !revoke_read;
        {
            let mut service = PropertyService::new(4, &mut backend, 128).unwrap();
            let mut out = [0xaa; 128];
            let body = set(7, 0);
            let request = if revoke_read {
                frame(0x10, &[10, 0])
            } else {
                frame(0x11, &body)
            };
            assert_eq!(
                service.dispatch(request, &access, &mut out),
                Err(PropertyDispatchError::AccessRevoked)
            );
            assert_eq!(out, [0; 128]);
        }
        assert_eq!(backend.writes, 0);
    }
}

#[test]
fn backend_commit_denial_and_revision_exhaustion_are_not_success() {
    for exhausted in [false, true] {
        let access = Access(Cell::new(true));
        let mut backend = Backend::new(&access);
        backend.deny_commit = !exhausted;
        if exhausted {
            backend.revision = u32::MAX;
        }
        {
            let mut service = PropertyService::new(4, &mut backend, 128).unwrap();
            let mut out = [0xaa; 128];
            let body = set(if exhausted { u32::MAX } else { 7 }, 0);
            let n = service
                .dispatch(frame(0x11, &body), &access, &mut out)
                .unwrap();
            assert_eq!(
                &out[..n],
                if exhausted {
                    &[7, 0, 0, 0]
                } else {
                    &[5, 0, 3, 255]
                }
            );
        }
        assert_eq!(backend.writes, 0);
    }
}

#[test]
fn bad_frame_or_reply_capacity_cannot_mutate() {
    let access = Access(Cell::new(true));
    let mut backend = Backend::new(&access);
    {
        let mut service = PropertyService::new(4, &mut backend, 128).unwrap();
        let mut out = [0xaa; 128];
        let body = set(7, 0);
        let mut invalid = frame(0x11, &body);
        invalid.header.offset = 1;
        assert_eq!(
            service.dispatch(invalid, &access, &mut out),
            Err(PropertyDispatchError::InvalidFrame)
        );
        assert_eq!(out, [0xaa; 128]);
        assert_eq!(
            service.dispatch(frame(0x11, &body), &access, &mut out[..3]),
            Err(PropertyDispatchError::Capacity)
        );
        let n = service
            .dispatch(frame(0x11, &body), &access, &mut out[..7])
            .unwrap();
        assert_eq!(&out[..n], &[7, 0, 0, 0]);
    }
    assert_eq!(backend.writes, 0);
}

#[test]
fn debug_never_formats_backend_value_bytes() {
    let access = Access(Cell::new(true));
    let mut backend = Backend::new(&access);
    backend.value = 99;
    let service = PropertyService::new(4, &mut backend, 128).unwrap();
    let text = format!("{service:?}");
    assert!(!text.contains("99"));
    assert!(!text.contains("Backend"));
}

#[test]
fn property_writes_route_through_session_cache_and_recheck_key_scope_on_replay() {
    use device_protocol::{ModuleEntry, ProtocolLimits};
    use device_services::{BuildProfile, Config, DeviceInfo, Session, SessionError, SessionState};
    struct KeyScope(Cell<bool>);
    impl PropertyAccess for KeyScope {
        fn allowed(&self, _module: u8, _operation: PropertyOperation, key: Option<u16>) -> bool {
            key.is_none() || self.0.get()
        }
    }
    let modules = [
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
            minor: 0,
            availability: 1,
            features: 0,
            input_max: 128,
            output_max: 128,
        },
    ];
    let mut session = Session::new(Config {
        session: 1,
        capability_rev: 1,
        profile: BuildProfile::Commercial,
        info: DeviceInfo {
            product_id: 0,
            fw_major: 0,
            fw_minor: 1,
            fw_patch: 0,
            build_id: 0,
        },
        modules: &modules,
        limits: ProtocolLimits {
            rx_frame_max: 20,
            tx_frame_max: 20,
            control_max: 128,
            data_max: 0,
            idle_ms: 2000,
            fragment_ms: 100,
            response_ms: 10000,
        },
    })
    .unwrap();
    session.control_notifications_enabled().unwrap();
    let mut hello = frame(1, &[1, 1, 128, 0, 0, 0, 3, 0]);
    hello.header.module = 0;
    session.dispatch(hello, &mut [0; 128]).unwrap();
    let access = Access(Cell::new(true));
    let mut backend = Backend::new(&access);
    let scope = KeyScope(Cell::new(true));
    {
        let service = PropertyService::new(4, &mut backend, 128).unwrap();
        let mut handler = PropertyHandler::new(service, &scope, (1, 0));
        let mut out = [0xaa; 128];
        let body = set(7, 0);
        let mut request = frame(0x11, &body);
        request.header.message_id = 2;
        for _ in 0..2 {
            let n = session
                .dispatch_with_handler(request, &mut handler, &mut out)
                .unwrap();
            assert_eq!(&out[..n], &[0, 0, 0, 0, 8, 0, 0, 0]);
        }
        scope.0.set(false);
        assert_eq!(
            session.dispatch_with_handler(request, &mut handler, &mut out),
            Err(SessionError::AuthorizationRevoked)
        );
        assert_eq!(out, [0; 128]);
        assert_eq!(session.state(), SessionState::Closed);
    }
    assert_eq!((backend.value, backend.revision, backend.writes), (0, 8, 1));
}

/// Fault-injecting full-capacity backend: writes scratch even on failed reads.
#[derive(Debug)]
struct ScratchBackend<'a> {
    access: &'a Access,
    directory: [PropertyDescriptor; 1],
    snapshot: PropertyValue,
    read_failure: Option<PropertyFailure>,
    schema_failure: Option<PropertyFailure>,
    revoke_at: u8,
    capacity: Cell<usize>,
    validations: Cell<usize>,
}
impl<'a> ScratchBackend<'a> {
    fn new(access: &'a Access, length: usize) -> Self {
        Self {
            access,
            directory: [PropertyDescriptor {
                key: 10,
                access: 1,
                schema: 1,
                value_max: 512,
            }],
            snapshot: PropertyValue {
                revision: 0x1234_5678,
                length,
            },
            read_failure: None,
            schema_failure: None,
            revoke_at: 0,
            capacity: Cell::new(0),
            validations: Cell::new(0),
        }
    }
}
impl PropertyBackend for ScratchBackend<'_> {
    fn catalogue(&self) -> &[PropertyDescriptor] {
        &self.directory
    }
    fn read(&self, _: u16, out: &mut [u8]) -> Result<PropertyValue, PropertyFailure> {
        self.capacity.set(out.len());
        out.fill(0x6d);
        if self.revoke_at == 1 {
            self.access.0.set(false);
        }
        self.read_failure.map_or(Ok(self.snapshot), Err)
    }
    fn validate_value(&self, _: u16, value: &[u8]) -> Result<(), PropertyFailure> {
        self.validations.set(self.validations.get() + 1);
        assert_eq!(value, vec![0x6d; self.snapshot.length]);
        if self.revoke_at == 2 {
            self.access.0.set(false);
        }
        self.schema_failure.map_or(Ok(()), Err)
    }
    fn commit_atomic(
        &mut self,
        _: u8,
        _: u16,
        _: u32,
        _: &[u8],
        _: &dyn PropertyAccess,
    ) -> Result<RevisionChange, PropertyFailure> {
        unreachable!("GET must not mutate")
    }
}

#[test]
fn test_should_stage_maximum_get_and_initialize_entire_success_header() {
    for length in [0, 1, 116, 500] {
        let access = Access(Cell::new(true));
        let mut backend = ScratchBackend::new(&access, length);
        let mut out = [0xa5; 520];
        let n = PropertyService::new(4, &mut backend, 512)
            .unwrap()
            .dispatch(frame(0x10, &[10, 0]), &access, &mut out)
            .unwrap();
        assert_eq!(n, 12 + length);
        assert_eq!(&out[..10], &[0, 0, 0, 0, 0x78, 0x56, 0x34, 0x12, 10, 0]);
        assert_eq!(&out[10..12], &u16::try_from(length).unwrap().to_le_bytes());
        assert_eq!(&out[12..n], vec![0x6d; length]);
        assert!(out[n..].iter().all(|&b| b == 0xa5));
        assert_eq!(
            (backend.capacity.get(), backend.validations.get()),
            (512, 1)
        );
    }
}

#[test]
fn test_should_keep_full_backend_capacity_and_validate_before_response_capacity() {
    for (logical_max, length, capacity, schema_failure, expected) in [
        (128, 116, 127, None, PropertyFailure::LimitExceeded),
        (128, 117, 128, None, PropertyFailure::LimitExceeded),
        (512, 501, 512, None, PropertyFailure::LimitExceeded),
        (512, 512, 512, None, PropertyFailure::LimitExceeded),
        (128, 512, 4, None, PropertyFailure::LimitExceeded),
        (
            128,
            512,
            4,
            Some(PropertyFailure::InvalidValue),
            PropertyFailure::IntegrityFailure,
        ),
    ] {
        let access = Access(Cell::new(true));
        let mut backend = ScratchBackend::new(&access, length);
        backend.schema_failure = schema_failure;
        let mut out = [0xa5; 512];
        let n = PropertyService::new(4, &mut backend, logical_max)
            .unwrap()
            .dispatch(frame(0x10, &[10, 0]), &access, &mut out[..capacity])
            .unwrap();
        assert_eq!(n, 4);
        assert_eq!(&out[..2], &(expected.status().0 as u16).to_le_bytes());
        assert_eq!(&out[2..4], &[0, 0]);
        assert!(out[4..].iter().all(|&b| b == 0xa5));
        assert_eq!(
            (backend.capacity.get(), backend.validations.get()),
            (512, 1)
        );
    }
}

#[test]
fn test_should_reject_malicious_metadata_and_failed_reads_without_exposing_scratch() {
    for (revision, length, value_max, read_failure, validations, expected) in [
        (0, 500, 512, None, 0, PropertyFailure::InternalFailure),
        (1, 513, 512, None, 0, PropertyFailure::InternalFailure),
        (
            1,
            usize::MAX,
            512,
            None,
            0,
            PropertyFailure::InternalFailure,
        ),
        (1, 500, 499, None, 0, PropertyFailure::InternalFailure),
        (
            0,
            usize::MAX,
            512,
            Some(PropertyFailure::StorageFailure),
            0,
            PropertyFailure::StorageFailure,
        ),
    ] {
        let access = Access(Cell::new(true));
        let mut backend = ScratchBackend::new(&access, length);
        backend.snapshot.revision = revision;
        backend.directory[0].value_max = value_max;
        backend.read_failure = read_failure;
        let mut out = [0xa5; 512];
        let n = PropertyService::new(4, &mut backend, 512)
            .unwrap()
            .dispatch(frame(0x10, &[10, 0]), &access, &mut out)
            .unwrap();
        assert_eq!(n, 4);
        assert_eq!(&out[..2], &(expected.status().0 as u16).to_le_bytes());
        assert!(out[4..].iter().all(|&b| b == 0xa5));
        assert_eq!(
            (backend.capacity.get(), backend.validations.get()),
            (512, validations)
        );
    }
}

#[test]
fn test_should_recheck_live_authority_after_read_and_schema_callbacks() {
    for (revoke_at, read_failure, schema_failure) in [
        (1, None, None),
        (1, Some(PropertyFailure::StorageFailure), None),
        (2, None, None),
        (2, None, Some(PropertyFailure::InvalidValue)),
    ] {
        let access = Access(Cell::new(true));
        let mut backend = ScratchBackend::new(&access, 500);
        backend.revoke_at = revoke_at;
        backend.read_failure = read_failure;
        backend.schema_failure = schema_failure;
        let mut out = [0xa5; 512];
        assert_eq!(
            PropertyService::new(4, &mut backend, 512)
                .unwrap()
                .dispatch(frame(0x10, &[10, 0]), &access, &mut out),
            Err(PropertyDispatchError::AccessRevoked)
        );
        assert_eq!(out, [0; 512]);
    }
}

#[test]
fn test_should_reject_bounded_staging_smaller_than_catalogue_before_reading() {
    let access = Access(Cell::new(true));
    let mut backend = ScratchBackend::new(&access, 200);
    let mut out = [0xa5; 128];
    let result = PropertyService::new(4, &mut backend, 128)
        .unwrap()
        .dispatch_bounded::<128>(frame(0x10, &[10, 0]), &access, &mut out);
    assert_eq!(result, Err(PropertyDispatchError::Capacity));
    assert_eq!(backend.capacity.get(), 0);
    assert_eq!(out, [0xa5; 128]);
}

#[test]
fn test_should_keep_large_objects_describable_and_mixed_scalars_usable() {
    const MIXED: [PropertyDescriptor; 2] = [
        CATALOGUE[0],
        PropertyDescriptor {
            key: 20,
            access: 7,
            schema: 1,
            value_max: 4096,
        },
    ];
    for bounded in [false, true] {
        let access = Access(Cell::new(true));
        let mut backend = Backend::new(&access);
        backend.directory = &MIXED;
        {
            let mut service = PropertyService::new(4, &mut backend, 128).unwrap();
            let mut out = [0xaa; 128];
            let mut dispatch = |opcode, body: &[u8], out: &mut [u8]| {
                if bounded {
                    service.dispatch_bounded::<128>(frame(opcode, body), &access, out)
                } else {
                    service.dispatch(frame(opcode, body), &access, out)
                }
            };
            assert_eq!(dispatch(0x12, &[0, 0, 16], &mut out), Ok(21));
            assert_eq!(&out[14..21], &[20, 0, 7, 1, 0, 0, 16]);
            assert_eq!(dispatch(0x10, &[20, 0], &mut out), Ok(4));
            assert_eq!(&out[..4], &[1, 0, 0, 0]);
            let mut object_set = set(0, 1);
            object_set[0] = 20;
            assert_eq!(dispatch(0x11, &object_set, &mut out), Ok(4));
            assert_eq!(&out[..4], &[1, 0, 0, 0]);
            assert_eq!(dispatch(0x10, &[10, 0], &mut out), Ok(13));
            assert_eq!(dispatch(0x11, &set(7, 0), &mut out), Ok(8));
        }
        assert_eq!((backend.reads.get(), backend.writes), (1, 1));
    }
}
