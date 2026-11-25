use crate::{DurableObjects, Entry, owner::service_error};
use device_object_service::{
    Birth, DirectoryEntry, Enumeration, IdState, ObjectAccess, ObjectOperation, ObjectRepository,
    ReadResult,
};
use device_protocol::object_crc;
use device_storage::persistence::SnapshotStore;
use device_storage::{
    Authorization, Begin, Commit, ObjectMeta, ObjectSchema, ObjectStore, StoreError,
};
fn allowed(
    a: &dyn ObjectAccess,
    m: u8,
    op: ObjectOperation,
    id: Option<u32>,
) -> Result<(), StoreError> {
    if a.allowed(m, op, None) && id.is_none_or(|id| a.allowed(m, op, Some(id))) {
        Ok(())
    } else {
        Err(StoreError::Denied)
    }
}
fn meta<const B: usize>(e: &Entry<B>) -> ObjectMeta {
    ObjectMeta {
        object_id: e.begin.object_id,
        kind: e.begin.kind,
        revision: e.revision,
        total: e.total,
        crc: e.crc,
        birth_token: e.birth.token,
    }
}
fn validate_begin<const B: usize>(b: &Begin) -> Result<(), StoreError> {
    if b.object_id == 0 || b.object_id == u32::MAX || b.kind == 0 {
        return Err(StoreError::Invalid);
    }
    if usize::try_from(b.total).map_err(|_| StoreError::Capacity)? > B || b.total > 4096 {
        return Err(StoreError::Capacity);
    }
    if (b.create && (b.expected_revision != 0 || b.creation_token == [0; 16]))
        || (!b.create && (b.expected_revision == 0 || b.creation_token != [0; 16]))
    {
        return Err(StoreError::Invalid);
    }
    Ok(())
}
impl<
    S: SnapshotStore,
    V: ObjectSchema,
    const M: usize,
    const E: usize,
    const B: usize,
    const I: usize,
> ObjectStore for DurableObjects<S, V, M, E, B, I>
{
    fn reserve(&mut self, b: &Begin, auth: &dyn Authorization) -> Result<(), StoreError> {
        validate_begin::<B>(b)?;
        if !b.create {
            return Err(StoreError::Invalid);
        }
        let mut state = *self.ready()?;
        let mi = state.module(b.module)?;
        let mut module = state.modules[mi].ok_or(StoreError::Backend)?;
        if !auth.write_allowed(b.module) {
            return Err(StoreError::Denied);
        }
        if b.object_id <= module.high
            || state
                .entries
                .iter()
                .flatten()
                .any(|e| e.begin.creation_token == b.creation_token)
        {
            return Err(StoreError::Conflict);
        }
        let index = state
            .entries
            .iter()
            .position(Option::is_none)
            .ok_or(StoreError::Capacity)?;
        state.entries[index] = Some(Entry {
            begin: *b,
            birth: Birth::default(),
            revision: 0,
            total: 0,
            crc: 0,
            alive: false,
            body: [0; B],
        });
        module.high = b.object_id;
        state.modules[mi] = Some(module);
        if !auth.write_allowed(b.module) {
            return Err(StoreError::Denied);
        }
        self.persist(state).map_err(service_error)
    }
    fn stat(&self, module: u8, id: u32) -> Result<ObjectMeta, StoreError> {
        let state = self.ready()?;
        state.module(module)?;
        let entry = state.entries[state.entry(module, id)?]
            .as_ref()
            .ok_or(StoreError::Backend)?;
        if !entry.alive {
            return Err(StoreError::NotFound);
        }
        Ok(meta(entry))
    }
    fn commit(
        &mut self,
        c: &Commit,
        body: &[u8],
        auth: &dyn Authorization,
    ) -> Result<ObjectMeta, StoreError> {
        let b = c.begin;
        validate_begin::<B>(&b)?;
        let mut state = *self.ready()?;
        let mi = state.module(b.module)?;
        if !auth.write_allowed(b.module) {
            return Err(StoreError::Denied);
        }
        if usize::try_from(b.total).map_err(|_| StoreError::Capacity)? != body.len() {
            return Err(StoreError::Incomplete);
        }
        if object_crc(b.module, b.kind, body).map_err(|_| StoreError::Integrity)? != c.crc {
            return Err(StoreError::Integrity);
        }
        self.schema.validate(b.module, b.kind, body)?;
        let index = state
            .entry(b.module, b.object_id)
            .map_err(|_| StoreError::Conflict)?;
        let mut e = state.entries[index].ok_or(StoreError::Backend)?;
        if b.create {
            if e.begin != b || e.birth != Birth::default() || e.alive {
                return Err(StoreError::Conflict);
            }
            e.birth = Birth {
                kind: b.kind,
                total: b.total,
                crc: c.crc,
                revision: 1,
                token: b.creation_token,
            };
            e.revision = 1;
        } else {
            if !e.alive || e.revision != b.expected_revision || e.begin.kind != b.kind {
                return Err(StoreError::Conflict);
            }
            e.revision = e.revision.checked_add(1).ok_or(StoreError::Capacity)?;
        }
        e.body.fill(0);
        e.body[..body.len()].copy_from_slice(body);
        e.total = b.total;
        e.crc = c.crc;
        e.alive = true;
        let result = meta(&e);
        state.entries[index] = Some(e);
        let mut m = state.modules[mi].ok_or(StoreError::Backend)?;
        m.revision = m.revision.checked_add(1).ok_or(StoreError::Capacity)?;
        state.modules[mi] = Some(m);
        if !auth.write_allowed(b.module) {
            return Err(StoreError::Denied);
        }
        self.persist(state).map_err(service_error)?;
        Ok(result)
    }
}
impl<
    S: SnapshotStore,
    V: ObjectSchema,
    const M: usize,
    const E: usize,
    const B: usize,
    const I: usize,
> ObjectRepository for DurableObjects<S, V, M, E, B, I>
{
    fn read_range(
        &self,
        module: u8,
        id: u32,
        expected: u32,
        offset: u32,
        out: &mut [u8],
        access: &dyn ObjectAccess,
    ) -> Result<ReadResult, StoreError> {
        let state = self.ready()?;
        state.module(module)?;
        allowed(access, module, ObjectOperation::Read, Some(id))?;
        let entry = state.entries[state.entry(module, id)?]
            .as_ref()
            .ok_or(StoreError::Backend)?;
        if !entry.alive {
            return Err(StoreError::NotFound);
        }
        if entry.revision != expected {
            return Err(StoreError::Conflict);
        }
        let offset = usize::try_from(offset).map_err(|_| StoreError::Invalid)?;
        let total = usize::try_from(entry.total).map_err(|_| StoreError::Backend)?;
        if offset > total {
            return Err(StoreError::Invalid);
        }
        let length = out.len().min(total - offset);
        allowed(access, module, ObjectOperation::Read, Some(id))?;
        out[..length].copy_from_slice(&entry.body[offset..offset + length]);
        Ok(ReadResult {
            meta: meta(entry),
            length,
        })
    }
    fn delete_atomic(
        &mut self,
        module: u8,
        id: u32,
        expected: u32,
        access: &dyn ObjectAccess,
    ) -> Result<(), StoreError> {
        let mut state = *self.ready()?;
        let mi = state.module(module)?;
        let mut m = state.modules[mi].ok_or(StoreError::Backend)?;
        let op = if id == u32::MAX {
            ObjectOperation::Clear
        } else {
            ObjectOperation::Write
        };
        allowed(access, module, op, Some(id))?;
        if id == u32::MAX {
            if !access.dangerous_allowed(module) {
                return Err(StoreError::Denied);
            }
            if m.revision != expected {
                return Err(StoreError::Conflict);
            }
            for e in state
                .entries
                .iter_mut()
                .flatten()
                .filter(|e| e.begin.module == module)
            {
                erase(e);
            }
        } else {
            let index = state.entry(module, id)?;
            let e = state.entries[index].as_mut().ok_or(StoreError::Backend)?;
            if !e.alive {
                return Err(StoreError::NotFound);
            }
            if e.revision != expected {
                return Err(StoreError::Conflict);
            }
            erase(e);
        }
        m.revision = m.revision.checked_add(1).ok_or(StoreError::Capacity)?;
        state.modules[mi] = Some(m);
        allowed(access, module, op, Some(id))?;
        if id == u32::MAX && !access.dangerous_allowed(module) {
            return Err(StoreError::Denied);
        }
        self.persist(state).map_err(service_error)
    }
    fn enumerate(
        &self,
        module: u8,
        start: u32,
        out: &mut [DirectoryEntry],
        access: &dyn ObjectAccess,
    ) -> Result<Enumeration, StoreError> {
        let state = self.ready()?;
        let mi = state.module(module)?;
        allowed(access, module, ObjectOperation::Enumerate, None)?;
        let mut count = 0;
        let mut next = u32::MAX;
        let mut previous = None;
        loop {
            let entry = state
                .entries
                .iter()
                .flatten()
                .filter(|e| {
                    e.alive
                        && e.begin.module == module
                        && e.begin.object_id >= start
                        && previous.is_none_or(|p| e.begin.object_id > p)
                })
                .min_by_key(|e| e.begin.object_id);
            let Some(e) = entry else { break };
            allowed(
                access,
                module,
                ObjectOperation::Enumerate,
                Some(e.begin.object_id),
            )?;
            if count == out.len() {
                next = e.begin.object_id;
                break;
            }
            count += 1;
            previous = Some(e.begin.object_id);
        }
        if count == 0 && next != u32::MAX {
            return Err(StoreError::Capacity);
        }
        // Authorize the complete page and continuation before exposing any metadata.
        allowed(access, module, ObjectOperation::Enumerate, None)?;
        previous = None;
        for slot in &mut out[..count] {
            let e = state
                .entries
                .iter()
                .flatten()
                .filter(|e| {
                    e.alive
                        && e.begin.module == module
                        && e.begin.object_id >= start
                        && previous.is_none_or(|p| e.begin.object_id > p)
                })
                .min_by_key(|e| e.begin.object_id)
                .ok_or(StoreError::Backend)?;
            *slot = DirectoryEntry {
                id: e.begin.object_id,
                kind: e.begin.kind,
                total: e.total,
            };
            previous = Some(e.begin.object_id);
        }
        Ok(Enumeration {
            revision: state.modules[mi].ok_or(StoreError::Backend)?.revision,
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
        let state = self.ready()?;
        if id == 0 || id == u32::MAX {
            return Err(StoreError::Invalid);
        }
        let mi = state.module(module)?;
        allowed(access, module, ObjectOperation::Read, Some(id))?;
        let high = state.modules[mi].ok_or(StoreError::Backend)?.high;
        let entry = state
            .entries
            .iter()
            .flatten()
            .find(|e| e.begin.module == module && e.begin.object_id == id && e.alive);
        Ok(IdState {
            state: if entry.is_some() {
                1
            } else if id <= high {
                2
            } else {
                0
            },
            high_water: high,
            birth: entry.map_or(Birth::default(), |e| e.birth),
        })
    }
}
fn erase<const B: usize>(e: &mut Entry<B>) {
    e.alive = false;
    e.revision = 0;
    e.total = 0;
    e.crc = 0;
    e.body.fill(0);
}
