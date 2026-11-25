//! Non-durable test fixture: exercises contracts, never represents Flash acceptance.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use core::cell::Cell;
use device_object_service::*;
use device_protocol::{Frame, Header, Kind, ModuleEntry, ObjectBegin, ObjectRequest, object_crc};
use device_services::{ServiceHandler, SessionError};
use device_storage::{
    Authorization, Begin, Commit, ObjectMeta, ObjectSchema, ObjectStore, StoreError,
};
const BODY: &[u8] = &[0, 0, 255, 255, 1, 0, 0];
struct Auth {
    enabled: Cell<bool>,
    dangerous: Cell<bool>,
    blocked: Cell<Option<u32>>,
}
impl Auth {
    fn new() -> Self {
        Self {
            enabled: Cell::new(true),
            dangerous: Cell::new(true),
            blocked: Cell::new(None),
        }
    }
}
impl ObjectAccess for Auth {
    fn allowed(&self, _: u8, _: ObjectOperation, id: Option<u32>) -> bool {
        self.enabled.get() && (id.is_none() || id != self.blocked.get())
    }
    fn dangerous_allowed(&self, _: u8) -> bool {
        self.enabled.get() && self.dangerous.get()
    }
}
struct Schema;
impl ObjectSchema for Schema {
    fn validate(&self, module: u8, kind: u16, body: &[u8]) -> Result<(), StoreError> {
        if module == 4 && kind == 1 && body.len() == 7 && body[0] == 0 {
            Ok(())
        } else {
            Err(StoreError::Schema)
        }
    }
}
#[derive(Clone)]
struct Stored {
    meta: ObjectMeta,
    body: Vec<u8>,
    birth: Birth,
}
struct Memory<'access> {
    high: u32,
    revision: u32,
    objects: Vec<Stored>,
    tokens: Vec<[u8; 16]>,
    reservations: Vec<Begin>,
    reserves: u32,
    commits: u32,
    conflict: bool,
    invalid_page: bool,
    stat_error: Option<StoreError>,
    revoke_read: Option<&'access Auth>,
    revoke_commit: Option<&'access Auth>,
    revoke_delete: Option<&'access Auth>,
}
impl Memory<'_> {
    fn new() -> Self {
        Self {
            high: 0,
            revision: 1,
            objects: Vec::new(),
            tokens: Vec::new(),
            reservations: Vec::new(),
            reserves: 0,
            commits: 0,
            conflict: false,
            invalid_page: false,
            stat_error: None,
            revoke_read: None,
            revoke_commit: None,
            revoke_delete: None,
        }
    }
}
impl ObjectStore for Memory<'_> {
    fn reserve(&mut self, input: &Begin, access: &dyn Authorization) -> Result<(), StoreError> {
        if !access.write_allowed(input.module) {
            return Err(StoreError::Denied);
        }
        if input.object_id <= self.high || self.tokens.contains(&input.creation_token) {
            return Err(StoreError::Conflict);
        }
        self.high = input.object_id;
        self.tokens.push(input.creation_token);
        self.reservations.push(*input);
        self.reserves += 1;
        Ok(())
    }
    fn stat(&self, _: u8, id: u32) -> Result<ObjectMeta, StoreError> {
        if let Some(error) = self.stat_error {
            return Err(error);
        }
        self.objects
            .iter()
            .find(|o| o.meta.object_id == id)
            .map(|o| o.meta)
            .ok_or(StoreError::NotFound)
    }
    fn commit(
        &mut self,
        c: &Commit,
        body: &[u8],
        access: &dyn Authorization,
    ) -> Result<ObjectMeta, StoreError> {
        if let Some(auth) = self.revoke_commit {
            auth.enabled.set(false);
        }
        if !access.write_allowed(c.begin.module) {
            return Err(StoreError::Denied);
        }
        if self.conflict {
            return Err(StoreError::Conflict);
        }
        let input = c.begin;
        let (revision, birth) = if input.create {
            if !self.reservations.contains(&input)
                || self.stat(input.module, input.object_id).is_ok()
            {
                return Err(StoreError::Conflict);
            }
            (
                1,
                Birth {
                    kind: input.kind,
                    total: input.total,
                    crc: c.crc,
                    revision: 1,
                    token: input.creation_token,
                },
            )
        } else {
            let old = self
                .objects
                .iter()
                .find(|o| o.meta.object_id == input.object_id)
                .ok_or(StoreError::NotFound)?;
            if old.meta.revision != input.expected_revision || old.meta.kind != input.kind {
                return Err(StoreError::Conflict);
            }
            (
                old.meta
                    .revision
                    .checked_add(1)
                    .ok_or(StoreError::Capacity)?,
                old.birth,
            )
        };
        let meta = ObjectMeta {
            object_id: input.object_id,
            kind: input.kind,
            revision,
            total: input.total,
            crc: c.crc,
            birth_token: birth.token,
        };
        self.objects.retain(|o| o.meta.object_id != input.object_id);
        self.objects.push(Stored {
            meta,
            body: body.to_vec(),
            birth,
        });
        self.objects.sort_by_key(|o| o.meta.object_id);
        self.revision += 1;
        self.commits += 1;
        Ok(meta)
    }
}
impl ObjectRepository for Memory<'_> {
    fn read_range(
        &self,
        module: u8,
        id: u32,
        expected: u32,
        offset: u32,
        out: &mut [u8],
        access: &dyn ObjectAccess,
    ) -> Result<ReadResult, StoreError> {
        if !access.allowed(module, ObjectOperation::Read, Some(id)) {
            return Err(StoreError::Denied);
        }
        let o = self
            .objects
            .iter()
            .find(|o| o.meta.object_id == id)
            .ok_or(StoreError::NotFound)?;
        if expected != o.meta.revision {
            return Err(StoreError::Conflict);
        }
        let offset = usize::try_from(offset).unwrap();
        if offset > o.body.len() {
            return Err(StoreError::Invalid);
        }
        let n = out.len().min(o.body.len() - offset);
        out[..n].copy_from_slice(&o.body[offset..offset + n]);
        if let Some(auth) = self.revoke_read {
            auth.enabled.set(false);
        }
        Ok(ReadResult {
            meta: o.meta,
            length: n,
        })
    }
    fn delete_atomic(
        &mut self,
        module: u8,
        id: u32,
        expected: u32,
        access: &dyn ObjectAccess,
    ) -> Result<(), StoreError> {
        if let Some(auth) = self.revoke_delete {
            auth.enabled.set(false);
        }
        let operation = if id == u32::MAX {
            ObjectOperation::Clear
        } else {
            ObjectOperation::Write
        };
        if !access.allowed(module, operation, None)
            || !access.allowed(module, operation, Some(id))
            || (id == u32::MAX && !access.dangerous_allowed(module))
        {
            return Err(StoreError::Denied);
        }
        if id == u32::MAX {
            if self.revision != expected {
                return Err(StoreError::Conflict);
            }
            self.objects.clear();
        } else {
            if self.stat(module, id)?.revision != expected {
                return Err(StoreError::Conflict);
            }
            self.objects.retain(|o| o.meta.object_id != id);
        }
        self.revision += 1;
        Ok(())
    }
    fn enumerate(
        &self,
        module: u8,
        start: u32,
        out: &mut [DirectoryEntry],
        access: &dyn ObjectAccess,
    ) -> Result<Enumeration, StoreError> {
        if !access.allowed(module, ObjectOperation::Enumerate, None) {
            return Err(StoreError::Denied);
        }
        let mut count = 0;
        let mut next = u32::MAX;
        for o in self.objects.iter().filter(|o| o.meta.object_id >= start) {
            if !access.allowed(module, ObjectOperation::Enumerate, Some(o.meta.object_id)) {
                return Err(StoreError::Denied);
            }
            if count == out.len() {
                next = o.meta.object_id;
                break;
            }
            out[count] = DirectoryEntry {
                id: o.meta.object_id,
                kind: o.meta.kind,
                total: o.meta.total,
            };
            count += 1;
        }
        if self.invalid_page {
            return Ok(Enumeration {
                revision: self.revision,
                next: 1,
                count: 0,
            });
        }
        Ok(Enumeration {
            revision: self.revision,
            next,
            count,
        })
    }
    fn id_state(
        &self,
        module: u8,
        id: u32,
        access: &dyn ObjectAccess,
    ) -> Result<IdState, StoreError> {
        if !access.allowed(module, ObjectOperation::Read, Some(id)) {
            return Err(StoreError::Denied);
        }
        Ok(
            if let Some(o) = self.objects.iter().find(|o| o.meta.object_id == id) {
                IdState {
                    state: 1,
                    high_water: self.high,
                    birth: o.birth,
                }
            } else {
                IdState {
                    state: if id > self.high { 0 } else { 2 },
                    high_water: self.high,
                    birth: Birth::default(),
                }
            },
        )
    }
}
fn metadata() -> ModuleEntry {
    ModuleEntry {
        module: 4,
        major: 1,
        minor: 0,
        availability: 1,
        features: 0,
        input_max: 512,
        output_max: 512,
    }
}
fn frame(opcode: u8, body: &[u8]) -> Frame<'_> {
    Frame {
        header: Header {
            kind: Kind::Request,
            module: 4,
            opcode,
            message_id: 1,
            offset: 0,
            total_length: u16::try_from(body.len()).unwrap(),
        },
        payload: body,
    }
}
type Handler<'access, 'input> = ObjectHandler<'access, Memory<'input>, Schema, 64>;
fn handler<'access, 'input>(
    memory: &'access mut Memory<'input>,
    access: &'access Auth,
    owner: &'access ObjectTransactions<64>,
) -> Handler<'access, 'input> {
    ObjectHandler::new(metadata(), memory, &Schema, access, &[1], owner).unwrap()
}
fn run(service: &mut Handler<'_, '_>, r: ObjectRequest<'_>) -> Vec<u8> {
    let mut input = [0; 512];
    let n = r.encode(&mut input).unwrap();
    let mut out = [0xa5; 512];
    let n = service
        .dispatch(frame(r.opcode(), &input[..n]), &mut out)
        .unwrap();
    out[..n].to_vec()
}
fn begin(id: u32, token: u8) -> ObjectRequest<'static> {
    ObjectRequest::Begin(ObjectBegin {
        kind: 1,
        object_id: id,
        expected_revision: 0,
        total: 7,
        create: true,
        creation_token: [token; 16],
    })
}
fn tx(v: &[u8]) -> u32 {
    assert_eq!(&v[..4], &[0; 4]);
    u32::from_le_bytes(v[4..8].try_into().unwrap())
}
fn create(service: &mut Handler<'_, '_>, id: u32, token: u8) {
    let transaction = tx(&run(service, begin(id, token)));
    assert_eq!(
        run(
            service,
            ObjectRequest::Write {
                transaction,
                offset: 0,
                bytes: BODY
            }
        ),
        [0, 0, 0, 0, 7, 0, 0, 0]
    );
    assert_eq!(
        &run(
            service,
            ObjectRequest::Commit {
                transaction,
                crc: 0x66aa_5f61
            }
        )[..4],
        &[0; 4]
    );
}

#[test]
fn frozen_create_write_commit_info_and_read_bytes() {
    let owner = ObjectTransactions::new();
    let access = Auth::new();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &owner);
    let input = run(&mut service, begin(1, 1));
    assert_eq!(input, [0, 0, 0, 0, 1, 0, 0, 0, 64, 0]);
    assert_eq!(
        run(
            &mut service,
            ObjectRequest::Write {
                transaction: 1,
                offset: 0,
                bytes: BODY
            }
        ),
        [0, 0, 0, 0, 7, 0, 0, 0]
    );
    assert_eq!(
        run(
            &mut service,
            ObjectRequest::Commit {
                transaction: 1,
                crc: 0x66aa_5f61
            }
        ),
        [0, 0, 0, 0, 1, 0, 0, 0, 1, 0, 0, 0]
    );
    assert_eq!(
        run(&mut service, ObjectRequest::Info { object_id: 1 }),
        [
            0, 0, 0, 0, 1, 0, 1, 0, 0, 0, 7, 0, 0, 0, 0x61, 0x5f, 0xaa, 0x66
        ]
    );
    assert_eq!(
        run(
            &mut service,
            ObjectRequest::Read {
                object_id: 1,
                expected_revision: 1,
                offset: 2,
                max: 3
            }
        ),
        [
            0, 0, 0, 0, 1, 0, 0, 0, 7, 0, 0, 0, 2, 0, 0, 0, 3, 0, 255, 255, 1
        ]
    );
    assert_eq!(
        run(
            &mut service,
            ObjectRequest::Read {
                object_id: 1,
                expected_revision: 2,
                offset: 0,
                max: 7
            }
        ),
        [11, 0, 0, 0]
    );
}
#[test]
fn capacity_is_rejected_before_reservation_and_commit() {
    let owner = ObjectTransactions::new();
    let access = Auth::new();
    let mut memory = Memory::new();
    {
        let mut service = handler(&mut memory, &access, &owner);
        let mut body = [0; 31];
        begin(1, 1).encode(&mut body).unwrap();
        assert_eq!(
            service.dispatch(frame(0x20, &body), &mut [0; 9]),
            Err(SessionError::Capacity)
        );
        let transaction = tx(&run(&mut service, begin(1, 1)));
        run(
            &mut service,
            ObjectRequest::Write {
                transaction,
                offset: 0,
                bytes: BODY,
            },
        );
        let mut input = [0; 8];
        ObjectRequest::Commit {
            transaction,
            crc: 0x66aa_5f61,
        }
        .encode(&mut input)
        .unwrap();
        assert_eq!(
            service.dispatch(frame(0x22, &input), &mut [0; 11]),
            Err(SessionError::Capacity)
        );
        assert_eq!(
            &run(
                &mut service,
                ObjectRequest::Commit {
                    transaction,
                    crc: 0x66aa_5f61
                }
            )[..4],
            &[0; 4]
        );
    }
    assert_eq!(memory.reserves, 1);
    assert_eq!(memory.commits, 1);
}
#[test]
fn bad_crc_schema_and_commit_conflict_end_staging() {
    for mode in 0..3 {
        let owner = ObjectTransactions::new();
        let access = Auth::new();
        let mut memory = Memory::new();
        memory.conflict = mode == 2;
        let mut service = handler(&mut memory, &access, &owner);
        let transaction = tx(&run(&mut service, begin(1, 1)));
        let bytes = if mode == 1 {
            &[1, 0, 0, 0, 0, 0, 0][..]
        } else {
            BODY
        };
        run(
            &mut service,
            ObjectRequest::Write {
                transaction,
                offset: 0,
                bytes,
            },
        );
        let crc = if mode == 0 {
            0
        } else {
            object_crc(4, 1, bytes).unwrap()
        };
        let expected = match mode {
            0 => vec![18, 0, 1, 255],
            1 => vec![1, 0, 0, 0],
            _ => vec![11, 0, 0, 0],
        };
        assert_eq!(
            run(&mut service, ObjectRequest::Commit { transaction, crc }),
            expected
        );
        assert_eq!(
            run(&mut service, ObjectRequest::Abort { transaction }),
            [4, 0, 0, 0]
        );
        assert_eq!(&run(&mut service, begin(2, 2))[..4], &[0; 4]);
    }
}
#[test]
fn aborted_skipped_deleted_ids_stay_consumed_and_birth_is_zero() {
    let owner = ObjectTransactions::new();
    let access = Auth::new();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &owner);
    let transaction = tx(&run(&mut service, begin(5, 1)));
    run(&mut service, ObjectRequest::Abort { transaction });
    for id in [1, 5] {
        let r = run(&mut service, ObjectRequest::IdState { object_id: id });
        assert_eq!(r.len(), 39);
        assert_eq!(r[4], 2);
        assert_eq!(&r[5..9], &5u32.to_le_bytes());
        assert!(r[9..].iter().all(|input| *input == 0));
    }
    assert_eq!(run(&mut service, begin(5, 2)), [11, 0, 0, 0]);
    assert_eq!(
        run(&mut service, ObjectRequest::IdState { object_id: 6 })[4],
        0
    );
    create(&mut service, 8, 3);
    assert_eq!(
        run(
            &mut service,
            ObjectRequest::Delete {
                object_id: 8,
                expected_revision: 1
            }
        ),
        [0; 4]
    );
    assert_eq!(
        run(&mut service, ObjectRequest::IdState { object_id: 8 })[4],
        2
    );
}
#[test]
fn immutable_birth_survives_edit_current_info_changes() {
    let owner = ObjectTransactions::new();
    let access = Auth::new();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &owner);
    create(&mut service, 1, 1);
    let original = run(&mut service, ObjectRequest::IdState { object_id: 1 });
    let transaction = tx(&run(
        &mut service,
        ObjectRequest::Begin(ObjectBegin {
            kind: 1,
            object_id: 1,
            expected_revision: 1,
            total: 7,
            create: false,
            creation_token: [0; 16],
        }),
    ));
    let body = [0, 1, 2, 3, 4, 5, 6];
    run(
        &mut service,
        ObjectRequest::Write {
            transaction,
            offset: 0,
            bytes: &body,
        },
    );
    assert_eq!(
        &run(
            &mut service,
            ObjectRequest::Commit {
                transaction,
                crc: object_crc(4, 1, &body).unwrap()
            }
        )[8..],
        &2u32.to_le_bytes()
    );
    assert_eq!(
        run(&mut service, ObjectRequest::IdState { object_id: 1 }),
        original
    );
    assert_eq!(
        &run(&mut service, ObjectRequest::Info { object_id: 1 })[6..10],
        &2u32.to_le_bytes()
    );
}
#[test]
fn enumeration_is_inclusive_sparse_and_eof_without_empty_loops() {
    let owner = ObjectTransactions::new();
    let access = Auth::new();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &owner);
    create(&mut service, 2, 1);
    create(&mut service, 9, 2);
    let p = run(&mut service, ObjectRequest::Enumerate { start: 0, max: 1 });
    assert_eq!(p.len(), 23);
    assert_eq!(&p[8..12], &9u32.to_le_bytes());
    assert_eq!(p[12], 1);
    assert_eq!(&p[13..], &[2, 0, 0, 0, 1, 0, 7, 0, 0, 0]);
    let p = run(&mut service, ObjectRequest::Enumerate { start: 9, max: 1 });
    assert_eq!(&p[8..12], &[255; 4]);
    assert_eq!(&p[13..17], &9u32.to_le_bytes());
    let p = run(&mut service, ObjectRequest::Enumerate { start: 10, max: 1 });
    assert_eq!(p.len(), 13);
    assert_eq!(p[12], 0);
    assert_eq!(&p[8..12], &[255; 4]);
}
#[test]
fn malformed_repository_empty_non_eof_page_is_not_forwarded() {
    let owner = ObjectTransactions::new();
    let access = Auth::new();
    let mut memory = Memory::new();
    memory.invalid_page = true;
    let mut service = handler(&mut memory, &access, &owner);
    assert_eq!(
        run(&mut service, ObjectRequest::Enumerate { start: 0, max: 1 }),
        [17, 0, 0, 0]
    );
}
#[test]
fn actual_storage_failure_uses_storage_status_not_contract_error() {
    let owner = ObjectTransactions::new();
    let access = Auth::new();
    let mut memory = Memory::new();
    memory.stat_error = Some(StoreError::Backend);
    let mut service = handler(&mut memory, &access, &owner);
    assert_eq!(
        run(&mut service, ObjectRequest::Info { object_id: 1 }),
        [9, 0, 0, 0]
    );
}
#[test]
fn clear_needs_extra_scope_and_collection_cas_without_recycling() {
    let owner = ObjectTransactions::new();
    let access = Auth::new();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &owner);
    create(&mut service, 1, 1);
    access.dangerous.set(false);
    assert_eq!(
        run(
            &mut service,
            ObjectRequest::Delete {
                object_id: u32::MAX,
                expected_revision: 2
            }
        ),
        [5, 0, 4, 255]
    );
    access.dangerous.set(true);
    assert_eq!(
        run(
            &mut service,
            ObjectRequest::Delete {
                object_id: u32::MAX,
                expected_revision: 1
            }
        ),
        [11, 0, 0, 0]
    );
    assert_eq!(
        run(
            &mut service,
            ObjectRequest::Delete {
                object_id: u32::MAX,
                expected_revision: 2
            }
        ),
        [0; 4]
    );
    assert_eq!(
        run(&mut service, ObjectRequest::IdState { object_id: 1 })[4],
        2
    );
}
#[test]
fn module_and_specific_scope_are_independent_no_existence_leak() {
    let owner = ObjectTransactions::new();
    let access = Auth::new();
    access.blocked.set(Some(7));
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &owner);
    assert_eq!(run(&mut service, begin(7, 1)), [5, 0, 3, 255]);
    assert_eq!(
        run(&mut service, ObjectRequest::Info { object_id: 7 }),
        [5, 0, 3, 255]
    );
    access.enabled.set(false);
    assert_eq!(
        run(&mut service, ObjectRequest::Enumerate { start: 0, max: 1 }),
        [5, 0, 3, 255]
    );
}
#[test]
fn read_revocation_clears_all_output_and_permanently_closes() {
    let owner = ObjectTransactions::new();
    let access = Auth::new();
    let mut memory = Memory::new();
    {
        let mut service = handler(&mut memory, &access, &owner);
        create(&mut service, 1, 1);
    }
    memory.revoke_read = Some(&access);
    let mut service = handler(&mut memory, &access, &owner);
    let mut body = [0; 14];
    ObjectRequest::Read {
        object_id: 1,
        expected_revision: 1,
        offset: 0,
        max: 7,
    }
    .encode(&mut body)
    .unwrap();
    let mut out = [0xa5; 64];
    assert_eq!(
        service.dispatch(frame(0x23, &body), &mut out),
        Err(SessionError::AuthorizationRevoked)
    );
    assert_eq!(out, [0; 64]);
    assert!(service.is_closed());
    access.enabled.set(true);
    assert_eq!(
        service.dispatch(frame(0x23, &body), &mut out),
        Err(SessionError::AuthorizationRevoked)
    );
}
#[test]
fn atomic_commit_and_delete_recheck_actual_specific_permission() {
    let owner = ObjectTransactions::new();
    let access = Auth::new();
    let mut memory = Memory::new();
    memory.revoke_commit = Some(&access);
    {
        let mut service = handler(&mut memory, &access, &owner);
        let transaction = tx(&run(&mut service, begin(1, 1)));
        run(
            &mut service,
            ObjectRequest::Write {
                transaction,
                offset: 0,
                bytes: BODY,
            },
        );
        let mut input = [0; 8];
        ObjectRequest::Commit {
            transaction,
            crc: 0x66aa_5f61,
        }
        .encode(&mut input)
        .unwrap();
        assert_eq!(
            service.dispatch(frame(0x22, &input), &mut [0; 64]),
            Err(SessionError::AuthorizationRevoked)
        );
    }
    assert_eq!(memory.commits, 0);
    access.enabled.set(true);
    let owner = ObjectTransactions::new();
    memory.revoke_commit = None;
    {
        let mut service = handler(&mut memory, &access, &owner);
        create(&mut service, 2, 2);
    }
    memory.revoke_delete = Some(&access);
    {
        let mut service = handler(&mut memory, &access, &owner);
        let mut input = [0; 8];
        ObjectRequest::Delete {
            object_id: 2,
            expected_revision: 1,
        }
        .encode(&mut input)
        .unwrap();
        assert_eq!(
            service.dispatch(frame(0x24, &input), &mut [0; 64]),
            Err(SessionError::AuthorizationRevoked)
        );
    }
    assert!(memory.stat(4, 2).is_ok());
}
#[test]
fn unknown_kind_and_metadata_mismatch_never_enable_commands() {
    let owner = ObjectTransactions::new();
    let access = Auth::new();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &owner);
    assert_eq!(service.required_features(&metadata(), 0x20), Some(0));
    let mut wrong = metadata();
    wrong.minor = 1;
    assert_eq!(service.required_features(&wrong, 0x20), None);
    wrong = metadata();
    wrong.output_max = 128;
    assert_eq!(service.required_features(&wrong, 0x20), None);
    assert_eq!(service.required_features(&metadata(), 0x29), None);
    if let ObjectRequest::Begin(mut input) = begin(1, 1) {
        input.kind = 2;
        assert_eq!(run(&mut service, ObjectRequest::Begin(input)), [1, 0, 0, 0]);
    }
    assert_eq!(&run(&mut service, begin(1, 1))[..4], &[0; 4]);
}
#[test]
fn debug_redacts_tokens_staging_and_payload() {
    let owner = ObjectTransactions::new();
    let access = Auth::new();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &owner);
    let transaction = tx(&run(&mut service, begin(1, 0xab)));
    run(
        &mut service,
        ObjectRequest::Write {
            transaction,
            offset: 0,
            bytes: BODY,
        },
    );
    let debug = format!("{service:?}");
    assert!(!debug.contains("171"));
    assert!(!debug.contains("255"));
    assert!(!debug.contains("birth"));
    service.disconnect();
    assert!(service.is_closed());
}

#[test]
fn every_module_shares_one_transaction_and_cannot_write_another_modules_tx() {
    let access = Auth::new();
    let owner = ObjectTransactions::<64>::new();
    let mut memory = Memory::new();
    let mut other = Memory::new();
    let mut first = handler(&mut memory, &access, &owner);
    let transaction = tx(&run(&mut first, begin(1, 1)));
    let mut meta = metadata();
    meta.module = 5;
    let mut second = ObjectHandler::new(meta, &mut other, &Schema, &access, &[1], &owner).unwrap();
    let mut bytes = [0; 512];
    let n = begin(1, 2).encode(&mut bytes).unwrap();
    let mut req = frame(0x20, &bytes[..n]);
    req.header.module = 5;
    let mut out = [0; 512];
    assert_eq!(second.dispatch(req, &mut out), Ok(4));
    assert_eq!(&out[..4], &[6, 0, 0, 0]);
    let r = ObjectRequest::Write {
        transaction,
        offset: 0,
        bytes: BODY,
    };
    let n = r.encode(&mut bytes).unwrap();
    let mut req = frame(0x21, &bytes[..n]);
    req.header.module = 5;
    assert_eq!(second.dispatch(req, &mut out), Ok(4));
    assert_eq!(&out[..4], &[4, 0, 0, 0]);
    assert_eq!(
        run(
            &mut first,
            ObjectRequest::Write {
                transaction,
                offset: 0,
                bytes: BODY
            }
        ),
        [0, 0, 0, 0, 7, 0, 0, 0]
    );
}

#[test]
fn malformed_commit_ends_only_its_modules_staging() {
    let access = Auth::new();
    let owner = ObjectTransactions::<64>::new();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &owner);
    let transaction = tx(&run(&mut service, begin(1, 1)));
    let mut out = [0; 512];
    assert_eq!(service.dispatch(frame(0x22, &[1, 0, 0]), &mut out), Ok(4));
    assert_eq!(&out[..4], &[1, 0, 0, 0]);
    assert_eq!(
        run(&mut service, ObjectRequest::Abort { transaction }),
        [4, 0, 0, 0]
    );
}
fn session(modules: &[ModuleEntry]) -> device_services::Session<'_> {
    session_with_control(modules, 512)
}
fn session_with_control(modules: &[ModuleEntry], control: u16) -> device_services::Session<'_> {
    use device_protocol::ProtocolLimits;
    use device_services::{BuildProfile, Config, DeviceInfo, Session};
    let mut session = Session::new(Config {
        session: 1,
        capability_rev: 1,
        profile: BuildProfile::Commercial,
        info: DeviceInfo {
            product_id: 0,
            fw_major: 0,
            fw_minor: 0,
            fw_patch: 0,
            build_id: 0,
        },
        modules,
        limits: ProtocolLimits {
            rx_frame_max: 20,
            tx_frame_max: 20,
            control_max: 512,
            data_max: 0,
            idle_ms: 2000,
            fragment_ms: 100,
            response_ms: 10000,
        },
    })
    .unwrap();
    session.control_notifications_enabled().unwrap();
    let limit = control.to_le_bytes();
    let body = [1, 1, limit[0], limit[1], 0, 0, 3, 0];
    let mut hello = frame(1, &body);
    hello.header.module = 0;
    session.dispatch(hello, &mut [0; 512]).unwrap();
    session
}
fn modules() -> [ModuleEntry; 2] {
    let mut core = metadata();
    core.module = 0;
    core.minor = 2;
    [core, metadata()]
}
#[test]
fn session_commit_replay_never_reexecutes_and_revoked_cached_scope_closes() {
    let modules = modules();
    let mut session = session(&modules);
    let access = Auth::new();
    let owner = ObjectTransactions::new();
    let mut memory = Memory::new();
    {
        let mut service = handler(&mut memory, &access, &owner);
        let mut bytes = [0; 512];
        let mut out = [0; 512];
        for (id, request) in [
            (2, begin(1, 1)),
            (
                3,
                ObjectRequest::Write {
                    transaction: 1,
                    offset: 0,
                    bytes: BODY,
                },
            ),
            (
                4,
                ObjectRequest::Commit {
                    transaction: 1,
                    crc: 0x66aa_5f61,
                },
            ),
        ] {
            let n = request.encode(&mut bytes).unwrap();
            let mut req = frame(request.opcode(), &bytes[..n]);
            req.header.message_id = id;
            assert!(
                session
                    .dispatch_with_handler(req, &mut service, &mut out)
                    .is_ok()
            );
            assert_eq!(&out[..4], &[0; 4]);
        }
        let request = ObjectRequest::Commit {
            transaction: 1,
            crc: 0x66aa_5f61,
        };
        let n = request.encode(&mut bytes).unwrap();
        let mut req = frame(0x22, &bytes[..n]);
        req.header.message_id = 4;
        assert_eq!(
            session.dispatch_with_handler(req, &mut service, &mut out),
            Ok(12)
        );
        access.blocked.set(Some(1));
        out.fill(0xa5);
        assert_eq!(
            session.dispatch_with_handler(req, &mut service, &mut out),
            Err(SessionError::AuthorizationRevoked)
        );
        assert_eq!(out, [0; 512]);
        assert_eq!(session.state(), device_services::SessionState::Closed);
    }
    assert_eq!(memory.commits, 1);
}
#[test]
fn revoked_staging_is_scrubbed_before_even_a_core_ping() {
    let modules = modules();
    let mut session = session(&modules);
    let access = Auth::new();
    let owner = ObjectTransactions::new();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &owner);
    let mut bytes = [0; 31];
    begin(1, 1).encode(&mut bytes).unwrap();
    let mut request = frame(0x20, &bytes);
    request.header.message_id = 2;
    session
        .dispatch_with_handler(request, &mut service, &mut [0; 512])
        .unwrap();
    access.blocked.set(Some(1));
    let mut ping = frame(5, &[0; 4]);
    ping.header.module = 0;
    ping.header.message_id = 3;
    let mut out = [0xa5; 512];
    assert_eq!(
        session.dispatch_with_handler(ping, &mut service, &mut out),
        Err(SessionError::AuthorizationRevoked)
    );
    assert_eq!(out, [0; 512]);
    assert!(service.is_closed());
    access.blocked.set(None);
    assert!(service.must_end_session());
}
#[test]
fn enumeration_replay_checks_original_returned_and_next_resource_scopes() {
    let owner = ObjectTransactions::new();
    let access = Auth::new();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &owner);
    create(&mut service, 2, 1);
    create(&mut service, 9, 2);
    run(&mut service, ObjectRequest::Enumerate { start: 0, max: 1 });
    let mut bytes = [0; 5];
    ObjectRequest::Enumerate { start: 0, max: 1 }
        .encode(&mut bytes)
        .unwrap();
    access.blocked.set(Some(9));
    assert!(!service.authorized(frame(0x25, &bytes)));
    assert!(service.must_end_session());
}
#[test]
fn session_clear_without_dangerous_scope_returns_exact_d_reason() {
    let modules = modules();
    let mut session = session(&modules);
    let owner = ObjectTransactions::new();
    let access = Auth::new();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &owner);
    create(&mut service, 1, 1);
    access.dangerous.set(false);
    let mut bytes = [0; 8];
    ObjectRequest::Delete {
        object_id: u32::MAX,
        expected_revision: 2,
    }
    .encode(&mut bytes)
    .unwrap();
    let mut request = frame(0x24, &bytes);
    request.header.message_id = 2;
    let mut out = [0; 512];
    assert_eq!(
        session.dispatch_with_handler(request, &mut service, &mut out),
        Ok(4)
    );
    assert_eq!(&out[..4], &[5, 0, 4, 255]);
}
#[test]
fn stored_integrity_error_uses_the_stored_crc_reason() {
    let owner = ObjectTransactions::new();
    let access = Auth::new();
    let mut memory = Memory::new();
    memory.stat_error = Some(StoreError::Integrity);
    let mut service = handler(&mut memory, &access, &owner);
    assert_eq!(
        run(&mut service, ObjectRequest::Info { object_id: 1 }),
        [18, 0, 2, 255]
    );
}
#[test]
fn cached_clear_ok_cannot_survive_dangerous_scope_revocation() {
    let modules = modules();
    let mut session = session(&modules);
    let owner = ObjectTransactions::new();
    let access = Auth::new();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &owner);
    create(&mut service, 1, 1);
    let mut bytes = [0; 8];
    ObjectRequest::Delete {
        object_id: u32::MAX,
        expected_revision: 2,
    }
    .encode(&mut bytes)
    .unwrap();
    let mut request = frame(0x24, &bytes);
    request.header.message_id = 2;
    let mut out = [0; 512];
    assert_eq!(
        session.dispatch_with_handler(request, &mut service, &mut out),
        Ok(4)
    );
    assert_eq!(&out[..4], &[0; 4]);
    access.dangerous.set(false);
    out.fill(0xa5);
    assert_eq!(
        session.dispatch_with_handler(request, &mut service, &mut out),
        Err(SessionError::AuthorizationRevoked)
    );
    assert_eq!(out, [0; 512]);
    assert!(service.is_closed());
}
#[test]
fn repository_result_debug_exposes_only_lengths_not_private_ids_or_evidence() {
    let id = 0x1234_5678;
    let entry = DirectoryEntry {
        id,
        kind: 513,
        total: 21,
    };
    let page = Enumeration {
        revision: id,
        next: id,
        count: 1,
    };
    let state = IdState {
        state: 1,
        high_water: id,
        birth: Birth {
            kind: 513,
            total: 21,
            crc: id,
            revision: 1,
            token: [171; 16],
        },
    };
    let read = ReadResult {
        meta: ObjectMeta {
            object_id: id,
            kind: 513,
            revision: id,
            total: 21,
            crc: id,
            birth_token: [171; 16],
        },
        length: 7,
    };
    let debug = format!("{entry:?}{page:?}{state:?}{read:?}");
    assert!(!debug.contains("305419896"));
    assert!(!debug.contains("513"));
    assert!(!debug.contains("171"));
    assert!(debug.contains("length"));
}
#[test]
fn session_lifecycle_hook_discards_staging_and_permanently_closes_owner() {
    let owner = ObjectTransactions::new();
    let access = Auth::new();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &owner);
    let transaction = tx(&run(&mut service, begin(5, 1)));
    assert_eq!(service.connection_closed(), Ok(()));
    assert!(service.must_end_session());
    assert_eq!(service.connection_closed(), Ok(()));
    let mut bytes = [0; 4];
    ObjectRequest::Abort { transaction }
        .encode(&mut bytes)
        .unwrap();
    let mut out = [0xa5; 512];
    assert_eq!(
        service.dispatch(frame(0x26, &bytes), &mut out),
        Err(SessionError::AuthorizationRevoked)
    );
    assert_eq!(out, [0; 512]);
}
#[test]
fn negotiated_control_capacity_bounds_read_and_enum_even_when_catalogue_declares_512() {
    let modules = modules();
    let mut session = session_with_control(&modules, 128);
    let owner = ObjectTransactions::new();
    let access = Auth::new();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &owner);
    create(&mut service, 2, 1);
    create(&mut service, 9, 2);
    let mut bytes = [0; 14];
    ObjectRequest::Read {
        object_id: 2,
        expected_revision: 1,
        offset: 0,
        max: 400,
    }
    .encode(&mut bytes)
    .unwrap();
    let mut request = frame(0x23, &bytes);
    request.header.message_id = 2;
    let mut out = [0; 512];
    assert_eq!(
        session.dispatch_with_handler(request, &mut service, &mut out),
        Ok(25)
    );
    assert_eq!(&out[18..25], BODY);
    let mut bytes = [0; 5];
    ObjectRequest::Enumerate { start: 0, max: 255 }
        .encode(&mut bytes)
        .unwrap();
    let mut request = frame(0x25, &bytes);
    request.header.message_id = 3;
    assert_eq!(
        session.dispatch_with_handler(request, &mut service, &mut out),
        Ok(33)
    );
    assert_eq!(out[12], 2);
}
#[test]
fn begin_advertises_a_chunk_that_fits_the_actual_control_capacity() {
    let owner = ObjectTransactions::<256>::new();
    let access = Auth::new();
    let mut memory = Memory::new();
    let mut service =
        ObjectHandler::new(metadata(), &mut memory, &Schema, &access, &[1], &owner).unwrap();
    let mut body = [0; 31];
    begin(1, 1).encode(&mut body).unwrap();
    let mut out = [0; 128];
    assert_eq!(service.dispatch(frame(0x20, &body), &mut out), Ok(10));
    assert_eq!(&out[8..10], &118u16.to_le_bytes());
}
#[test]
fn initially_missing_one_directory_resource_scope_is_denied_without_claiming_revocation() {
    let owner = ObjectTransactions::new();
    let access = Auth::new();
    let mut memory = Memory::new();
    let mut service = handler(&mut memory, &access, &owner);
    create(&mut service, 2, 1);
    create(&mut service, 9, 2);
    access.blocked.set(Some(9));
    assert_eq!(
        run(&mut service, ObjectRequest::Enumerate { start: 0, max: 2 }),
        [5, 0, 3, 255]
    );
    assert!(!service.is_closed());
}
