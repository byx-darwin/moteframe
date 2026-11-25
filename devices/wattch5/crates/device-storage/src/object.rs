//! Object transaction engine and mandatory durable backend contracts.
use device_protocol::object_crc;
/// Portable storage/service failures, mapped to wire status by the service layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreError {
    /// Invalid identity, length or creation fields.
    Invalid,
    /// No transaction or wrong transaction identifier.
    InvalidState,
    /// A transaction is already active.
    Busy,
    /// Caller no longer has write scope.
    Denied,
    /// Revision or stable identity conflicts.
    Conflict,
    /// Object does not exist.
    NotFound,
    /// Storage or buffer capacity is exhausted.
    Capacity,
    /// Write offset is not the accepted contiguous prefix.
    Sequence,
    /// Not every declared byte has arrived.
    Incomplete,
    /// Object CRC mismatches.
    Integrity,
    /// Object body violates its module schema.
    Schema,
    /// Backend failed; caller must query stable ID before retrying an uncertain commit.
    Backend,
}
/// BEGIN fields, independent of a physical medium.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Begin {
    /// Owning module.
    pub module: u8,
    /// Module-local object type.
    pub kind: u16,
    /// Stable client-selected ID, 1..FFFFFFFE.
    pub object_id: u32,
    /// Zero on creation; current nonzero revision on edit.
    pub expected_revision: u32,
    /// Complete encoded object length.
    pub total: u32,
    /// Whether this reserves a never-used ID.
    pub create: bool,
    /// Nonzero globally unique creation token; zero on edit.
    pub creation_token: [u8; 16],
}
impl core::fmt::Debug for Begin {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Begin")
            .field("module", &self.module)
            .field("kind", &self.kind)
            .field("object_id", &self.object_id)
            .field("expected_revision", &self.expected_revision)
            .field("total", &self.total)
            .field("create", &self.create)
            .finish_non_exhaustive()
    }
}
/// Committed metadata. Birth fields must survive edits and tombstones.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ObjectMeta {
    /// Stable object identifier.
    pub object_id: u32,
    /// Immutable object type.
    pub kind: u16,
    /// Current nonzero revision.
    pub revision: u32,
    /// Encoded length.
    pub total: u32,
    /// Domain-separated WOBJ CRC.
    pub crc: u32,
    /// Original creation token, never replaced by an edit's zero token.
    pub birth_token: [u8; 16],
}
impl core::fmt::Debug for ObjectMeta {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ObjectMeta")
            .field("object_id", &self.object_id)
            .field("kind", &self.kind)
            .field("revision", &self.revision)
            .field("total", &self.total)
            .finish_non_exhaustive()
    }
}
/// Fully received and validated commit parameters.
#[derive(Debug, Clone, Copy)]
pub struct Commit {
    /// Original transaction identity and revision expectation.
    pub begin: Begin,
    /// Validated WOBJ integrity value.
    pub crc: u32,
}
/// Trusted authorization source; never derive this from an unauthenticated request field.
pub trait Authorization {
    /// Recheck current write permission for this module.
    fn write_allowed(&self, module: u8) -> bool;
}
/// Module-specific validation, separate from transport and Flash.
pub trait ObjectSchema {
    /// Validate the complete logical body and its immutable type.
    /// # Errors
    /// Returns schema error or explicit unavailable schema; never default to accepting unknown types.
    fn validate(&self, module: u8, kind: u16, body: &[u8]) -> Result<(), StoreError>;
}
/// Durable object operations. Implementations must uphold these atomicity contracts.
/// There is deliberately no always-successful production memory backend.
pub trait ObjectStore {
    /// Atomically persist high-water advance and unique birth reservation before returning.
    /// Every skipped ID is consumed permanently; abort/delete never reduce the watermark.
    /// Recheck live authorization inside the same serialized durable reservation boundary,
    /// before consuming any ID; an earlier authorization snapshot is insufficient.
    /// Duplicate/reused tokens or IDs at/below the high water must conflict, across resets too.
    /// # Errors
    /// Backend, capacity, token/ID conflict. An uncertain outcome must be queried by stable identity.
    fn reserve(
        &mut self,
        begin: &Begin,
        authorization: &dyn Authorization,
    ) -> Result<(), StoreError>;
    /// Fetch current existing object metadata.
    /// # Errors
    /// Not found or backend failure.
    fn stat(&self, module: u8, id: u32) -> Result<ObjectMeta, StoreError>;
    /// Atomically compare revision/existence/reservation and replace all bytes and metadata.
    /// Creation must match its durable reservation. Edits preserve immutable birth fields.
    /// No partial write may become visible; revision increments must not wrap.
    /// Concurrent invalidation must fail rather than overwrite a newer UI/peer edit.
    /// Authorization MUST be rechecked while holding the same serialized commit boundary;
    /// a prior boolean snapshot is insufficient. Revocation and commit cannot race through that boundary.
    /// # Errors
    /// Conflict, capacity or backend failure. Backend error may mean unknown outcome.
    fn commit(
        &mut self,
        commit: &Commit,
        body: &[u8],
        authorization: &dyn Authorization,
    ) -> Result<ObjectMeta, StoreError>;
}
/// One static staging buffer per session; no allocation or physical addresses.
pub struct ObjectEngine<const N: usize> {
    buffer: [u8; N],
    active: Option<(u32, Begin)>,
    used: usize,
    next_tx: u32,
}
impl<const N: usize> core::fmt::Debug for ObjectEngine<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ObjectEngine")
            .field("active", &self.active)
            .field("used", &self.used)
            .field("capacity", &N)
            .finish_non_exhaustive()
    }
}
impl<const N: usize> Default for ObjectEngine<N> {
    fn default() -> Self {
        Self::new()
    }
}
impl<const N: usize> ObjectEngine<N> {
    /// Create an idle object engine. Storage is supplied for each operation.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            buffer: [0; N],
            active: None,
            used: 0,
            next_tx: 1,
        }
    }
    /// Precheck an edit or durably reserve a creation. Precheck does not lock revisions.
    /// # Errors
    /// Rejects capacity, fields, authorization, conflicts or an already active transaction.
    pub fn begin(
        &mut self,
        begin: Begin,
        store: &mut impl ObjectStore,
        auth: &impl Authorization,
    ) -> Result<u32, StoreError> {
        if self.active.is_some() {
            return Err(StoreError::Busy);
        }
        if !auth.write_allowed(begin.module) {
            return Err(StoreError::Denied);
        }
        if begin.object_id == 0 || begin.object_id == u32::MAX || begin.kind == 0 {
            return Err(StoreError::Invalid);
        }
        let total = usize::try_from(begin.total).map_err(|_| StoreError::Capacity)?;
        if total > N || total > 4096 {
            return Err(StoreError::Capacity);
        }
        if begin.create {
            if begin.expected_revision != 0 || begin.creation_token == [0; 16] {
                return Err(StoreError::Invalid);
            }
        } else {
            if begin.expected_revision == 0 || begin.creation_token != [0; 16] {
                return Err(StoreError::Invalid);
            }
            let current = store.stat(begin.module, begin.object_id)?;
            if current.revision != begin.expected_revision || current.kind != begin.kind {
                return Err(StoreError::Conflict);
            }
        }
        let tx = self.next_tx;
        let next = tx.checked_add(1).ok_or(StoreError::Capacity)?;
        if begin.create {
            store.reserve(&begin, auth)?;
        }
        self.next_tx = next;
        self.active = Some((tx, begin));
        self.used = 0;
        Ok(tx)
    }
    /// Append the next contiguous object bytes, returning accepted prefix length.
    /// # Errors
    /// Rejects wrong transaction, gaps/overlap, empty chunks or excess bytes.
    pub fn write(&mut self, tx: u32, offset: u32, bytes: &[u8]) -> Result<u32, StoreError> {
        let begin = self.transaction(tx)?;
        if usize::try_from(offset).map_err(|_| StoreError::Sequence)? != self.used {
            return Err(StoreError::Sequence);
        }
        let end = self
            .used
            .checked_add(bytes.len())
            .ok_or(StoreError::Capacity)?;
        if bytes.is_empty()
            || end > N
            || end > usize::try_from(begin.total).map_err(|_| StoreError::Capacity)?
        {
            return Err(StoreError::Capacity);
        }
        self.buffer[self.used..end].copy_from_slice(bytes);
        self.used = end;
        u32::try_from(end).map_err(|_| StoreError::Capacity)
    }
    /// Validate full length, CRC, schema and fresh authorization, then atomically CAS in storage.
    /// # Errors
    /// Returns incomplete/integrity/schema/denied/conflict or backend failure; no fake completion.
    pub fn commit(
        &mut self,
        tx: u32,
        crc: u32,
        store: &mut impl ObjectStore,
        schema: &impl ObjectSchema,
        auth: &impl Authorization,
    ) -> Result<ObjectMeta, StoreError> {
        let begin = self.transaction(tx)?;
        if !auth.write_allowed(begin.module) {
            self.clear();
            return Err(StoreError::Denied);
        }
        if u32::try_from(self.used).map_err(|_| StoreError::Capacity)? != begin.total {
            return Err(StoreError::Incomplete);
        }
        let body = &self.buffer[..self.used];
        if object_crc(begin.module, begin.kind, body).map_err(|_| StoreError::Integrity)? != crc {
            return Err(StoreError::Integrity);
        }
        if let Err(error) = schema.validate(begin.module, begin.kind, body) {
            self.clear();
            return Err(error);
        }
        if !auth.write_allowed(begin.module) {
            self.clear();
            return Err(StoreError::Denied);
        }
        let result = store.commit(&Commit { begin, crc }, &self.buffer[..self.used], auth);
        // A commit conflict or unknown backend outcome ends staging too; query stable identity.
        self.clear();
        result
    }
    /// Discard staging bytes. Durable ID reservations remain consumed.
    /// # Errors
    /// Returns invalid state for a different or absent transaction.
    pub fn abort(&mut self, tx: u32) -> Result<(), StoreError> {
        self.transaction(tx)?;
        self.clear();
        Ok(())
    }
    /// Discard all staging immediately when a trusted authorization source revokes scope.
    pub fn authorization_revoked(&mut self) {
        self.clear();
    }
    /// Discard session staging at disconnect; never undo persistent watermarks.
    pub fn disconnect(&mut self) {
        self.clear();
    }
    fn clear(&mut self) {
        self.buffer[..self.used].fill(0);
        self.active = None;
        self.used = 0;
    }
    fn transaction(&self, tx: u32) -> Result<Begin, StoreError> {
        match self.active {
            Some((id, begin)) if id == tx => Ok(begin),
            _ => Err(StoreError::InvalidState),
        }
    }
}
