//! Generic object service; physical durable storage and product schemas are injected.
#![no_std]
#![forbid(unsafe_code)]
use core::cell::{Cell, RefCell};
use device_protocol::{Frame, Kind, LOGICAL_MAX, ModuleEntry, ObjectBegin, ObjectRequest};
use device_services::{Reason, ServiceHandler, SessionError, Status};
use device_storage::{
    Authorization, Begin, ObjectEngine, ObjectMeta, ObjectSchema, ObjectStore, StoreError,
};

/// Trusted resource operation, never granted by a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObjectOperation {
    /// Read body or immutable identity evidence.
    Read,
    /// Create, edit, stage, commit, abort or delete one object.
    Write,
    /// Read directory entries and collection revision.
    Enumerate,
    /// Atomically clear all objects without recycling stable IDs.
    Clear,
}
/// Live integration-owned resource permission.
pub trait ObjectAccess {
    /// None checks module scope, Some binds a concrete ID (including clear sentinel).
    fn allowed(&self, module: u8, operation: ObjectOperation, id: Option<u32>) -> bool;
    /// Independently granted dangerous-write scope and product clear policy.
    fn dangerous_allowed(&self, module: u8) -> bool;
}
/// Specific immutable resource binding passed to durable reservation and CAS.
pub struct BoundAuthorization<'a> {
    module: u8,
    id: u32,
    access: &'a dyn ObjectAccess,
}
impl core::fmt::Debug for BoundAuthorization<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("BoundAuthorization").finish_non_exhaustive()
    }
}
impl Authorization for BoundAuthorization<'_> {
    fn write_allowed(&self, module: u8) -> bool {
        module == self.module
            && self.access.allowed(module, ObjectOperation::Write, None)
            && self
                .access
                .allowed(module, ObjectOperation::Write, Some(self.id))
    }
}
/// Exact one-entry directory representation, encoded as10 bytes.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct DirectoryEntry {
    /// Stable ID, 1..FFFFFFFE.
    pub id: u32,
    /// Actual known module-local kind.
    pub kind: u16,
    /// Complete object byte length.
    pub total: u32,
}
/// Atomic consistent directory page.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Enumeration {
    /// Nonzero collection revision.
    pub revision: u32,
    /// First unreturned actual ID, or FFFFFFFF at EOF.
    pub next: u32,
    /// Number of initialized entries.
    pub count: usize,
}
/// One revision-bound consistent read.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ReadResult {
    /// Actual metadata of the same body snapshot.
    pub meta: ObjectMeta,
    /// Number of actual initialized output bytes.
    pub length: usize,
}
/// Immutable creation evidence, separate from the current object metadata.
#[derive(Clone, Copy, PartialEq, Eq, Default)]
pub struct Birth {
    /// First committed kind.
    pub kind: u16,
    /// First committed complete length.
    pub total: u32,
    /// First WOBJ CRC.
    pub crc: u32,
    /// First committed revision, exactly1.
    pub revision: u32,
    /// Nonzero immutable creation token.
    pub token: [u8; 16],
}
impl core::fmt::Debug for Birth {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Birth")
            .field("encoded_length", &30)
            .finish_non_exhaustive()
    }
}
/// Actual durable ID state and high water.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct IdState {
    /// 0unused,1exists,2consumed without existing object.
    pub state: u8,
    /// Persistent high water, never FFFFFFFF or reduced by deletion/abort.
    pub high_water: u32,
    /// Zero in state0/2; immutable original evidence in state1.
    pub birth: Birth,
}
impl core::fmt::Debug for DirectoryEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("DirectoryEntry")
            .field("encoded_length", &10)
            .finish_non_exhaustive()
    }
}
impl core::fmt::Debug for Enumeration {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Enumeration")
            .field(
                "encoded_length",
                &self.count.saturating_mul(10).saturating_add(9),
            )
            .finish_non_exhaustive()
    }
}
impl core::fmt::Debug for ReadResult {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ReadResult")
            .field("length", &self.length)
            .finish_non_exhaustive()
    }
}
impl core::fmt::Debug for IdState {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("IdState")
            .field("encoded_length", &35)
            .finish_non_exhaustive()
    }
}
/// Mandatory real repository contracts; no production memory backend is supplied.
pub trait ObjectRepository: ObjectStore {
    /// Copy the complete requested range from one expected-revision snapshot.
    /// Recheck current specific read scope; return Conflict rather than mixed revisions.
    /// # Errors
    /// Actual bounds, authorization, storage and revision failures.
    fn read_range(
        &self,
        module: u8,
        id: u32,
        expected: u32,
        offset: u32,
        out: &mut [u8],
        access: &dyn ObjectAccess,
    ) -> Result<ReadResult, StoreError>;
    /// Atomically recheck live specific authorization, existence and revision CAS.
    /// FFFFFFFF means collection CAS and requires Clear plus dangerous scope.
    /// Persist collection revision and tombstones; never reduce/recycle high water.
    /// # Errors
    /// Validation/CAS failures preserve old state. An uncertain Backend outcome
    /// blocks successful access until recovery; query the ID/revision afterward,
    /// because the complete new deletion may have become durable.
    fn delete_atomic(
        &mut self,
        module: u8,
        id: u32,
        expected: u32,
        access: &dyn ObjectAccess,
    ) -> Result<(), StoreError>;
    /// Actual persistent sorted directory snapshot, inclusive start, no omissions.
    /// Next is the first unreturned actual ID; empty pages must be EOF.
    /// Include only a complete authorized page; do not leak inaccessible metadata.
    /// # Errors
    /// Actual scope, storage or capacity failures.
    fn enumerate(
        &self,
        module: u8,
        start: u32,
        out: &mut [DirectoryEntry],
        access: &dyn ObjectAccess,
    ) -> Result<Enumeration, StoreError>;
    /// Read actual durable high water and immutable first-commit evidence.
    /// # Errors
    /// Actual specific read scope or storage failure; no synthesized volatile state.
    fn id_state(
        &self,
        module: u8,
        id: u32,
        access: &dyn ObjectAccess,
    ) -> Result<IdState, StoreError>;
}
/// Invalid immutable adapter configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigError {
    /// Module metadata/capacities do not describe the compiled adapter.
    Metadata,
    /// Empty, unsorted, duplicate or zero actual kind list.
    Kinds,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum DispatchFailure {
    Store(StoreError),
    Contract,
}
impl From<StoreError> for DispatchFailure {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}
#[derive(Clone, Copy)]
struct Active {
    tx: u32,
    begin: Begin,
    chunk_max: u16,
}
#[derive(Clone, Copy)]
struct EnumGuard {
    valid: bool,
    start: u32,
    max: u8,
    ids: [u32; 50],
    count: usize,
}
impl EnumGuard {
    const EMPTY: Self = Self {
        valid: false,
        start: 0,
        max: 0,
        ids: [0; 50],
        count: 0,
    };
}
/// One bounded transaction owner shared by every object module on a connection.
/// Never construct independent owners for modules on the same connection.
pub struct ObjectTransactions<const N: usize> {
    engine: RefCell<ObjectEngine<N>>,
    active: Cell<Option<Active>>,
    last_tx: Cell<Option<Active>>,
    closed: Cell<bool>,
}
impl<const N: usize> Default for ObjectTransactions<N> {
    fn default() -> Self {
        Self::new()
    }
}
impl<const N: usize> ObjectTransactions<N> {
    /// Construct once for one new physical connection; closed owners never reopen.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            engine: RefCell::new(ObjectEngine::new()),
            active: Cell::new(None),
            last_tx: Cell::new(None),
            closed: Cell::new(false),
        }
    }
}
impl<const N: usize> core::fmt::Debug for ObjectTransactions<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ObjectTransactions")
            .field("capacity", &N)
            .finish_non_exhaustive()
    }
}
/// Session handler owning one bounded transaction and one real borrowed repository.
/// Scope checks use interior mutability solely to scrub staging from Session's
/// immutable authorization preflight; no background execution or allocation exists.
pub struct ObjectHandler<'a, R: ObjectRepository, S: ObjectSchema, const N: usize> {
    metadata: ModuleEntry,
    repository: &'a mut R,
    schema: &'a S,
    access: &'a dyn ObjectAccess,
    kinds: &'a [u16],
    transactions: &'a ObjectTransactions<N>,
    enum_guard: Cell<EnumGuard>,
    clear_guard: Cell<Option<u16>>,
}

impl<R: ObjectRepository, S: ObjectSchema, const N: usize> core::fmt::Debug
    for ObjectHandler<'_, R, S, N>
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ObjectHandler")
            .field("capacity", &N)
            .finish_non_exhaustive()
    }
}
impl<'a, R: ObjectRepository, S: ObjectSchema, const N: usize> ObjectHandler<'a, R, S, N> {
    /// Bind exact compiled module metadata, a real schema and sorted supported kinds.
    /// Kinds must describe the actual complete-body schema, never SDK availability.
    /// # Errors
    /// Reject invalid metadata, staging capacity or empty/unsorted/zero kinds.
    pub fn new(
        metadata: ModuleEntry,
        repository: &'a mut R,
        schema: &'a S,
        access: &'a dyn ObjectAccess,
        kinds: &'a [u16],
        transactions: &'a ObjectTransactions<N>,
    ) -> Result<Self, ConfigError> {
        if metadata.module == 0
            || metadata.major == 0
            || metadata.availability != 1
            || metadata.features != 0
            || !(128..=512).contains(&metadata.input_max)
            || !(128..=512).contains(&metadata.output_max)
            || N == 0
            || N > 4096
        {
            return Err(ConfigError::Metadata);
        }
        if kinds.is_empty() || kinds[0] == 0 || kinds.windows(2).any(|w| w[0] >= w[1]) {
            return Err(ConfigError::Kinds);
        }
        Ok(Self {
            metadata,
            repository,
            schema,
            access,
            kinds,
            transactions,
            enum_guard: Cell::new(EnumGuard::EMPTY),
            clear_guard: Cell::new(None),
        })
    }
    /// Scrub all staging and permanently end this connection-owned handler.
    pub fn disconnect(&mut self) {
        self.invalidate();
    }
    /// Whether connection teardown is required; never reset this handler for reconnect.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.transactions.closed.get()
    }
    fn invalidate(&self) {
        self.transactions
            .engine
            .borrow_mut()
            .authorization_revoked();
        self.transactions.active.set(None);
        self.transactions.last_tx.set(None);
        self.enum_guard.set(EnumGuard::EMPTY);
        self.clear_guard.set(None);
        self.transactions.closed.set(true);
    }
    fn allowed(&self, operation: ObjectOperation, id: Option<u32>) -> bool {
        self.access.allowed(self.metadata.module, operation, None)
            && id.is_none_or(|id| {
                self.access
                    .allowed(self.metadata.module, operation, Some(id))
            })
    }
    fn active_live(&self) -> bool {
        if self.transactions.closed.get() {
            return false;
        }
        if self.transactions.active.get().is_some_and(|a| {
            !(self
                .access
                .allowed(a.begin.module, ObjectOperation::Write, None)
                && self.access.allowed(
                    a.begin.module,
                    ObjectOperation::Write,
                    Some(a.begin.object_id),
                ))
        }) {
            self.invalidate();
            return false;
        }
        true
    }
    fn binding(&self, tx: u32) -> Option<Active> {
        self.transactions
            .active
            .get()
            .or(self.transactions.last_tx.get())
            .filter(|a| a.tx == tx && a.begin.module == self.metadata.module)
    }
    fn scope(&self, r: ObjectRequest<'_>) -> (ObjectOperation, Option<u32>) {
        match r {
            ObjectRequest::Begin(b) => (ObjectOperation::Write, Some(b.object_id)),
            ObjectRequest::Write { transaction, .. }
            | ObjectRequest::Commit { transaction, .. }
            | ObjectRequest::Abort { transaction } => (
                ObjectOperation::Write,
                self.binding(transaction).map(|a| a.begin.object_id),
            ),
            ObjectRequest::Read { object_id, .. }
            | ObjectRequest::Info { object_id }
            | ObjectRequest::IdState { object_id } => (ObjectOperation::Read, Some(object_id)),
            ObjectRequest::Delete { object_id, .. } => (
                if object_id == u32::MAX {
                    ObjectOperation::Clear
                } else {
                    ObjectOperation::Write
                },
                Some(object_id),
            ),
            ObjectRequest::Enumerate { .. } => (ObjectOperation::Enumerate, None),
        }
    }
    fn preflight(&self, request: ObjectRequest<'_>) -> bool {
        let (op, id) = self.scope(request);
        if !self.allowed(op, id) {
            return false;
        }
        if let ObjectRequest::Enumerate { start, max } = request {
            let guard = self.enum_guard.get();
            if guard.valid
                && guard.start == start
                && guard.max == max
                && guard.ids[..guard.count]
                    .iter()
                    .any(|id| !self.allowed(op, Some(*id)))
            {
                self.invalidate();
                return false;
            }
        }
        true
    }
    fn known(&self, kind: u16) -> bool {
        self.kinds.binary_search(&kind).is_ok()
    }
    fn valid_meta(&self, meta: ObjectMeta, id: u32) -> bool {
        meta.object_id == id
            && self.known(meta.kind)
            && meta.revision != 0
            && meta.total <= 4096
            && meta.birth_token != [0; 16]
    }
    fn end_staging(&self) {
        self.transactions.engine.borrow_mut().disconnect();
        if let Some(a) = self.transactions.active.take() {
            self.transactions.last_tx.set(Some(a));
        }
    }
    fn execute(
        &mut self,
        request: ObjectRequest<'_>,
        out: &mut [u8],
    ) -> Result<usize, DispatchFailure> {
        match request {
            ObjectRequest::Begin(b) => self.begin_object(b, out),
            ObjectRequest::Write {
                transaction,
                offset,
                bytes,
            } => self.write_object(transaction, offset, bytes, out),
            ObjectRequest::Commit { transaction, crc } => self.commit_object(transaction, crc, out),
            ObjectRequest::Read {
                object_id,
                expected_revision,
                offset,
                max,
            } => self.read_object(object_id, expected_revision, offset, max, out),
            ObjectRequest::Delete {
                object_id,
                expected_revision,
            } => self.delete_object(object_id, expected_revision, out),
            ObjectRequest::Enumerate { start, max } => self.enumerate_objects(start, max, out),
            ObjectRequest::Abort { transaction } => self.abort_object(transaction, out),
            ObjectRequest::Info { object_id } => self.info_object(object_id, out),
            ObjectRequest::IdState { object_id } => self.state_object(object_id, out),
        }
    }
    fn begin_object(&mut self, b: ObjectBegin, out: &mut [u8]) -> Result<usize, DispatchFailure> {
        let module = self.metadata.module;
        if !self.known(b.kind) {
            return Err(StoreError::Schema.into());
        }
        let begin = Begin {
            module,
            kind: b.kind,
            object_id: b.object_id,
            expected_revision: b.expected_revision,
            total: b.total,
            create: b.create,
            creation_token: b.creation_token,
        };
        let auth = BoundAuthorization {
            module,
            id: b.object_id,
            access: self.access,
        };
        let tx = self
            .transactions
            .engine
            .borrow_mut()
            .begin(begin, self.repository, &auth)?;
        let chunk_max = self.chunk_max(out.len());
        let active = Active {
            tx,
            begin,
            chunk_max,
        };
        self.transactions.active.set(Some(active));
        self.transactions.last_tx.set(Some(active));
        put32(out, 4, tx);
        put16(out, 8, chunk_max);
        Ok(10)
    }
    fn write_object(
        &mut self,
        transaction: u32,
        offset: u32,
        bytes: &[u8],
        out: &mut [u8],
    ) -> Result<usize, DispatchFailure> {
        let active = self.binding(transaction).ok_or(StoreError::InvalidState)?;
        if bytes.len() > usize::from(active.chunk_max) {
            return Err(StoreError::Capacity.into());
        }
        let end = self
            .transactions
            .engine
            .borrow_mut()
            .write(transaction, offset, bytes)?;
        put32(out, 4, end);
        Ok(8)
    }
    fn commit_object(
        &mut self,
        transaction: u32,
        crc: u32,
        out: &mut [u8],
    ) -> Result<usize, DispatchFailure> {
        let module = self.metadata.module;
        let Some(active) = self
            .transactions
            .active
            .get()
            .filter(|a| a.tx == transaction && a.begin.module == self.metadata.module)
        else {
            if self
                .transactions
                .active
                .get()
                .is_none_or(|a| a.begin.module == self.metadata.module)
            {
                self.end_staging();
            }
            return Err(StoreError::InvalidState.into());
        };
        let auth = BoundAuthorization {
            module,
            id: active.begin.object_id,
            access: self.access,
        };
        let committed = self.transactions.engine.borrow_mut().commit(
            transaction,
            crc,
            self.repository,
            self.schema,
            &auth,
        );
        self.end_staging();
        let meta = committed?;
        let b = active.begin;
        let revision = if b.create {
            Some(1)
        } else {
            b.expected_revision.checked_add(1)
        };
        if !self.valid_meta(meta, b.object_id)
            || meta.kind != b.kind
            || meta.total != b.total
            || meta.crc != crc
            || Some(meta.revision) != revision
            || (b.create && meta.birth_token != b.creation_token)
        {
            return Err(DispatchFailure::Contract);
        }
        put32(out, 4, meta.object_id);
        put32(out, 8, meta.revision);
        Ok(12)
    }
    fn read_object(
        &mut self,
        object_id: u32,
        expected_revision: u32,
        offset: u32,
        max: u16,
        out: &mut [u8],
    ) -> Result<usize, DispatchFailure> {
        let module = self.metadata.module;
        let capacity = usize::from(max).min(out.len() - 18);
        let read = self.repository.read_range(
            module,
            object_id,
            expected_revision,
            offset,
            &mut out[18..18 + capacity],
            self.access,
        )?;
        if read.meta.revision != expected_revision {
            return Err(StoreError::Conflict.into());
        }
        if !self.valid_meta(read.meta, object_id)
            || offset > read.meta.total
            || read.length
                != capacity.min(
                    usize::try_from(read.meta.total - offset).map_err(|_| StoreError::Backend)?,
                )
        {
            return Err(DispatchFailure::Contract);
        }
        put32(out, 4, read.meta.revision);
        put32(out, 8, read.meta.total);
        put32(out, 12, offset);
        put16(
            out,
            16,
            u16::try_from(read.length).map_err(|_| StoreError::Backend)?,
        );
        Ok(18 + read.length)
    }
    fn delete_object(
        &mut self,
        object_id: u32,
        expected_revision: u32,
        _out: &mut [u8],
    ) -> Result<usize, DispatchFailure> {
        let module = self.metadata.module;
        self.repository
            .delete_atomic(module, object_id, expected_revision, self.access)?;
        Ok(4)
    }
    fn enumerate_objects(
        &mut self,
        start: u32,
        max: u8,
        out: &mut [u8],
    ) -> Result<usize, DispatchFailure> {
        let module = self.metadata.module;
        let capacity = usize::from(max).min((out.len() - 13) / 10);
        let mut entries = [DirectoryEntry::default(); 49];
        let page =
            self.repository
                .enumerate(module, start, &mut entries[..capacity], self.access)?;
        if page.revision == 0 || page.count > capacity || (page.count == 0 && page.next != u32::MAX)
        {
            return Err(DispatchFailure::Contract);
        }
        let mut previous = None;
        let mut guard = EnumGuard {
            valid: true,
            start,
            max,
            ..EnumGuard::EMPTY
        };
        for (n, e) in entries[..page.count].iter().enumerate() {
            if e.id == 0
                || e.id == u32::MAX
                || e.id < start
                || previous.is_some_and(|p| p >= e.id)
                || !self.known(e.kind)
                || e.total > 4096
            {
                return Err(DispatchFailure::Contract);
            }
            if !self.allowed(ObjectOperation::Enumerate, Some(e.id)) {
                return Err(StoreError::Denied.into());
            }
            previous = Some(e.id);
            guard.ids[guard.count] = e.id;
            guard.count += 1;
            let at = 13 + n * 10;
            put32(out, at, e.id);
            put16(out, at + 4, e.kind);
            put32(out, at + 6, e.total);
        }
        if page.next != u32::MAX {
            if page.next == 0 || page.next < start || previous.is_none_or(|p| page.next <= p) {
                return Err(DispatchFailure::Contract);
            }
            if !self.allowed(ObjectOperation::Enumerate, Some(page.next)) {
                return Err(StoreError::Denied.into());
            }
            guard.ids[guard.count] = page.next;
            guard.count += 1;
        }
        self.enum_guard.set(guard);
        put32(out, 4, page.revision);
        put32(out, 8, page.next);
        out[12] = u8::try_from(page.count).map_err(|_| StoreError::Backend)?;
        Ok(13 + page.count * 10)
    }
    fn abort_object(
        &mut self,
        transaction: u32,
        _out: &mut [u8],
    ) -> Result<usize, DispatchFailure> {
        if self.binding(transaction).is_none() {
            return Err(StoreError::InvalidState.into());
        }
        self.transactions.engine.borrow_mut().abort(transaction)?;
        if let Some(a) = self.transactions.active.take() {
            self.transactions.last_tx.set(Some(a));
        }
        Ok(4)
    }
    fn info_object(&mut self, object_id: u32, out: &mut [u8]) -> Result<usize, DispatchFailure> {
        let module = self.metadata.module;
        let meta = self.repository.stat(module, object_id)?;
        if !self.valid_meta(meta, object_id) {
            return Err(DispatchFailure::Contract);
        }
        put16(out, 4, meta.kind);
        put32(out, 6, meta.revision);
        put32(out, 10, meta.total);
        put32(out, 14, meta.crc);
        Ok(18)
    }
    fn state_object(&mut self, object_id: u32, out: &mut [u8]) -> Result<usize, DispatchFailure> {
        let module = self.metadata.module;
        let state = self.repository.id_state(module, object_id, self.access)?;
        let b = state.birth;
        if state.high_water == u32::MAX
            || state.state > 2
            || (state.state == 0 && object_id <= state.high_water)
            || (state.state != 0 && object_id > state.high_water)
            || (state.state != 1 && b != Birth::default())
            || (state.state == 1
                && (!self.known(b.kind) || b.total > 4096 || b.revision != 1 || b.token == [0; 16]))
        {
            return Err(DispatchFailure::Contract);
        }
        out[4] = state.state;
        put32(out, 5, state.high_water);
        put16(out, 9, b.kind);
        put32(out, 11, b.total);
        put32(out, 15, b.crc);
        put32(out, 19, b.revision);
        out[23..39].copy_from_slice(&b.token);
        Ok(39)
    }
    fn chunk_max(&self, output_limit: usize) -> u16 {
        // Session supplies its actual >=128 logical output limit. A standalone
        // caller may provide only the fixed10-byte reply buffer; that does not
        // negotiate a smaller connection. Conservative advertised limits remain
        // valid when the module's declared output maximum is smaller than input.
        let control = if output_limit >= 128 {
            u16::try_from(output_limit).unwrap_or(u16::MAX)
        } else {
            self.metadata.input_max
        };
        self.metadata
            .input_max
            .min(control)
            .saturating_sub(10)
            .min(u16::try_from(N).unwrap_or(u16::MAX))
    }
    fn needed(request: ObjectRequest<'_>, limit: usize) -> usize {
        match request {
            ObjectRequest::Begin(_) => 10,
            ObjectRequest::Write { .. } => 8,
            ObjectRequest::Commit { .. } => 12,
            ObjectRequest::Read { max, .. } => {
                if limit < 19 {
                    19
                } else {
                    18 + usize::from(max).min(limit - 18)
                }
            }
            ObjectRequest::Enumerate { max, .. } => {
                if limit < 13 {
                    13
                } else {
                    13 + 10 * usize::from(max).min((limit - 13) / 10)
                }
            }
            ObjectRequest::Info { .. } => 18,
            ObjectRequest::IdState { .. } => 39,
            _ => 4,
        }
    }
}
impl<R: ObjectRepository, S: ObjectSchema, const N: usize> ServiceHandler
    for ObjectHandler<'_, R, S, N>
{
    fn connection_closed(&mut self) -> Result<(), SessionError> {
        self.disconnect();
        Ok(())
    }
    fn must_end_session(&self) -> bool {
        !self.active_live()
    }
    fn required_features(&self, module: &ModuleEntry, opcode: u8) -> Option<u32> {
        (*module == self.metadata && matches!(opcode, 0x20..=0x28)).then_some(0)
    }
    fn authorized(&self, frame: Frame<'_>) -> bool {
        if !self.active_live() || frame.header.module != self.metadata.module {
            return false;
        }
        let operation = match frame.header.opcode {
            0x20..=0x22 | 0x24 | 0x26 => ObjectOperation::Write,
            0x25 => ObjectOperation::Enumerate,
            _ => ObjectOperation::Read,
        };
        if let Ok(request) = ObjectRequest::decode(frame.header.opcode, frame.payload) {
            // Fresh missing-D requests need the exact FF04 response. A previously
            // successful clear may never replay its cached OK after D revocation.
            if matches!(
                request,
                ObjectRequest::Delete {
                    object_id: u32::MAX,
                    ..
                }
            ) && self.clear_guard.get() == Some(frame.header.message_id)
                && !self.access.dangerous_allowed(self.metadata.module)
            {
                self.invalidate();
                return false;
            }
            self.preflight(request)
        } else {
            self.allowed(operation, None)
        }
    }
    fn dispatch(&mut self, frame: Frame<'_>, out: &mut [u8]) -> Result<usize, SessionError> {
        if !self.active_live() {
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        if frame.header.kind != Kind::Request
            || frame.header.message_id == 0
            || frame.header.offset != 0
            || usize::from(frame.header.total_length) != frame.payload.len()
            || frame.payload.len() > usize::from(self.metadata.input_max)
        {
            return Err(SessionError::InvalidFrame);
        }
        if out.len() < 4 {
            return Err(SessionError::Capacity);
        }
        if frame.header.module != self.metadata.module {
            return Ok(status(out, Status::UnsupportedModule, Reason::None));
        }
        if !matches!(frame.header.opcode, 0x20..=0x28) {
            return Ok(status(out, Status::UnsupportedOpcode, Reason::None));
        }
        let Ok(request) = ObjectRequest::decode(frame.header.opcode, frame.payload) else {
            if frame.header.opcode == 0x22
                && self
                    .transactions
                    .active
                    .get()
                    .is_some_and(|a| a.begin.module == self.metadata.module)
            {
                self.end_staging();
            }
            return Ok(status(out, Status::InvalidArgument, Reason::None));
        };
        let (operation, id) = self.scope(request);
        if !self.allowed(operation, id) {
            return Ok(status(out, Status::AccessDenied, Reason::RoleRequired));
        }
        if matches!(request, ObjectRequest::Enumerate { .. }) && !self.preflight(request) {
            if self.is_closed() {
                out.fill(0);
                return Err(SessionError::AuthorizationRevoked);
            }
            return Ok(status(out, Status::AccessDenied, Reason::RoleRequired));
        }
        if operation == ObjectOperation::Clear
            && !self.access.dangerous_allowed(self.metadata.module)
        {
            return Ok(status(
                out,
                Status::AccessDenied,
                Reason::DangerousScopeRequired,
            ));
        }
        let limit = out.len().min(usize::from(self.metadata.output_max));
        if limit < Self::needed(request, limit) {
            return Err(SessionError::Capacity);
        }
        let mut pending = [0; LOGICAL_MAX];
        let result = self.execute(request, &mut pending[..limit]);
        if !self.active_live()
            || !self.allowed(operation, id)
            || (operation == ObjectOperation::Clear
                && !self.access.dangerous_allowed(self.metadata.module))
            || (result == Err(DispatchFailure::Store(StoreError::Denied))
                && !matches!(request, ObjectRequest::Enumerate { .. }))
        {
            self.invalidate();
            out.fill(0);
            return Err(SessionError::AuthorizationRevoked);
        }
        let length = match result {
            Ok(n) => {
                if operation == ObjectOperation::Clear {
                    self.clear_guard.set(Some(frame.header.message_id));
                }
                n
            }
            Err(e) => {
                let (code, reason) = match e {
                    DispatchFailure::Contract => (Status::InternalFailure, Reason::None),
                    DispatchFailure::Store(error) => failure(error, frame.header.opcode == 0x22),
                };
                status(&mut pending, code, reason)
            }
        };
        out[..length].copy_from_slice(&pending[..length]);
        Ok(length)
    }
}
fn put16(out: &mut [u8], at: usize, v: u16) {
    out[at..at + 2].copy_from_slice(&v.to_le_bytes());
}
fn put32(out: &mut [u8], at: usize, v: u32) {
    out[at..at + 4].copy_from_slice(&v.to_le_bytes());
}
fn status(out: &mut [u8], code: Status, reason: Reason) -> usize {
    put16(out, 0, code as u16);
    put16(out, 2, reason as u16);
    4
}
fn failure(error: StoreError, incoming_commit: bool) -> (Status, Reason) {
    let code = match error {
        StoreError::Invalid
        | StoreError::Sequence
        | StoreError::Incomplete
        | StoreError::Schema => Status::InvalidArgument,
        StoreError::InvalidState => Status::InvalidState,
        StoreError::Busy => Status::Busy,
        StoreError::Denied => return (Status::AccessDenied, Reason::RoleRequired),
        StoreError::Conflict => Status::Conflict,
        StoreError::NotFound => Status::NotFound,
        StoreError::Capacity => Status::LimitExceeded,
        StoreError::Integrity => {
            return (
                Status::IntegrityFailure,
                if incoming_commit {
                    Reason::CrcMismatch
                } else {
                    Reason::StoredCrcMismatch
                },
            );
        }
        StoreError::Backend => Status::StorageFailure,
    };
    (code, Reason::None)
}
