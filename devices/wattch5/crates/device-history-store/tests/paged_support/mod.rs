use device_history_service::{HistoryAccess, HistoryFailure, HistoryOperation};
use device_history_store::{
    AppendRecord, HistoryClock, HistorySchema,
    paged::{LogConfig, MediaLayout},
};
use device_protocol::HistoryFilter;
use device_storage::{
    StoredRecord,
    persistence::{Error, Geometry, Layout, Medium},
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
#[derive(Clone, Debug)]
pub struct Mem {
    pub bytes: Rc<RefCell<Vec<u8>>>,
    pub fail: Rc<Cell<Option<usize>>>,
    pub calls: Rc<Cell<usize>>,
    pub byte_cut: Rc<Cell<Option<usize>>>,
    pub geometry: Rc<Cell<Geometry>>,
    pub durable: Rc<RefCell<Vec<u8>>>,
    pub buffered: Rc<Cell<bool>>,
    pub sync_failure: Rc<Cell<Option<(usize, usize)>>>,
    pub syncs: Rc<Cell<usize>>,
}
impl Mem {
    pub fn new(n: usize) -> Self {
        Self {
            bytes: Rc::new(RefCell::new(vec![255; n])),
            fail: Rc::new(Cell::new(None)),
            calls: Rc::new(Cell::new(0)),
            byte_cut: Rc::new(Cell::new(None)),
            geometry: Rc::new(Cell::new(Geometry {
                capacity: u32::try_from(n).unwrap_or_else(|e| panic!("{e:?}")),
                erase_size: 1,
                write_size: 1,
            })),
            durable: Rc::new(RefCell::new(vec![255; n])),
            buffered: Rc::new(Cell::new(false)),
            sync_failure: Rc::new(Cell::new(None)),
            syncs: Rc::new(Cell::new(0)),
        }
    }
    fn op(&self) -> Result<(), Error> {
        let n = self.calls.get();
        self.calls.set(n + 1);
        if self.fail.get() == Some(n) {
            Err(Error::Io)
        } else {
            Ok(())
        }
    }
}
impl Medium for Mem {
    fn geometry(&self) -> Geometry {
        self.geometry.get()
    }
    fn read(&mut self, o: u32, out: &mut [u8]) -> Result<(), Error> {
        self.op()?;
        if out.len() > 256 {
            return Err(Error::Io);
        }
        out.copy_from_slice(&self.bytes.borrow()[o as usize..o as usize + out.len()]);
        Ok(())
    }
    fn erase(&mut self, o: u32, n: u32) -> Result<(), Error> {
        self.op()?;
        self.bytes.borrow_mut()[o as usize..(o + n) as usize].fill(255);
        Ok(())
    }
    fn program(&mut self, o: u32, b: &[u8]) -> Result<(), Error> {
        self.op()?;
        if !o.is_multiple_of(self.geometry.get().write_size)
            || !u32::try_from(b.len())
                .map_err(|_| Error::Io)?
                .is_multiple_of(self.geometry.get().write_size)
            || b.len() > 256
        {
            return Err(Error::Io);
        }
        for (i, x) in b.iter().enumerate() {
            if self.byte_cut.get() == Some(0) {
                return Err(Error::Io);
            }
            if let Some(n) = self.byte_cut.get() {
                self.byte_cut.set(Some(n - 1));
            }
            self.bytes.borrow_mut()[o as usize + i] &= *x;
        }
        Ok(())
    }
    fn sync(&mut self) -> Result<(), Error> {
        self.op()?;
        let sync = self.syncs.get();
        self.syncs.set(sync + 1);
        if self.buffered.get() {
            let failure = self.sync_failure.get().filter(|(at, _)| *at == sync);
            let limit = failure.map_or(usize::MAX, |(_, n)| n);
            let visible = self.bytes.borrow();
            let mut durable = self.durable.borrow_mut();
            let mut changed = 0;
            for (a, b) in durable.iter_mut().zip(visible.iter()) {
                if *a != *b {
                    if changed < limit {
                        *a = *b;
                    }
                    changed += 1;
                }
            }
            if failure.is_some() {
                return Err(Error::Io);
            }
        }
        Ok(())
    }
}
#[derive(Debug)]
pub struct Schema;
impl HistorySchema for Schema {
    fn validate(&self, r: StoredRecord<'_>, p: u64) -> Result<(), HistoryFailure> {
        if r.schema == 1 && r.payload.len() == 16 && p == 1 {
            Ok(())
        } else {
            Err(HistoryFailure::Invalid)
        }
    }
}
#[derive(Debug, Clone)]
pub struct Clock(pub Rc<Cell<u64>>);
impl Clock {
    pub fn new() -> Self {
        Self(Rc::new(Cell::new(100)))
    }
}
impl HistoryClock for Clock {
    fn now_ms(&self) -> Result<u64, HistoryFailure> {
        Ok(self.0.get())
    }
}
pub struct Access;
impl HistoryAccess for Access {
    fn module_allowed(&self) -> bool {
        true
    }
    fn allowed(&self, _: [u8; 16], _: Option<u8>, _: HistoryOperation) -> bool {
        true
    }
}
pub fn layout(n: u32) -> MediaLayout {
    MediaLayout {
        metadata: Layout {
            start: 0,
            bank_bytes: 4096,
        },
        log_start: 8192,
        log_bytes: n - 8192,
    }
}
pub fn config(slots: u32) -> LogConfig {
    LogConfig {
        namespace: [9; 16],
        log_start: 8192,
        slots_per_domain: slots,
    }
}
pub fn record(domain: u8) -> AppendRecord<'static> {
    AppendRecord {
        domain,
        record_type: 1,
        schema: 1,
        utc_ms: 0,
        flags: 0,
        parent_session: 1,
        payload: &[1; 16],
    }
}
pub fn filter(domain: u8) -> HistoryFilter {
    HistoryFilter {
        domain,
        filter_flags: 0,
        start_utc_ms: 0,
        end_utc_ms: 0,
        record_type: 0,
        parent_session: 0,
    }
}
