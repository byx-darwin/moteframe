use crate::{Entry, Module, State, owner::Error};
use device_object_service::Birth;
use device_protocol::{Crc32c, object_crc};
use device_storage::persistence::Error as StorageError;
use device_storage::{Begin, ObjectSchema};
const MAGIC: &[u8; 8] = b"DOBJ0001";
fn crc(bytes: &[u8]) -> u32 {
    let mut c = Crc32c::new();
    c.update(bytes);
    c.finish()
}
struct Writer<'a> {
    bytes: &'a mut [u8],
    at: usize,
}
impl Writer<'_> {
    fn put(&mut self, b: &[u8]) -> Result<(), Error> {
        let end = self
            .at
            .checked_add(b.len())
            .ok_or(Error::Storage(StorageError::Capacity))?;
        self.bytes
            .get_mut(self.at..end)
            .ok_or(Error::Storage(StorageError::Capacity))?
            .copy_from_slice(b);
        self.at = end;
        Ok(())
    }
    fn u32(&mut self, v: u32) -> Result<(), Error> {
        self.put(&v.to_le_bytes())
    }
}
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}
impl Reader<'_> {
    fn take<const N: usize>(&mut self) -> Result<[u8; N], Error> {
        let end = self.at.checked_add(N).ok_or(Error::Integrity)?;
        let b = self.bytes.get(self.at..end).ok_or(Error::Integrity)?;
        self.at = end;
        b.try_into().map_err(|_| Error::Integrity)
    }
    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.take()?))
    }
}
pub(crate) fn encode<const M: usize, const E: usize, const B: usize>(
    state: &State<M, E, B>,
    revision: u64,
    out: &mut [u8],
) -> Result<usize, Error> {
    let mut w = Writer { bytes: out, at: 0 };
    w.put(MAGIC)?;
    for n in [M, E, B] {
        w.u32(u32::try_from(n).map_err(|_| Error::Invalid)?)?;
    }
    w.put(&revision.to_le_bytes())?;
    w.u32(u32::try_from(state.modules.iter().flatten().count()).map_err(|_| Error::Invalid)?)?;
    w.u32(u32::try_from(state.entries.iter().flatten().count()).map_err(|_| Error::Invalid)?)?;
    for m in state.modules.iter().flatten() {
        w.put(&[m.id])?;
        w.u32(m.high)?;
        w.u32(m.revision)?;
    }
    // Modules and IDs define the canonical ordering, independent of reservation order.
    for module in state.modules.iter().flatten() {
        let mut previous = 0;
        loop {
            let next = state
                .entries
                .iter()
                .flatten()
                .filter(|e| e.begin.module == module.id && e.begin.object_id > previous)
                .min_by_key(|e| e.begin.object_id);
            let Some(e) = next else { break };
            write_entry(&mut w, e)?;
            previous = e.begin.object_id;
        }
    }
    let check = crc(&w.bytes[..w.at]);
    w.u32(check)?;
    Ok(w.at)
}
fn write_entry<const B: usize>(w: &mut Writer<'_>, e: &Entry<B>) -> Result<(), Error> {
    let b = e.begin;
    w.put(&[b.module])?;
    w.put(&b.kind.to_le_bytes())?;
    w.u32(b.object_id)?;
    w.u32(b.expected_revision)?;
    w.u32(b.total)?;
    w.put(&[u8::from(b.create)])?;
    w.put(&b.creation_token)?;
    let b = e.birth;
    w.put(&b.kind.to_le_bytes())?;
    w.u32(b.total)?;
    w.u32(b.crc)?;
    w.u32(b.revision)?;
    w.put(&b.token)?;
    w.put(&[u8::from(e.alive)])?;
    w.u32(e.revision)?;
    w.u32(e.total)?;
    w.u32(e.crc)?;
    if e.alive {
        let length = usize::try_from(e.total).map_err(|_| Error::Integrity)?;
        w.put(e.body.get(..length).ok_or(Error::Integrity)?)?;
    }
    Ok(())
}
pub(crate) fn decode<V: ObjectSchema, const M: usize, const E: usize, const B: usize>(
    bytes: &[u8],
    revision: u64,
    schema: &V,
) -> Result<State<M, E, B>, Error> {
    if bytes.len() < 40 {
        return Err(Error::Integrity);
    }
    let split = bytes.len() - 4;
    let check = u32::from_le_bytes(bytes[split..].try_into().map_err(|_| Error::Integrity)?);
    if crc(&bytes[..split]) != check {
        return Err(Error::Integrity);
    }
    let mut r = Reader {
        bytes: &bytes[..split],
        at: 0,
    };
    if &r.take::<8>()? != MAGIC {
        return Err(Error::Integrity);
    }
    for n in [M, E, B] {
        if usize::try_from(r.u32()?).map_err(|_| Error::Integrity)? != n {
            return Err(Error::Integrity);
        }
    }
    if u64::from_le_bytes(r.take()?) != revision {
        return Err(Error::Integrity);
    }
    let mc = usize::try_from(r.u32()?).map_err(|_| Error::Integrity)?;
    let ec = usize::try_from(r.u32()?).map_err(|_| Error::Integrity)?;
    if mc == 0 || mc > M || ec > E {
        return Err(Error::Integrity);
    }
    let mut s = State::empty();
    let mut previous = 0;
    for slot in &mut s.modules[..mc] {
        let id = r.take::<1>()?[0];
        let high = r.u32()?;
        let rev = r.u32()?;
        if id <= previous || high == u32::MAX || rev == 0 {
            return Err(Error::Integrity);
        }
        *slot = Some(Module {
            id,
            high,
            revision: rev,
        });
        previous = id;
    }
    let mut previous = (0, 0);
    for index in 0..ec {
        let e = read_entry::<B>(&mut r)?;
        let key = (e.begin.module, e.begin.object_id);
        if key <= previous {
            return Err(Error::Integrity);
        }
        previous = key;
        let mi = s.module(e.begin.module).map_err(|_| Error::Integrity)?;
        let m = s.modules[mi].ok_or(Error::Integrity)?;
        if e.begin.object_id > m.high
            || s.entries[..index]
                .iter()
                .flatten()
                .any(|p| p.begin.creation_token == e.begin.creation_token)
        {
            return Err(Error::Integrity);
        }
        validate_entry(&e, schema)?;
        s.entries[index] = Some(e);
    }
    if r.at != split {
        return Err(Error::Integrity);
    }
    validate_counters(&s, revision)?;
    Ok(s)
}
fn validate_counters<const M: usize, const E: usize, const B: usize>(
    state: &State<M, E, B>,
    revision: u64,
) -> Result<(), Error> {
    // Initialization, every permanent reservation, and every collection mutation
    // each publish exactly one snapshot. v1 has no other durable operations.
    let reservations =
        u64::try_from(state.entries.iter().flatten().count()).map_err(|_| Error::Integrity)?;
    let mut exact_revision = 1u64.checked_add(reservations).ok_or(Error::Integrity)?;
    for module in state.modules.iter().flatten() {
        let entries = state
            .entries
            .iter()
            .flatten()
            .filter(|e| e.begin.module == module.id);
        if entries
            .clone()
            .map(|e| e.begin.object_id)
            .max()
            .unwrap_or(0)
            != module.high
        {
            return Err(Error::Integrity);
        }
        let mut minimum_collection = 1u64;
        let mut removed = false;
        for entry in entries {
            let commits = if entry.alive {
                u64::from(entry.revision)
            } else if entry.birth != Birth::default() {
                removed = true;
                1
            } else {
                0
            };
            minimum_collection = minimum_collection
                .checked_add(commits)
                .ok_or(Error::Integrity)?;
        }
        // One Clear may remove all tombstones; their erased edit revisions are
        // unknown. At least one removal publication is nevertheless necessary.
        minimum_collection = minimum_collection
            .checked_add(u64::from(removed))
            .ok_or(Error::Integrity)?;
        if u64::from(module.revision) < minimum_collection {
            return Err(Error::Integrity);
        }
        exact_revision = exact_revision
            .checked_add(u64::from(module.revision) - 1)
            .ok_or(Error::Integrity)?;
    }
    if exact_revision != revision {
        return Err(Error::Integrity);
    }
    Ok(())
}
fn read_entry<const B: usize>(r: &mut Reader<'_>) -> Result<Entry<B>, Error> {
    let begin = Begin {
        module: r.take::<1>()?[0],
        kind: u16::from_le_bytes(r.take()?),
        object_id: r.u32()?,
        expected_revision: r.u32()?,
        total: r.u32()?,
        create: match r.take::<1>()?[0] {
            1 => true,
            _ => return Err(Error::Integrity),
        },
        creation_token: r.take()?,
    };
    let birth = Birth {
        kind: u16::from_le_bytes(r.take()?),
        total: r.u32()?,
        crc: r.u32()?,
        revision: r.u32()?,
        token: r.take()?,
    };
    let alive = match r.take::<1>()?[0] {
        0 => false,
        1 => true,
        _ => return Err(Error::Integrity),
    };
    let revision = r.u32()?;
    let total = r.u32()?;
    let crc = r.u32()?;
    let mut body = [0; B];
    if alive {
        let length = usize::try_from(total).map_err(|_| Error::Integrity)?;
        let end = r.at.checked_add(length).ok_or(Error::Integrity)?;
        body.get_mut(..length)
            .ok_or(Error::Integrity)?
            .copy_from_slice(r.bytes.get(r.at..end).ok_or(Error::Integrity)?);
        r.at = end;
    }
    Ok(Entry {
        begin,
        birth,
        revision,
        total,
        crc,
        alive,
        body,
    })
}
fn validate_entry<V: ObjectSchema, const B: usize>(e: &Entry<B>, schema: &V) -> Result<(), Error> {
    let b = e.begin;
    if b.object_id == 0
        || b.object_id == u32::MAX
        || b.kind == 0
        || b.expected_revision != 0
        || b.creation_token == [0; 16]
        || usize::try_from(b.total).map_err(|_| Error::Integrity)? > B
    {
        return Err(Error::Integrity);
    }
    if e.birth != Birth::default()
        && (e.birth.kind != b.kind
            || e.birth.total != b.total
            || e.birth.revision != 1
            || e.birth.token != b.creation_token)
    {
        return Err(Error::Integrity);
    }
    if !e.alive {
        if e.revision != 0 || e.total != 0 || e.crc != 0 {
            return Err(Error::Integrity);
        }
        return Ok(());
    }
    if e.birth == Birth::default() || e.revision == 0 {
        return Err(Error::Integrity);
    }
    let body = e
        .body
        .get(..usize::try_from(e.total).map_err(|_| Error::Integrity)?)
        .ok_or(Error::Integrity)?;
    if object_crc(b.module, b.kind, body).map_err(|_| Error::Integrity)? != e.crc {
        return Err(Error::Integrity);
    }
    schema
        .validate(b.module, b.kind, body)
        .map_err(|_| Error::Integrity)?;
    if e.revision == 1 && (e.total != e.birth.total || e.crc != e.birth.crc) {
        return Err(Error::Integrity);
    }
    Ok(())
}

impl core::fmt::Debug for Reader<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Reader")
            .field("length", &self.bytes.len())
            .field("position", &self.at)
            .finish()
    }
}
impl core::fmt::Debug for Writer<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Writer")
            .field("capacity", &self.bytes.len())
            .field("position", &self.at)
            .finish()
    }
}
