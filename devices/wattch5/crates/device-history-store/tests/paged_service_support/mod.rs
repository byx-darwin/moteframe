//! Logical NOR fixture below the actual paged owner and `AtomicSnapshot`.
use core::cell::{Cell, RefCell};
use device_history_service::{HistoryAccess, HistoryFailure, HistoryOperation};
use device_history_store::paged::{
    LogConfig, MediaLayout, MediaView, PagedHistoryStore, PagedMedia,
};
use device_history_store::{AppendRecord, DomainConfig, HistoryClock, HistorySchema};
use device_protocol::HistoryFilter;
use device_storage::{
    StoredRecord,
    persistence::{AtomicSnapshot, Error, Geometry, Layout, Medium},
};
use std::rc::Rc;

#[derive(Clone, Debug)]
pub struct MediumControl {
    bytes: Rc<RefCell<Vec<u8>>>,
    pub syncs: Rc<Cell<u32>>,
    pub writes: Rc<Cell<u32>>,
    pub fail_sync: Rc<Cell<Option<u32>>>,
}
impl MediumControl {
    pub fn blank() -> Self {
        Self {
            bytes: Rc::new(RefCell::new(vec![0xff; 65_536])),
            syncs: Rc::new(Cell::new(0)),
            writes: Rc::new(Cell::new(0)),
            fail_sync: Rc::new(Cell::new(None)),
        }
    }
    pub fn media(&self) -> PagedMedia<Self> {
        PagedMedia::new(
            self.clone(),
            MediaLayout {
                metadata: metadata_layout(),
                log_start: 8192,
                log_bytes: 57_344,
            },
        )
        .unwrap()
    }
}
impl Medium for MediumControl {
    fn geometry(&self) -> Geometry {
        Geometry {
            capacity: 65_536,
            erase_size: 256,
            write_size: 8,
        }
    }
    fn read(&mut self, offset: u32, out: &mut [u8]) -> Result<(), Error> {
        let start = usize::try_from(offset).map_err(|_| Error::Io)?;
        let end = start.checked_add(out.len()).ok_or(Error::Io)?;
        out.copy_from_slice(self.bytes.borrow().get(start..end).ok_or(Error::Io)?);
        Ok(())
    }
    fn erase(&mut self, offset: u32, length: u32) -> Result<(), Error> {
        if !offset.is_multiple_of(256) || !length.is_multiple_of(256) {
            return Err(Error::Io);
        }
        let end = offset.checked_add(length).ok_or(Error::Io)?;
        self.bytes
            .borrow_mut()
            .get_mut(offset as usize..end as usize)
            .ok_or(Error::Io)?
            .fill(0xff);
        self.writes.set(self.writes.get() + 1);
        Ok(())
    }
    fn program(&mut self, offset: u32, input: &[u8]) -> Result<(), Error> {
        if !offset.is_multiple_of(8) || !input.len().is_multiple_of(8) {
            return Err(Error::Io);
        }
        let start = usize::try_from(offset).map_err(|_| Error::Io)?;
        let end = start.checked_add(input.len()).ok_or(Error::Io)?;
        let mut bytes = self.bytes.borrow_mut();
        for (stored, new) in bytes
            .get_mut(start..end)
            .ok_or(Error::Io)?
            .iter_mut()
            .zip(input)
        {
            if *stored & *new != *new {
                return Err(Error::Io);
            }
            *stored &= new;
        }
        self.writes.set(self.writes.get() + 1);
        Ok(())
    }
    fn sync(&mut self) -> Result<(), Error> {
        let next = self.syncs.get() + 1;
        self.syncs.set(next);
        if self.fail_sync.get() == Some(next) {
            self.fail_sync.set(None);
            return Err(Error::Io);
        }
        Ok(())
    }
}
#[derive(Clone, Debug)]
pub struct Clock(pub Rc<Cell<Option<u64>>>);
impl Clock {
    pub fn new() -> Self {
        Self(Rc::new(Cell::new(Some(0))))
    }
    pub fn set(&self, time: u64) {
        self.0.set(Some(time));
    }
}
impl HistoryClock for Clock {
    fn now_ms(&self) -> Result<u64, HistoryFailure> {
        self.0.get().ok_or(HistoryFailure::Clock)
    }
}
#[derive(Clone, Copy, Debug)]
pub struct Schema;
impl HistorySchema for Schema {
    fn validate(&self, record: StoredRecord<'_>, parent: u64) -> Result<(), HistoryFailure> {
        if record.domain == 0x20
            && [0x20, 0x21].contains(&record.record_type)
            && record.schema == 1
            && record.payload.len() == 4
            && record.payload[2..] == [0, 1]
            && parent == u64::from(record.payload[1])
        {
            Ok(())
        } else {
            Err(HistoryFailure::StoredIntegrity)
        }
    }
}
#[derive(Debug)]
pub struct Access;
impl HistoryAccess for Access {
    fn module_allowed(&self) -> bool {
        true
    }
    fn allowed(&self, _: [u8; 16], _: Option<u8>, _: HistoryOperation) -> bool {
        true
    }
}
pub type Store<'a> = PagedHistoryStore<
    AtomicSnapshot<MediaView<'a, MediumControl>, 512>,
    MediaView<'a, MediumControl>,
    Schema,
    Clock,
    1,
    255,
    16,
    512,
>;
pub fn metadata_layout() -> Layout {
    Layout {
        start: 0,
        bank_bytes: 4096,
    }
}
pub fn config() -> LogConfig {
    LogConfig {
        namespace: [0x71; 16],
        log_start: 8192,
        slots_per_domain: 256,
    }
}
pub fn filter() -> HistoryFilter {
    HistoryFilter {
        domain: 0x20,
        filter_flags: 0,
        start_utc_ms: 0,
        end_utc_ms: 0,
        record_type: 0,
        parent_session: 0,
    }
}
pub fn input(body: &[u8]) -> AppendRecord<'_> {
    AppendRecord {
        domain: 0x20,
        record_type: 0x20,
        schema: 1,
        utc_ms: 1000,
        flags: 1,
        parent_session: u64::from(body[1]),
        payload: body,
    }
}
pub fn initialize(media: &mut PagedMedia<MediumControl>, clock: Clock) -> Store<'_> {
    let (meta, log) = media.split();
    PagedHistoryStore::initialize(
        AtomicSnapshot::open(meta, metadata_layout()).unwrap(),
        log,
        Schema,
        clock,
        config(),
        [7; 16],
        &[DomainConfig {
            domain: 0x20,
            epoch: 3,
        }],
        60,
    )
    .unwrap()
}
pub fn reopen(media: &mut PagedMedia<MediumControl>, clock: Clock) -> Store<'_> {
    let (meta, log) = media.split();
    PagedHistoryStore::open(
        AtomicSnapshot::open(meta, metadata_layout()).unwrap(),
        log,
        Schema,
        clock,
        config(),
        60,
    )
    .unwrap()
}
