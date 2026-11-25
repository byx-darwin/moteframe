//! Independent asynchronous NOR fixtures; shared across focused suites.
#![allow(
    dead_code,
    clippy::unwrap_used,
    reason = "Test-only stateful NOR fixture assertions."
)]
use core::{
    cell::{Cell, RefCell},
    task::Poll,
};
use device_history_service::HistoryFailure;
use device_history_store::HistoryClock;
use device_storage::{
    incremental::{AsyncMedium, Completion, MediumOp, StartError},
    persistence::{Error, Geometry},
};
use std::rc::Rc;
use std::{vec, vec::Vec}; // Each suite selects a different subset of the common fixture.
#[derive(Debug, Clone)]
pub struct Control {
    pub bytes: Rc<RefCell<Vec<u8>>>,
    pub durable: Rc<RefCell<Vec<u8>>>,
    pub calls: Rc<Cell<usize>>,
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
