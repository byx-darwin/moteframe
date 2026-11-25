//! Independent asynchronous NOR fixtures; shared across focused suites.
#![allow(dead_code, clippy::unwrap_used)] // Each suite selects a different subset of the common fixture.
use core::{
    cell::{Cell, RefCell},
    task::Poll,
};
use device_history_service::{HistoryAccess, HistoryFailure, HistoryOperation, Preparation};
use device_history_store::{
    AppendRecord, DomainConfig, HistoryClock, HistorySchema,
    async_paged::{AsyncPagedHistoryStore, JobResult, JobToken},
    paged::LogConfig,
};
use device_storage::{
    StoredRecord,
    incremental::{AsyncMedium, Completion, IncrementalSnapshot, MediumOp, StartError},
    persistence::{Error, Geometry, Layout},
};
use std::{rc::Rc, vec, vec::Vec};
#[derive(Debug, Clone)]
pub struct Control {
    pub bytes: Rc<RefCell<Vec<u8>>>,
    pub durable: Rc<RefCell<Vec<u8>>>,
    pub calls: Rc<Cell<usize>>,
    pub reads: Rc<RefCell<Vec<(u32, usize)>>>,
    pub fail: Rc<Cell<Option<usize>>>,
    pub cut: Rc<Cell<Option<usize>>>,
    pub buffered: Rc<Cell<bool>>,
    pub sync_cut: Rc<Cell<Option<usize>>>,
    pub busy: Rc<Cell<usize>>,
    pub wrong: Rc<Cell<bool>>,
    pub slow: usize,
    pub w: u32,
    pub e: u32,
}
impl Control {
    pub fn new(n: usize, w: u32, e: u32) -> Self {
        Self {
            bytes: Rc::new(RefCell::new(vec![255; n])),
            durable: Rc::new(RefCell::new(vec![255; n])),
            calls: Rc::new(Cell::new(0)),
            reads: Rc::new(RefCell::new(Vec::new())),
            fail: Rc::new(Cell::new(None)),
            cut: Rc::new(Cell::new(None)),
            buffered: Rc::new(Cell::new(false)),
            sync_cut: Rc::new(Cell::new(None)),
            busy: Rc::new(Cell::new(0)),
            wrong: Rc::new(Cell::new(false)),
            slow: 1,
            w,
            e,
        }
    }
    pub fn medium(&self) -> Mem {
        Mem {
            control: self.clone(),
            active: None,
            delay: 0,
        }
    }
    pub fn power_loss(&self) {
        self.bytes.borrow_mut().clone_from(&self.durable.borrow());
    }
}
#[derive(Debug)]
enum Op {
    Read(u32, usize),
    Program(u32, Vec<u8>),
    Erase(u32),
    Sync,
}
#[derive(Debug)]
pub struct Mem {
    control: Control,
    active: Option<Op>,
    delay: usize,
}
impl AsyncMedium for Mem {
    fn geometry(&self) -> Geometry {
        Geometry {
            capacity: u32::try_from(self.control.bytes.borrow().len()).unwrap(),
            erase_size: self.control.e,
            write_size: self.control.w,
        }
    }
    fn begin(&mut self, op: MediumOp<'_>) -> Result<(), StartError> {
        let c = &self.control;
        let call = c.calls.get();
        c.calls.set(call + 1);
        if c.busy.get() > 0 {
            c.busy.set(c.busy.get() - 1);
            return Err(StartError::Busy);
        }
        if self.active.is_some() {
            return Err(StartError::Busy);
        }
        if c.fail.get() == Some(call) {
            return Err(StartError::Io);
        }
        self.active = Some(match op {
            MediumOp::Read { offset, length } => {
                c.reads.borrow_mut().push((offset, length));
                assert!((1..=256).contains(&length));
                Op::Read(offset, length)
            }
            MediumOp::Program { offset, bytes } => {
                assert_eq!(bytes.len(), c.w as usize);
                assert!(offset.is_multiple_of(c.w));
                Op::Program(offset, bytes.to_vec())
            }
            MediumOp::Erase { offset } => {
                assert!(offset.is_multiple_of(c.e));
                Op::Erase(offset)
            }
            MediumOp::Sync => Op::Sync,
        });
        self.delay = c.slow;
        Ok(())
    }
    fn poll(&mut self, out: &mut [u8]) -> Poll<Result<Completion, Error>> {
        let c = &self.control;
        let call = c.calls.get();
        c.calls.set(call + 1);
        if self.delay > 0 {
            self.delay -= 1;
            return Poll::Pending;
        }
        let Some(op) = self.active.take() else {
            return Poll::Ready(Err(Error::Io));
        };
        if c.fail.get() == Some(call) {
            return Poll::Ready(Err(Error::Io));
        }
        let mut failure = false;
        let done = match op {
            Op::Read(at, n) => {
                out[..n].copy_from_slice(&c.bytes.borrow()[at as usize..at as usize + n]);
                Completion::Read(n)
            }
            Op::Program(at, bytes) => {
                for (i, b) in bytes.iter().enumerate() {
                    if c.cut.get() == Some(0) {
                        failure = true;
                        break;
                    }
                    if let Some(n) = c.cut.get() {
                        c.cut.set(Some(n - 1));
                    }
                    c.bytes.borrow_mut()[at as usize + i] &= *b;
                }
                if !c.buffered.get() {
                    let n = bytes.len();
                    c.durable.borrow_mut()[at as usize..at as usize + n]
                        .copy_from_slice(&c.bytes.borrow()[at as usize..at as usize + n]);
                }
                Completion::Programmed
            }
            Op::Erase(at) => {
                c.bytes.borrow_mut()[at as usize..(at + c.e) as usize].fill(255);
                if !c.buffered.get() {
                    c.durable.borrow_mut()[at as usize..(at + c.e) as usize].fill(255);
                }
                Completion::Erased
            }
            Op::Sync => {
                if c.buffered.get() {
                    let visible = c.bytes.borrow();
                    let mut durable = c.durable.borrow_mut();
                    for (a, b) in durable.iter_mut().zip(visible.iter()) {
                        if *a != *b {
                            if c.sync_cut.get() == Some(0) {
                                failure = true;
                                break;
                            }
                            if let Some(n) = c.sync_cut.get() {
                                c.sync_cut.set(Some(n - 1));
                            }
                            *a = *b;
                        }
                    }
                }
                Completion::Synced
            }
        };
        if c.wrong.replace(false) {
            return Poll::Ready(Ok(Completion::Erased));
        }
        Poll::Ready(if failure { Err(Error::Io) } else { Ok(done) })
    }
}
#[derive(Debug, Clone)]
pub struct Clock(pub Rc<Cell<Option<u64>>>);
impl Clock {
    pub fn new() -> Self {
        Self(Rc::new(Cell::new(Some(100))))
    }
}
impl HistoryClock for Clock {
    fn now_ms(&self) -> Result<u64, HistoryFailure> {
        self.0.get().ok_or(HistoryFailure::Clock)
    }
}
#[derive(Debug)]
pub struct Schema;
impl HistorySchema for Schema {
    fn validate(&self, r: StoredRecord<'_>, parent: u64) -> Result<(), HistoryFailure> {
        if r.schema == 1 && r.record_type > 0 && !r.payload.is_empty() && parent == 7 {
            Ok(())
        } else {
            Err(HistoryFailure::Invalid)
        }
    }
}
#[derive(Debug)]
pub struct Access(pub Cell<bool>);
impl Access {
    pub fn yes() -> Self {
        Self(Cell::new(true))
    }
}
impl HistoryAccess for Access {
    fn module_allowed(&self) -> bool {
        self.0.get()
    }
    fn allowed(&self, _: [u8; 16], _: Option<u8>, _: HistoryOperation) -> bool {
        self.0.get()
    }
}
pub type Store<
    const D: usize = 1,
    const R: usize = 255,
    const B: usize = 474,
    const N: usize = 464,
> = AsyncPagedHistoryStore<Mem, Mem, Schema, Clock, D, R, B, N>;
pub fn config(slots: u32) -> LogConfig {
    LogConfig {
        namespace: [9; 16],
        log_start: 0x0020_2000,
        slots_per_domain: slots,
    }
}
pub fn owner<const D: usize, const R: usize, const B: usize, const N: usize>(
    meta: &Control,
    log: &Control,
    clock: Clock,
    slots: u32,
) -> Store<D, R, B, N> {
    Store::new_unrecovered(
        IncrementalSnapshot::new_unrecovered(
            meta.medium(),
            Layout {
                start: 0,
                bank_bytes: 4096,
            },
        )
        .unwrap(),
        log.medium(),
        Schema,
        clock,
        config(slots),
        60,
    )
    .unwrap()
}
pub fn run<const D: usize, const R: usize, const B: usize, const N: usize>(
    s: &Store<D, R, B, N>,
    t: JobToken<'_>,
    controls: &[&Control],
) -> Result<JobResult, HistoryFailure> {
    for _ in 0..2_000_000 {
        if let Poll::Ready(result) = s.result(t).unwrap() {
            return result;
        }
        let before: usize = controls.iter().map(|c| c.calls.get()).sum();
        s.step().unwrap();
        let after: usize = controls.iter().map(|c| c.calls.get()).sum();
        assert!(after - before <= 1, "more than one physical call per slice");
    }
    panic!("job did not finish")
}
pub fn initialize<const D: usize, const R: usize, const B: usize, const N: usize>(
    s: &Store<D, R, B, N>,
    meta: &Control,
    log: &Control,
    domains: &[DomainConfig],
) {
    assert_eq!(
        run(s, s.begin_recover().unwrap(), &[meta, log]),
        Ok(JobResult::Recovered { initialized: false })
    );
    assert_eq!(
        run(
            s,
            s.begin_initialize([3; 16], domains).unwrap(),
            &[meta, log]
        ),
        Ok(JobResult::Initialized)
    );
}
pub fn input(body: &[u8], domain: u8) -> AppendRecord<'_> {
    AppendRecord {
        domain,
        record_type: 1,
        schema: 1,
        utc_ms: 0,
        flags: 0,
        parent_session: 7,
        payload: body,
    }
}
pub fn filter(domain: u8) -> device_protocol::HistoryFilter {
    device_protocol::HistoryFilter {
        domain,
        filter_flags: 0,
        start_utc_ms: 0,
        end_utc_ms: 0,
        record_type: 0,
        parent_session: 0,
    }
}
pub fn prepare<const D: usize, const R: usize, const B: usize, const N: usize>(
    s: &Store<D, R, B, N>,
    adapter: &mut impl device_history_service::RecordStore,
    request: Preparation,
    access: &Access,
    controls: &[&Control],
) {
    assert!(!adapter.prepare(request, 100, access).unwrap());
    for _ in 0..100_000 {
        s.step().unwrap();
        if adapter.prepare(request, 100, access).unwrap() {
            return;
        }
        let _: usize = controls.iter().map(|c| c.calls.get()).sum();
    }
    panic!("preparation stalled")
}
/// Independent synchronous access is used only for exact legacy format comparison.
#[derive(Debug)]
pub struct SyncMem(pub Control);
impl device_storage::persistence::Medium for SyncMem {
    fn geometry(&self) -> Geometry {
        Geometry {
            capacity: u32::try_from(self.0.bytes.borrow().len()).unwrap(),
            write_size: self.0.w,
            erase_size: self.0.e,
        }
    }
    fn read(&mut self, at: u32, out: &mut [u8]) -> Result<(), Error> {
        out.copy_from_slice(&self.0.bytes.borrow()[at as usize..at as usize + out.len()]);
        Ok(())
    }
    fn program(&mut self, at: u32, bytes: &[u8]) -> Result<(), Error> {
        for (i, b) in bytes.iter().enumerate() {
            self.0.bytes.borrow_mut()[at as usize + i] &= *b;
        }
        Ok(())
    }
    fn erase(&mut self, at: u32, n: u32) -> Result<(), Error> {
        self.0.bytes.borrow_mut()[at as usize..(at + n) as usize].fill(255);
        Ok(())
    }
    fn sync(&mut self) -> Result<(), Error> {
        Ok(())
    }
}
