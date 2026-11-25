//! Real byte-array NOR semantics at the `AsyncMedium` boundary; no sync adapter.
use core::task::Poll;
use device_storage::incremental::{AsyncMedium, Completion, MediumOp, StartError};
use device_storage::partitions::{AsyncPartitions, PartitionError, Range};
use device_storage::persistence::{Error, Geometry};
use std::{cell::RefCell, rc::Rc};

const G: Geometry = Geometry {
    capacity: 1024,
    erase_size: 64,
    write_size: 4,
};
const R: [Range; 2] = [
    Range {
        start: 128,
        length: 256,
        erase: true,
    },
    Range {
        start: 512,
        length: 256,
        erase: false,
    },
];
#[derive(Debug)]
enum Op {
    Read(usize, usize),
    Program(usize, Vec<u8>),
    Erase(usize),
    Sync,
}
#[derive(Debug)]
struct Trace {
    begins: usize,
    polls: usize,
    bytes: [u8; 1024],
    durable: [u8; 1024],
    malformed: Option<Completion>,
    error: bool,
    begin_error: bool,
    busy: bool,
    pending: usize,
}
impl Default for Trace {
    fn default() -> Self {
        Self {
            begins: 0,
            polls: 0,
            bytes: [255; 1024],
            durable: [255; 1024],
            malformed: None,
            error: false,
            begin_error: false,
            busy: false,
            pending: 1,
        }
    }
}
#[derive(Debug)]
struct Nor {
    trace: Rc<RefCell<Trace>>,
    op: Option<Op>,
    wait: usize,
    geometry: Geometry,
}
impl AsyncMedium for Nor {
    fn geometry(&self) -> Geometry {
        self.geometry
    }
    fn begin(&mut self, op: MediumOp<'_>) -> Result<(), StartError> {
        let mut t = self.trace.borrow_mut();
        t.begins += 1;
        if self.op.is_some() || t.busy {
            t.busy = false;
            return Err(StartError::Busy);
        }
        self.op = Some(match op {
            MediumOp::Read { offset, length } => Op::Read(offset as usize, length),
            MediumOp::Program { offset, bytes } => Op::Program(offset as usize, bytes.to_vec()),
            MediumOp::Erase { offset } => Op::Erase(offset as usize),
            MediumOp::Sync => Op::Sync,
        });
        self.wait = t.pending;
        if t.begin_error {
            t.begin_error = false;
            return Err(StartError::Io);
        }
        Ok(())
    }
    fn poll(&mut self, out: &mut [u8]) -> Poll<Result<Completion, Error>> {
        let mut t = self.trace.borrow_mut();
        t.polls += 1;
        if self.wait > 0 {
            self.wait -= 1;
            out[0] = 0x99;
            return Poll::Pending;
        }
        let Some(op) = self.op.take() else {
            // Terminal results are consumed once; idle polling need not replay.
            return Poll::Pending;
        };
        if t.error {
            t.error = false;
            return Poll::Ready(Err(Error::Io));
        }
        let c = match op {
            Op::Read(o, n) => {
                out[..n].copy_from_slice(&t.bytes[o..o + n]);
                Completion::Read(n)
            }
            Op::Program(o, b) => {
                for (i, v) in b.into_iter().enumerate() {
                    if t.bytes[o + i] & v != v {
                        return Poll::Ready(Err(Error::Io));
                    }
                    t.bytes[o + i] = v;
                }
                Completion::Programmed
            }
            Op::Erase(o) => {
                t.bytes[o..o + 64].fill(255);
                Completion::Erased
            }
            Op::Sync => {
                t.durable = t.bytes;
                Completion::Synced
            }
        };
        Poll::Ready(Ok(t.malformed.take().unwrap_or(c)))
    }
}
fn broker() -> (AsyncPartitions<Nor, 2>, Rc<RefCell<Trace>>) {
    let t = Rc::new(RefCell::new(Trace::default()));
    (
        AsyncPartitions::new_unrecovered(
            Nor {
                trace: t.clone(),
                op: None,
                wait: 0,
                geometry: G,
            },
            R,
        )
        .unwrap_or_else(|e| panic!("{e:?}")),
        t,
    )
}
fn calls(t: &Rc<RefCell<Trace>>) -> usize {
    let t = t.borrow();
    t.begins + t.polls
}
fn recover(c: &mut device_storage::partitions::Coordinator<'_, Nor>, t: &Rc<RefCell<Trace>>) {
    for _ in 0..20 {
        let old = calls(t);
        let p = c.recover_step();
        assert!(calls(t) - old <= 1);
        if let Poll::Ready(r) = p {
            r.unwrap_or_else(|e| panic!("{e:?}"));
            return;
        }
    }
    panic!("recovery did not terminate");
}
fn finish(p: &mut impl AsyncMedium) -> Completion {
    let mut out = [0xa5; 256];
    for _ in 0..20 {
        if let Poll::Ready(r) = p.poll(&mut out) {
            return r.unwrap_or_else(|e| panic!("{e:?}"));
        }
    }
    panic!("operation did not terminate");
}

#[test]
fn test_should_validate_geometry_disjoint_and_checked_end_without_io() {
    for g in [
        Geometry { capacity: 0, ..G },
        Geometry { write_size: 0, ..G },
        Geometry {
            write_size: 257,
            ..G
        },
        Geometry { erase_size: 0, ..G },
        Geometry {
            erase_size: 62,
            ..G
        },
        Geometry {
            capacity: 1023,
            ..G
        },
    ] {
        let trace = Rc::new(RefCell::new(Trace::default()));
        assert!(
            AsyncPartitions::new_unrecovered(
                Nor {
                    trace: trace.clone(),
                    op: None,
                    wait: 0,
                    geometry: g
                },
                R
            )
            .is_err()
        );
        assert_eq!(calls(&trace), 0);
    }
    for ranges in [
        [
            Range {
                start: 128,
                length: 0,
                erase: true,
            },
            R[1],
        ],
        [Range { start: 129, ..R[0] }, R[1]],
        [R[0], Range { start: 320, ..R[1] }],
        [
            Range {
                start: u32::MAX - 63,
                length: 128,
                erase: true,
            },
            R[1],
        ],
        [
            R[0],
            Range {
                start: 896,
                length: 256,
                erase: true,
            },
        ],
    ] {
        let trace = Rc::new(RefCell::new(Trace::default()));
        assert!(
            AsyncPartitions::new_unrecovered(
                Nor {
                    trace: trace.clone(),
                    op: None,
                    wait: 0,
                    geometry: G
                },
                ranges
            )
            .is_err()
        );
        assert_eq!(calls(&trace), 0);
    }
}
#[test]
fn test_should_require_sync_then_owner_attestation_before_writes() {
    let (mut broker_state, trace) = broker();
    let (mut coordinator, [mut first, mut second]) =
        broker_state.split().unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(
        first.begin(MediumOp::Read {
            offset: 0,
            length: 1
        }),
        Err(StartError::Io)
    );
    assert_eq!(calls(&trace), 0);
    assert_eq!(
        coordinator.attest_normal(),
        Err(PartitionError::NeedsRecovery)
    );
    recover(&mut coordinator, &trace);
    assert_eq!(
        first.begin(MediumOp::Program {
            offset: 0,
            bytes: &[0; 4]
        }),
        Err(StartError::Io)
    );
    first
        .begin(MediumOp::Read {
            offset: 0,
            length: 1,
        })
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(coordinator.attest_normal(), Err(PartitionError::Busy));
    assert_eq!(finish(&mut first), Completion::Read(1));
    second
        .begin(MediumOp::Sync)
        .unwrap_or_else(|e| panic!("{e:?}"));
    finish(&mut second);
    coordinator
        .attest_normal()
        .unwrap_or_else(|e| panic!("{e:?}"));
    first
        .begin(MediumOp::Program {
            offset: 0,
            bytes: &[1, 2, 3, 4],
        })
        .unwrap_or_else(|e| panic!("{e:?}"));
    finish(&mut first);
    assert_eq!(&trace.borrow().bytes[128..132], &[1, 2, 3, 4]);
    assert_eq!(&trace.borrow().bytes[0..4], &[255; 4]);
}
#[test]
fn test_should_keep_owner_across_pending_wrong_polls_and_drop() {
    let (mut broker_state, trace) = broker();
    let (mut coordinator, [mut first, mut second]) =
        broker_state.split().unwrap_or_else(|e| panic!("{e:?}"));
    recover(&mut coordinator, &trace);
    coordinator
        .attest_normal()
        .unwrap_or_else(|e| panic!("{e:?}"));
    first
        .begin(MediumOp::Program {
            offset: 4,
            bytes: &[1, 2, 3, 4],
        })
        .unwrap_or_else(|e| panic!("{e:?}"));
    let old = calls(&trace);
    assert_eq!(
        second.begin(MediumOp::Read {
            offset: 0,
            length: 1
        }),
        Err(StartError::Busy)
    );
    assert_eq!(
        second.poll(&mut [0; 256]),
        Poll::Ready(Err(Error::Conflict))
    );
    assert_eq!(calls(&trace), old);
    assert_eq!(first.poll(&mut [0; 256]), Poll::Pending);
    // This explicitly exercises dropping an accepted owner capability.
    #[allow(clippy::drop_non_drop)]
    drop(first);
    assert_eq!(
        second.begin(MediumOp::Read {
            offset: 0,
            length: 1
        }),
        Err(StartError::Busy)
    );
    recover(&mut coordinator, &trace);
    assert_eq!(&trace.borrow().durable[132..136], &[1, 2, 3, 4]);
    assert_eq!(
        second.begin(MediumOp::Program {
            offset: 0,
            bytes: &[0; 4]
        }),
        Err(StartError::Io)
    );
    coordinator
        .attest_normal()
        .unwrap_or_else(|e| panic!("{e:?}"));
    second
        .begin(MediumOp::Read {
            offset: 0,
            length: 1,
        })
        .unwrap_or_else(|e| panic!("{e:?}"));
    finish(&mut second);
}
#[test]
fn test_should_translate_check_bounds_alignment_erase_permission_and_copy_program_input() {
    let (mut broker_state, trace) = broker();
    let (mut coordinator, [mut first, mut second]) =
        broker_state.split().unwrap_or_else(|e| panic!("{e:?}"));
    recover(&mut coordinator, &trace);
    coordinator
        .attest_normal()
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert_eq!(first.geometry(), Geometry { capacity: 256, ..G });
    for op in [
        MediumOp::Read {
            offset: 256,
            length: 1,
        },
        MediumOp::Read {
            offset: u32::MAX,
            length: 2,
        },
        MediumOp::Read {
            offset: 0,
            length: 0,
        },
        MediumOp::Read {
            offset: 0,
            length: 257,
        },
        MediumOp::Program {
            offset: 1,
            bytes: &[0; 4],
        },
        MediumOp::Program {
            offset: 0,
            bytes: &[0; 8],
        },
        MediumOp::Erase { offset: 1 },
        MediumOp::Erase { offset: 256 },
    ] {
        let old = calls(&trace);
        assert_eq!(first.begin(op), Err(StartError::Io));
        assert_eq!(calls(&trace), old);
    }
    let old = calls(&trace);
    assert_eq!(
        second.begin(MediumOp::Erase { offset: 0 }),
        Err(StartError::Io)
    );
    assert_eq!(calls(&trace), old);
    let mut data = [4, 3, 2, 1];
    first
        .begin(MediumOp::Program {
            offset: 252,
            bytes: &data,
        })
        .unwrap_or_else(|e| panic!("{e:?}"));
    data.fill(0);
    finish(&mut first);
    assert_eq!(&trace.borrow().bytes[380..384], &[4, 3, 2, 1]);
    first
        .begin(MediumOp::Erase { offset: 192 })
        .unwrap_or_else(|e| panic!("{e:?}"));
    finish(&mut first);
    assert_eq!(&trace.borrow().bytes[320..384], &[255; 64]);
}
#[test]
fn test_should_never_mutate_caller_on_pending_error_short_or_malformed_read() {
    let (mut broker_state, trace) = broker();
    let (mut coordinator, [mut first, mut second]) =
        broker_state.split().unwrap_or_else(|e| panic!("{e:?}"));
    recover(&mut coordinator, &trace);
    coordinator
        .attest_normal()
        .unwrap_or_else(|e| panic!("{e:?}"));
    first
        .begin(MediumOp::Read {
            offset: 0,
            length: 4,
        })
        .unwrap_or_else(|e| panic!("{e:?}"));
    let mut out = [0xa5; 256];
    assert_eq!(first.poll(&mut out), Poll::Pending);
    assert_eq!(out, [0xa5; 256]);
    let old = calls(&trace);
    assert_eq!(first.poll(&mut [7; 3]), Poll::Ready(Err(Error::Capacity)));
    assert_eq!(calls(&trace), old);
    trace.borrow_mut().malformed = Some(Completion::Read(3));
    assert_eq!(first.poll(&mut out), Poll::Ready(Err(Error::Corrupt)));
    assert_eq!(out, [0xa5; 256]);
    assert_eq!(
        second.begin(MediumOp::Read {
            offset: 0,
            length: 1
        }),
        Err(StartError::Io)
    );
    assert_eq!(first.poll(&mut out), Poll::Ready(Err(Error::NeedsRecovery)));
    recover(&mut coordinator, &trace);
    coordinator
        .attest_normal()
        .unwrap_or_else(|e| panic!("{e:?}"));
    first
        .begin(MediumOp::Program {
            offset: 0,
            bytes: &[0; 4],
        })
        .unwrap_or_else(|e| panic!("{e:?}"));
    trace.borrow_mut().malformed = Some(Completion::Erased);
    assert_eq!(first.poll(&mut out), Poll::Pending);
    assert_eq!(first.poll(&mut out), Poll::Ready(Err(Error::Corrupt)));
    assert_eq!(out, [0xa5; 256]);
}
#[test]
fn test_should_recover_uncertain_admission_and_terminal_io_globally() {
    let (mut broker_state, trace) = broker();
    let (mut coordinator, [mut first, mut second]) =
        broker_state.split().unwrap_or_else(|e| panic!("{e:?}"));
    recover(&mut coordinator, &trace);
    coordinator
        .attest_normal()
        .unwrap_or_else(|e| panic!("{e:?}"));
    trace.borrow_mut().begin_error = true;
    assert_eq!(
        first.begin(MediumOp::Program {
            offset: 0,
            bytes: &[0; 4]
        }),
        Err(StartError::Io)
    );
    assert_eq!(
        second.begin(MediumOp::Read {
            offset: 0,
            length: 1
        }),
        Err(StartError::Busy)
    );
    recover(&mut coordinator, &trace);
    assert_eq!(&trace.borrow().durable[128..132], &[0; 4]);
    coordinator
        .attest_normal()
        .unwrap_or_else(|e| panic!("{e:?}"));
    first
        .begin(MediumOp::Read {
            offset: 0,
            length: 4,
        })
        .unwrap_or_else(|e| panic!("{e:?}"));
    trace.borrow_mut().error = true;
    let mut out = [0xa5; 256];
    assert_eq!(first.poll(&mut out), Poll::Pending);
    assert_eq!(first.poll(&mut out), Poll::Ready(Err(Error::Io)));
    assert_eq!(out, [0xa5; 256]);
    assert_eq!(second.begin(MediumOp::Sync), Err(StartError::Io));
    trace.borrow_mut().error = true;
    assert!(matches!(coordinator.recover_step(), Poll::Pending));
    assert!(matches!(coordinator.recover_step(), Poll::Pending));
    assert_eq!(
        coordinator.recover_step(),
        Poll::Ready(Err(PartitionError::Storage(Error::Io)))
    );
    assert_eq!(
        coordinator.attest_normal(),
        Err(PartitionError::NeedsRecovery)
    );
    recover(&mut coordinator, &trace);
}
#[test]
fn test_should_split_only_once_even_after_all_handles_drop() {
    let (mut broker_state, _) = broker();
    {
        let (_coordinator, _partitions) = broker_state.split().unwrap_or_else(|e| panic!("{e:?}"));
    }
    assert!(matches!(
        broker_state.split(),
        Err(PartitionError::AlreadySplit)
    ));
}

#[test]
fn test_should_preserve_no_admission_busy_and_validate_recovery_sync_completion() {
    let (mut broker_state, trace) = broker();
    let (mut coordinator, [mut first, mut second]) =
        broker_state.split().unwrap_or_else(|e| panic!("{e:?}"));
    trace.borrow_mut().busy = true;
    let old = calls(&trace);
    assert_eq!(coordinator.recover_step(), Poll::Pending);
    assert_eq!(calls(&trace), old + 1);
    trace.borrow_mut().malformed = Some(Completion::Programmed);
    assert_eq!(coordinator.recover_step(), Poll::Pending);
    assert_eq!(coordinator.recover_step(), Poll::Pending);
    assert_eq!(
        coordinator.recover_step(),
        Poll::Ready(Err(PartitionError::Storage(Error::Corrupt)))
    );
    assert_eq!(first.begin(MediumOp::Sync), Err(StartError::Io));
    recover(&mut coordinator, &trace);
    coordinator
        .attest_normal()
        .unwrap_or_else(|e| panic!("{e:?}"));
    trace.borrow_mut().busy = true;
    assert_eq!(
        first.begin(MediumOp::Read {
            offset: 0,
            length: 4
        }),
        Err(StartError::Busy)
    );
    second
        .begin(MediumOp::Read {
            offset: 252,
            length: 4,
        })
        .unwrap_or_else(|e| panic!("{e:?}"));
    let mut out = [0xa5; 256];
    assert_eq!(second.poll(&mut out), Poll::Pending);
    assert_eq!(second.poll(&mut out), Poll::Ready(Ok(Completion::Read(4))));
    assert_eq!(&out[..4], &[255; 4]);
    assert_eq!(&out[4..], &[0xa5; 252]);
    assert_eq!(first.poll(&mut out), Poll::Ready(Err(Error::Conflict)));
}
#[test]
fn test_should_close_normal_gate_before_drain_and_never_forward_owner_poll_during_recovery() {
    let (mut broker_state, trace) = broker();
    let (mut coordinator, [mut first, mut second]) =
        broker_state.split().unwrap_or_else(|e| panic!("{e:?}"));
    recover(&mut coordinator, &trace);
    coordinator
        .attest_normal()
        .unwrap_or_else(|e| panic!("{e:?}"));
    first
        .begin(MediumOp::Erase { offset: 0 })
        .unwrap_or_else(|e| panic!("{e:?}"));
    let old = calls(&trace);
    assert_eq!(coordinator.recover_step(), Poll::Pending);
    assert_eq!(calls(&trace), old + 1);
    let old = calls(&trace);
    assert_eq!(
        first.poll(&mut [0; 256]),
        Poll::Ready(Err(Error::NeedsRecovery))
    );
    assert_eq!(
        second.begin(MediumOp::Read {
            offset: 0,
            length: 1
        }),
        Err(StartError::Busy)
    );
    assert_eq!(calls(&trace), old);
    recover(&mut coordinator, &trace);
    let old = calls(&trace);
    assert_eq!(coordinator.recover_step(), Poll::Ready(Ok(())));
    assert_eq!(calls(&trace), old);
    assert_eq!(
        first.begin(MediumOp::Erase { offset: 0 }),
        Err(StartError::Io)
    );
    coordinator
        .attest_normal()
        .unwrap_or_else(|e| panic!("{e:?}"));
}

fn terminal_once_partition(malformed: bool) {
    let (mut broker_state, trace) = broker();
    trace.borrow_mut().pending = 0;
    let (mut coordinator, [mut first, mut second]) =
        broker_state.split().unwrap_or_else(|e| panic!("{e:?}"));
    recover(&mut coordinator, &trace);
    coordinator
        .attest_normal()
        .unwrap_or_else(|e| panic!("{e:?}"));
    first
        .begin(MediumOp::Read {
            offset: 0,
            length: 4,
        })
        .unwrap_or_else(|e| panic!("{e:?}"));
    if malformed {
        trace.borrow_mut().malformed = Some(Completion::Read(3));
    } else {
        trace.borrow_mut().error = true;
    }
    let mut out = [0xa5; 256];
    assert_eq!(
        first.poll(&mut out),
        Poll::Ready(Err(if malformed { Error::Corrupt } else { Error::Io }))
    );
    assert_eq!(out, [0xa5; 256]);
    let polls = trace.borrow().polls;
    let begins = trace.borrow().begins;
    // Closed global access survives terminal release, without issuing new I/O.
    assert!(second.begin(MediumOp::Sync).is_err());
    assert_eq!(calls(&trace), polls + begins);
    recover(&mut coordinator, &trace);
    assert_eq!(trace.borrow().begins, begins + 1);
    assert_eq!(
        trace.borrow().polls,
        polls + 1,
        "only poll the newly accepted Sync, never idle"
    );
    assert_eq!(
        first.begin(MediumOp::Program {
            offset: 0,
            bytes: &[0; 4]
        }),
        Err(StartError::Io)
    );
    coordinator
        .attest_normal()
        .unwrap_or_else(|e| panic!("{e:?}"));
}
fn terminal_once_sync(malformed: bool) {
    let (mut broker_state, trace) = broker();
    trace.borrow_mut().pending = 0;
    let (mut coordinator, [mut first, _second]) =
        broker_state.split().unwrap_or_else(|e| panic!("{e:?}"));
    if malformed {
        trace.borrow_mut().malformed = Some(Completion::Programmed);
    } else {
        trace.borrow_mut().error = true;
    }
    assert_eq!(coordinator.recover_step(), Poll::Pending);
    assert_eq!(
        coordinator.recover_step(),
        Poll::Ready(Err(PartitionError::Storage(if malformed {
            Error::Corrupt
        } else {
            Error::Io
        })))
    );
    let polls = trace.borrow().polls;
    let begins = trace.borrow().begins;
    assert!(first.begin(MediumOp::Sync).is_err());
    assert_eq!(calls(&trace), polls + begins);
    recover(&mut coordinator, &trace);
    assert_eq!(trace.borrow().begins, begins + 1);
    assert_eq!(
        trace.borrow().polls,
        polls + 1,
        "only poll the newly accepted Sync, never idle"
    );
    assert_eq!(
        first.begin(MediumOp::Program {
            offset: 0,
            bytes: &[0; 4]
        }),
        Err(StartError::Io)
    );
    coordinator
        .attest_normal()
        .unwrap_or_else(|e| panic!("{e:?}"));
}
#[test]
fn test_should_recover_terminal_once_partition_error_without_repoll() {
    terminal_once_partition(false);
}
#[test]
fn test_should_recover_terminal_once_partition_malformed_without_repoll() {
    terminal_once_partition(true);
}
#[test]
fn test_should_recover_terminal_once_sync_error_without_repoll() {
    terminal_once_sync(false);
}
#[test]
fn test_should_recover_terminal_once_sync_malformed_without_repoll() {
    terminal_once_sync(true);
}
