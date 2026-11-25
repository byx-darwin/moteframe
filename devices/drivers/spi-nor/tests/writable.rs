//! Stateful command model: real worker and snapshot engine, no hardware.
use core::task::Poll;
use device_spi_nor::writable::{
    Attestation, Fault, Monotonic, NorChip, Partition, Profile, ProfileConfig, RecoveryToken,
    WritableNor,
};
use device_storage::{
    incremental::{AsyncMedium, Completion, IncrementalSnapshot, MediumOp, StartError},
    persistence::{Error, Layout},
};
use embedded_hal::spi::{ErrorType, Operation, SpiDevice};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

trait Require<T> {
    fn require(self) -> T;
}
impl<T, E: core::fmt::Debug> Require<T> for Result<T, E> {
    fn require(self) -> T {
        match self {
            Ok(value) => value,
            Err(error) => panic!("{error:?}"),
        }
    }
}
impl<T> Require<T> for Option<T> {
    fn require(self) -> T {
        match self {
            Some(value) => value,
            None => panic!("missing test value"),
        }
    }
}
fn abandon<T>(_: T) {}

#[derive(Debug, Default)]
struct ModelFaults {
    no_wel: bool,
    stuck: bool,
    corrupt: bool,
}

#[derive(Debug)]
struct Model {
    bytes: Vec<u8>,
    log: Vec<Vec<u8>>,
    wel: bool,
    faults: ModelFaults,
    protection: u8,
    busy: u8,
    fail: Option<usize>,
}
impl Model {
    fn new() -> Self {
        Self {
            bytes: vec![255; 65536],
            log: Vec::new(),
            wel: false,
            faults: ModelFaults::default(),
            protection: 0,
            busy: 0,
            fail: None,
        }
    }
    fn command(&mut self, command: &[u8], out: &mut [u8]) {
        let offset = if command.len() >= 4 {
            usize::from(command[1]) * 65536
                + usize::from(command[2]) * 256
                + usize::from(command[3])
        } else {
            0
        };
        match command[0] {
            0x05 => {
                out.fill(
                    self.protection
                        | (u8::from(self.wel) * 2)
                        | u8::from(self.faults.stuck || self.busy > 0),
                );
                self.busy = self.busy.saturating_sub(1);
            }
            0x9f => out.copy_from_slice(&[0xef, 0x40, 0x10]),
            0x06 => self.wel = !self.faults.no_wel,
            0x02 => {
                assert!(self.wel && self.busy == 0 && self.protection == 0);
                for (old, new) in self.bytes[offset..offset + command.len() - 4]
                    .iter_mut()
                    .zip(&command[4..])
                {
                    *old &= new;
                }
                self.wel = false;
                self.busy = 2;
            }
            0x20 => {
                assert!(self.wel && self.busy == 0 && self.protection == 0);
                self.bytes[offset..offset + 4096].fill(255);
                self.wel = false;
                self.busy = 2;
            }
            0x03 => {
                assert!(!self.faults.stuck && self.busy == 0, "array read while WIP");
                out.copy_from_slice(&self.bytes[offset..offset + out.len()]);
                if self.faults.corrupt && !out.is_empty() {
                    out[0] ^= 1;
                }
            }
            other => panic!("unqualified opcode {other:02x}"),
        }
    }
}
#[derive(Debug)]
struct Device(Rc<RefCell<Model>>);
impl ErrorType for Device {
    type Error = embedded_hal::spi::ErrorKind;
}
impl SpiDevice for Device {
    fn transaction(&mut self, ops: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
        let mut command = Vec::new();
        for op in ops.iter() {
            if let Operation::Write(bytes) = op {
                command.extend_from_slice(bytes);
            }
        }
        let mut m = self.0.borrow_mut();
        let index = m.log.len();
        m.log.push(command.clone());
        let mut read = false;
        for op in ops {
            if let Operation::Read(out) = op {
                m.command(&command, out);
                read = true;
            }
        }
        if !read {
            m.command(&command, &mut []);
        }
        if m.fail == Some(index) {
            return Err(Self::Error::Other);
        }
        Ok(())
    }
}
#[derive(Debug)]
struct Clock(Rc<Cell<u64>>);
impl Monotonic for Clock {
    fn now_ms(&mut self) -> u64 {
        self.0.get()
    }
}
type Chip = NorChip<Device, Clock>;
type Writer<'a> = WritableNor<'a, Device, Clock>;
fn config(unit: u32) -> ProfileConfig {
    ProfileConfig {
        id: [0xef, 0x40, 0x10],
        capacity: 65536,
        write_size: unit,
        protection_mask: 0x3c,
        protection_value: 0,
        poll_interval_ms: 1,
        program_max_ms: 100,
        erase_max_ms: 100,
        sync_max_ms: 100,
        operation_max_ms: 1000,
    }
}
fn fixture(unit: u32) -> (Chip, Rc<RefCell<Model>>, Rc<Cell<u64>>) {
    let model = Rc::new(RefCell::new(Model::new()));
    let clock = Rc::new(Cell::new(0));
    let profile = Profile::validate(config(unit)).unwrap_or_else(|e| panic!("{e:?}"));
    let evidence = Attestation {
        part: "model only",
        protection: "model only",
        timing: "model only",
        ownership: "model only",
        durability: "model only",
    };
    let chip = NorChip::new(
        Device(model.clone()),
        Clock(clock.clone()),
        profile,
        Partition {
            start: 0x1000,
            length: 0x4000,
        },
        evidence,
    )
    .unwrap_or_else(|e| panic!("{e:?}"));
    (chip, model, clock)
}
fn tick(clock: &Cell<u64>) {
    clock.set(clock.get() + 1);
}
fn finish(writer: &mut Writer<'_>, clock: &Cell<u64>, expected: Completion) {
    for _ in 0..1000 {
        tick(clock);
        match writer.poll(&mut [0; 256]) {
            Poll::Pending => {}
            Poll::Ready(result) => {
                assert_eq!(result, Ok(expected));
                return;
            }
        }
    }
    panic!("bounded model operation never completed");
}
fn settle(chip: &Chip, writer: &mut Writer<'_>, clock: &Cell<u64>) -> RecoveryToken {
    let token = chip.begin_recovery().unwrap_or_else(|e| panic!("{e:?}"));
    writer
        .begin(MediumOp::Sync)
        .unwrap_or_else(|e| panic!("{e:?}"));
    finish(writer, clock, Completion::Synced);
    token
}
fn open(chip: &Chip, writer: &mut Writer<'_>, clock: &Cell<u64>) {
    let token = settle(chip, writer, clock);
    chip.attest_recovery_complete(token)
        .unwrap_or_else(|e| panic!("{e:?}"));
}

// Missing command admission/copy/step ordering would leave erased bytes or expose output early.
#[test]
fn test_should_copy_one_unit_and_program_in_bounded_exact_command_order() {
    let (chip, model, clock) = fixture(256);
    let mut writer = chip.claim_writer().unwrap_or_else(|e| panic!("{e:?}"));
    assert!(chip.claim_writer().is_err());
    open(&chip, &mut writer, &clock);
    model.borrow_mut().log.clear();
    let mut candidate = [0x5a; 256];
    writer
        .begin(MediumOp::Program {
            offset: 0,
            bytes: &candidate,
        })
        .require();
    candidate.fill(0);
    assert!(model.borrow().log.is_empty());
    assert_eq!(writer.begin(MediumOp::Sync), Err(StartError::Busy));
    let mut out = [0x91; 256];
    for _ in 0..100 {
        tick(&clock);
        let before = model.borrow().log.len();
        let result = writer.poll(&mut out);
        assert!(model.borrow().log.len() <= before + 1);
        assert_eq!(out, [0x91; 256]);
        if let Poll::Ready(result) = result {
            assert_eq!(result, Ok(Completion::Programmed));
            break;
        }
    }
    assert_eq!(&model.borrow().bytes[0x1000..0x1100], &[0x5a; 256]);
    assert_eq!(
        model.borrow().log.iter().map(|c| c[0]).collect::<Vec<_>>(),
        [0x05, 0x03, 0x06, 0x05, 0x02, 0x05, 0x05, 0x05, 0x03]
    );
    assert_eq!(model.borrow().log[4][..4], [0x02, 0, 0x10, 0]);
    writer.begin(MediumOp::Sync).require();
    finish(&mut writer, &clock, Completion::Synced);
}

// Skipped FF verification or wrong erase address/count would leave bytes or under-verify.
#[test]
fn test_should_erase_exactly_one_sector_and_verify_all_chunks() {
    let (chip, model, clock) = fixture(256);
    let mut writer = chip.claim_writer().require();
    open(&chip, &mut writer, &clock);
    model.borrow_mut().bytes[0x1000..0x3000].fill(0);
    model.borrow_mut().log.clear();
    writer.begin(MediumOp::Erase { offset: 0 }).require();
    finish(&mut writer, &clock, Completion::Erased);
    assert!(
        model.borrow().bytes[0x1000..0x2000]
            .iter()
            .all(|&v| v == 255)
    );
    assert!(model.borrow().bytes[0x2000..0x3000].iter().all(|&v| v == 0));
    let commands = model.borrow().log.iter().map(|c| c[0]).collect::<Vec<_>>();
    assert_eq!(&commands[..7], [0x05, 0x06, 0x05, 0x20, 0x05, 0x05, 0x05]);
    assert_eq!(&commands[7..], &[0x03; 16]);
}

// Relaxed admission/protection/WEL/bit-transition checks would issue WREN/array writes.
#[test]
fn test_should_reject_bad_ranges_alignment_and_units_without_spi() {
    let (chip, model, clock) = fixture(256);
    let mut writer = chip.claim_writer().require();
    open(&chip, &mut writer, &clock);
    for operation in [
        MediumOp::Read {
            offset: u32::MAX,
            length: 1,
        },
        MediumOp::Read {
            offset: 0,
            length: 257,
        },
        MediumOp::Read {
            offset: 0,
            length: 0,
        },
        MediumOp::Program {
            offset: 1,
            bytes: &[0; 256],
        },
        MediumOp::Program {
            offset: 0,
            bytes: &[0; 1],
        },
        MediumOp::Erase { offset: 1 },
        MediumOp::Erase { offset: 0x4000 },
    ] {
        let before = model.borrow().log.len();
        assert_eq!(writer.begin(operation), Err(StartError::Io));
        assert_eq!(model.borrow().log.len(), before);
        open(&chip, &mut writer, &clock);
    }
    for unit in [0, 2, 255, 257] {
        assert!(Profile::validate(config(unit)).is_err());
    }
}

#[test]
fn test_should_stop_before_array_command_on_protection_wel_or_zero_to_one() {
    for cause in 0..3 {
        let (chip, model, clock) = fixture(1);
        let mut writer = chip.claim_writer().require();
        open(&chip, &mut writer, &clock);
        model.borrow_mut().log.clear();
        match cause {
            0 => model.borrow_mut().protection = 4,
            1 => model.borrow_mut().faults.no_wel = true,
            _ => model.borrow_mut().bytes[0x1000] = 0,
        }
        writer
            .begin(MediumOp::Program {
                offset: 0,
                bytes: &[0x5a],
            })
            .require();
        let mut result = Poll::Pending;
        for _ in 0..20 {
            tick(&clock);
            result = writer.poll(&mut [0; 256]);
            if result.is_ready() {
                break;
            }
        }
        assert_eq!(result, Poll::Ready(Err(Error::Io)));
        let log = &model.borrow().log;
        assert!(!log.iter().any(|v| matches!(v[0], 0x02 | 0x20)));
        if cause != 1 {
            assert!(!log.iter().any(|v| v[0] == 0x06));
        }
        assert_eq!(chip.read_asset(0, &mut [0; 1]), Err(Fault::NeedsRecovery));
    }
}

// Forgetting staging can leak partial transport bytes through Pending/error.
#[test]
fn test_should_stage_reads_and_leave_output_unchanged_on_transport_failure() {
    let (chip, model, clock) = fixture(256);
    let mut writer = chip.claim_writer().require();
    open(&chip, &mut writer, &clock);
    writer
        .begin(MediumOp::Read {
            offset: 0,
            length: 7,
        })
        .require();
    let mut out = [0x91; 256];
    tick(&clock);
    assert_eq!(writer.poll(&mut out), Poll::Pending);
    assert_eq!(out, [0x91; 256]);
    let at = model.borrow().log.len();
    model.borrow_mut().fail = Some(at);
    tick(&clock);
    assert_eq!(writer.poll(&mut out), Poll::Ready(Err(Error::Io)));
    assert_eq!(out, [0x91; 256]);
    model.borrow_mut().fail = None;
    let token = settle(&chip, &mut writer, &clock);
    writer
        .begin(MediumOp::Read {
            offset: 0,
            length: 7,
        })
        .require();
    for _ in 0..5 {
        tick(&clock);
        if let Poll::Ready(r) = writer.poll(&mut out) {
            assert_eq!(r, Ok(Completion::Read(7)));
            break;
        }
    }
    assert_eq!(&out[..7], &[255; 7]);
    assert_eq!(&out[7..], &[0x91; 249]);
    chip.attest_recovery_complete(token).require();
}

// Opening assets at Sync or before it would violate recovery-only access.
#[test]
fn test_should_drain_then_scan_privately_until_explicit_recovery_completion() {
    let (chip, model, clock) = fixture(256);
    let mut writer = chip.claim_writer().require();
    let token = chip.begin_recovery().require();
    assert!(chip.attest_recovery_complete(token).is_err());
    writer.begin(MediumOp::Sync).require();
    assert!(chip.attest_recovery_complete(token).is_err());
    finish(&mut writer, &clock, Completion::Synced);
    assert_eq!(chip.read_asset(0, &mut [0; 1]), Err(Fault::NeedsRecovery));
    writer
        .begin(MediumOp::Read {
            offset: 0,
            length: 1,
        })
        .require();
    assert!(chip.attest_recovery_complete(token).is_err());
    finish(&mut writer, &clock, Completion::Read(1));
    chip.attest_recovery_complete(token).require();
    assert_eq!(chip.read_asset(0, &mut [0; 1]), Ok(()));
    assert!(chip.attest_recovery_complete(token).is_err());
    assert_eq!(chip.read_asset(0x1000, &mut [0; 1]), Err(Fault::Bounds));
    model.borrow_mut().faults.stuck = true;
    let next = chip.begin_recovery().require();
    writer.begin(MediumOp::Sync).require();
    clock.set(clock.get() + 1001);
    assert_eq!(writer.poll(&mut [0; 256]), Poll::Ready(Err(Error::Io)));
    let before = model.borrow().log.len();
    assert!(chip.attest_recovery_complete(next).is_err());
    assert_eq!(chip.read_asset(0, &mut [0; 1]), Err(Fault::NeedsRecovery));
    assert_eq!(model.borrow().log.len(), before);
}

// Removing monotonic/deadline validation would issue a command after time became untrusted.
#[test]
fn test_should_fail_closed_on_clock_regression_overflow_and_issue_timeout() {
    for case in 0..3 {
        let (chip, model, clock) = fixture(256);
        let mut writer = chip.claim_writer().require();
        open(&chip, &mut writer, &clock);
        writer.begin(MediumOp::Erase { offset: 0 }).require();
        if case == 0 {
            clock.set(clock.get() - 1);
        } else if case == 1 {
            clock.set(u64::MAX);
        } else {
            for _ in 0..4 {
                tick(&clock);
                assert_eq!(writer.poll(&mut [0; 256]), Poll::Pending);
            }
            clock.set(clock.get() + 101);
        }
        let before = model.borrow().log.len();
        assert_eq!(writer.poll(&mut [0; 256]), Poll::Ready(Err(Error::Io)));
        assert_eq!(model.borrow().log.len(), before);
        assert_eq!(chip.read_asset(0, &mut [0; 1]), Err(Fault::NeedsRecovery));
    }
    let (chip, model, clock) = fixture(1);
    let mut writer = chip.claim_writer().require();
    clock.set(u64::MAX);
    let before = model.borrow().log.len();
    assert_eq!(writer.begin(MediumOp::Sync), Err(StartError::Io));
    assert_eq!(model.borrow().log.len(), before);
}

// A command retried after transport uncertainty can mutate twice; recovery must only drain/read.
#[test]
fn test_should_recover_after_every_program_transaction_fault_without_reissuing_write() {
    for failure in 0..9 {
        let (chip, model, clock) = fixture(256);
        let mut writer = chip.claim_writer().require();
        open(&chip, &mut writer, &clock);
        model.borrow_mut().log.clear();
        model.borrow_mut().fail = Some(failure);
        writer
            .begin(MediumOp::Program {
                offset: 0,
                bytes: &[0x5a; 256],
            })
            .require();
        let mut terminal = Poll::Pending;
        for _ in 0..100 {
            tick(&clock);
            terminal = writer.poll(&mut [0; 256]);
            if terminal.is_ready() {
                break;
            }
        }
        assert_eq!(terminal, Poll::Ready(Err(Error::Io)), "fault {failure}");
        let program_count = model.borrow().log.iter().filter(|v| v[0] == 0x02).count();
        model.borrow_mut().fail = None;
        let token = chip.begin_recovery().require();
        writer.begin(MediumOp::Sync).require();
        let mut result = Poll::Pending;
        for _ in 0..200 {
            tick(&clock);
            result = writer.poll(&mut [0; 256]);
            if result.is_ready() {
                break;
            }
        }
        if matches!(failure, 2 | 3) {
            assert_eq!(
                result,
                Poll::Ready(Err(Error::Io)),
                "armed WEL cannot silently clear"
            );
            assert!(chip.attest_recovery_complete(token).is_err());
        } else {
            assert_eq!(result, Poll::Ready(Ok(Completion::Synced)));
        }
        assert_eq!(
            model.borrow().log.iter().filter(|v| v[0] == 0x02).count(),
            program_count
        );
        assert_eq!(chip.read_asset(0, &mut [0; 1]), Err(Fault::NeedsRecovery));
    }
}

// Real frozen engine can recover a persisted write after adapter error without deadlocking reads.
#[test]
fn test_should_compose_snapshot_fault_drain_scan_and_explicit_completion() {
    let (chip, model, clock) = fixture(256);
    let writer = chip.claim_writer().require();
    let mut engine = IncrementalSnapshot::<_, 256>::new_unrecovered(
        writer,
        Layout {
            start: 0,
            bank_bytes: 8192,
        },
    )
    .require();
    let token = chip.begin_recovery().require();
    engine.begin_recover().require();
    for _ in 0..1000 {
        tick(&clock);
        if let Poll::Ready(r) = engine.poll_recover() {
            r.require();
            break;
        }
    }
    chip.attest_recovery_complete(token).require();
    let id = engine.begin_commit(0, b"settings").require();
    let mut injected = false;
    for _ in 0..5000 {
        tick(&clock);
        if !injected && model.borrow().log.last().is_some_and(|v| v[0] == 0x02) {
            let at = model.borrow().log.len();
            model.borrow_mut().fail = Some(at);
            injected = true;
        }
        if let Poll::Ready(r) = engine.poll_commit(id) {
            assert!(r.is_err());
            break;
        }
    }
    assert!(injected);
    assert_eq!(
        engine.load_committed(&mut [0; 256]),
        Err(Error::NeedsRecovery)
    );
    model.borrow_mut().fail = None;
    let token = chip.begin_recovery().require();
    engine.begin_recover().require();
    for _ in 0..5000 {
        tick(&clock);
        let before = model.borrow().log.len();
        assert_eq!(chip.read_asset(0, &mut [0; 1]), Err(Fault::NeedsRecovery));
        assert_eq!(model.borrow().log.len(), before);
        if let Poll::Ready(r) = engine.poll_recover() {
            r.require();
            break;
        }
    }
    chip.attest_recovery_complete(token).require();
    assert!(engine.load_committed(&mut [0; 256]).is_ok());
    assert_eq!(chip.read_asset(0, &mut [0; 1]), Ok(()));
}

// A successful status is insufficient when verification disagrees with programmed bytes.
#[test]
fn test_should_poison_on_readback_mismatch_and_keep_short_output_untouched() {
    let (chip, model, clock) = fixture(256);
    let mut writer = chip.claim_writer().require();
    open(&chip, &mut writer, &clock);
    writer
        .begin(MediumOp::Program {
            offset: 0,
            bytes: &[0x5a; 256],
        })
        .require();
    let mut done = Poll::Pending;
    for _ in 0..30 {
        tick(&clock);
        if model.borrow().log.iter().any(|v| v[0] == 0x02) {
            model.borrow_mut().faults.corrupt = true;
        }
        done = writer.poll(&mut [0; 256]);
        if done.is_ready() {
            break;
        }
    }
    assert_eq!(done, Poll::Ready(Err(Error::Io)));
    assert_eq!(chip.last_fault(), Ok(Some(Fault::Readback)));
    model.borrow_mut().faults.corrupt = false;
    open(&chip, &mut writer, &clock);
    writer
        .begin(MediumOp::Read {
            offset: 0,
            length: 8,
        })
        .require();
    let mut short = [0x91; 7];
    tick(&clock);
    assert_eq!(writer.poll(&mut short), Poll::Ready(Err(Error::Io)));
    assert_eq!(short, [0x91; 7]);
}

// No caller can create a second writer or convert a malformed proof/partition into binding.
#[test]
fn test_should_validate_profile_attestation_and_retain_dropped_claim() {
    for change in 0..6 {
        let mut c = config(256);
        match change {
            0 => c.protection_mask = 3,
            1 => c.protection_value = 0x80,
            2 => c.poll_interval_ms = 0,
            3 => c.program_max_ms = 0,
            4 => c.capacity = 0x20000,
            _ => c.operation_max_ms = 1,
        }
        assert!(Profile::validate(c).is_err());
    }
    let (chip, _, _) = fixture(256);
    let writer = chip.claim_writer().require();
    abandon(writer);
    assert!(chip.claim_writer().is_err());
    let model = Rc::new(RefCell::new(Model::new()));
    let evidence = Attestation {
        part: "",
        protection: "model",
        timing: "model",
        ownership: "model",
        durability: "model",
    };
    assert!(
        NorChip::new(
            Device(model),
            Clock(Rc::new(Cell::new(0))),
            Profile::validate(config(256)).require(),
            Partition {
                start: 0x1000,
                length: 0x4000
            },
            evidence
        )
        .is_err()
    );
}

#[derive(Debug)]
struct Wire {
    model: Model,
    selected: Option<u8>,
    command: Vec<u8>,
    read: bool,
    events: Vec<(u8, bool)>,
    fail_flush: bool,
    fail_release: bool,
}
#[derive(Debug)]
struct Bus(Rc<RefCell<Wire>>);
impl ErrorType for Bus {
    type Error = embedded_hal::spi::ErrorKind;
}
impl embedded_hal::spi::SpiBus for Bus {
    fn read(&mut self, out: &mut [u8]) -> Result<(), Self::Error> {
        let mut w = self.0.borrow_mut();
        assert_eq!(w.selected, Some(15));
        let command = w.command.clone();
        w.model.command(&command, out);
        w.read = true;
        Ok(())
    }
    fn write(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        let mut w = self.0.borrow_mut();
        assert!(w.selected.is_some());
        w.command.extend_from_slice(bytes);
        Ok(())
    }
    fn transfer(&mut self, out: &mut [u8], bytes: &[u8]) -> Result<(), Self::Error> {
        self.write(bytes)?;
        self.read(out)
    }
    fn transfer_in_place(&mut self, _: &mut [u8]) -> Result<(), Self::Error> {
        Err(Self::Error::Other)
    }
    fn flush(&mut self) -> Result<(), Self::Error> {
        if self.0.borrow().fail_flush {
            Err(Self::Error::Other)
        } else {
            Ok(())
        }
    }
}
#[derive(Debug)]
struct Pin {
    wire: Rc<RefCell<Wire>>,
    id: u8,
}
impl device_core::OutputPin for Pin {
    type Error = bool;
    fn set_level(&mut self, high: bool) -> Result<(), Self::Error> {
        let mut w = self.wire.borrow_mut();
        w.events.push((self.id, high));
        if high {
            if w.selected == Some(self.id) {
                if self.id == 15 {
                    let command = w.command.clone();
                    if !w.read {
                        w.model.command(&command, &mut []);
                    }
                    w.model.log.push(command);
                }
                w.selected = None;
            }
            if w.fail_release {
                return Err(true);
            }
        } else {
            assert!(w.selected.is_none(), "two slaves selected");
            w.selected = Some(self.id);
            w.command.clear();
            w.read = false;
        }
        Ok(())
    }
}
#[derive(Debug)]
struct Delay;
impl embedded_hal::delay::DelayNs for Delay {
    fn delay_ns(&mut self, _: u32) {}
}

// A retained CS/bus borrow would fail the independent LCD write between every worker slice.
#[test]
fn test_should_release_shared_bus_each_poll_and_poison_on_flush_or_release_failure() {
    for failure in [0, 1, 2] {
        let wire = Rc::new(RefCell::new(Wire {
            model: Model::new(),
            selected: None,
            command: Vec::new(),
            read: false,
            events: Vec::new(),
            fail_flush: false,
            fail_release: false,
        }));
        let shared = device_core::spi::SharedBus::new(Bus(wire.clone()), Delay);
        let nor = shared
            .device(Pin {
                wire: wire.clone(),
                id: 15,
            })
            .require();
        let mut lcd = shared
            .device(Pin {
                wire: wire.clone(),
                id: 10,
            })
            .require();
        let clock = Rc::new(Cell::new(0));
        let chip = NorChip::new(
            nor,
            Clock(clock.clone()),
            Profile::validate(config(256)).require(),
            Partition {
                start: 0x1000,
                length: 0x4000,
            },
            Attestation {
                part: "model",
                protection: "model",
                timing: "model",
                ownership: "model",
                durability: "model",
            },
        )
        .require();
        let mut writer = chip.claim_writer().require();
        let token = chip.begin_recovery().require();
        writer.begin(MediumOp::Sync).require();
        for _ in 0..10 {
            tick(&clock);
            if let Poll::Ready(r) = writer.poll(&mut [0; 256]) {
                r.require();
                break;
            }
        }
        chip.attest_recovery_complete(token).require();
        writer
            .begin(MediumOp::Program {
                offset: 0,
                bytes: &[0x5a; 256],
            })
            .require();
        let mut final_result = Poll::Pending;
        for slice in 0..100 {
            let before = wire
                .borrow()
                .events
                .iter()
                .filter(|&&(id, high)| id == 15 && !high)
                .count();
            if slice == 5 && failure == 1 {
                wire.borrow_mut().fail_flush = true;
            }
            if slice == 5 && failure == 2 {
                wire.borrow_mut().fail_release = true;
            }
            tick(&clock);
            final_result = writer.poll(&mut [0; 256]);
            assert_eq!(wire.borrow().selected, None);
            let after = wire
                .borrow()
                .events
                .iter()
                .filter(|&&(id, high)| id == 15 && !high)
                .count();
            assert!(after <= before + 1);
            if final_result.is_ready() {
                break;
            }
            assert!(chip.read_asset(0, &mut [0; 1]).is_err());
            lcd.write(&[0x2c]).require();
        }
        if failure == 0 {
            assert_eq!(final_result, Poll::Ready(Ok(Completion::Programmed)));
        } else {
            assert_eq!(final_result, Poll::Ready(Err(Error::Io)));
            assert!(lcd.write(&[0x2c]).is_err());
            let token = chip.begin_recovery().require();
            writer.begin(MediumOp::Sync).require();
            tick(&clock);
            assert_eq!(writer.poll(&mut [0; 256]), Poll::Ready(Err(Error::Io)));
            assert!(chip.attest_recovery_complete(token).is_err());
        }
    }
}

// A terminal read replay must copy its exact retained bytes again, not return an empty receipt.
#[test]
fn test_should_replay_terminal_read_into_each_output_and_keep_failed_replay_untouched() {
    let (chip, model, clock) = fixture(256);
    let mut writer = chip.claim_writer().require();
    open(&chip, &mut writer, &clock);
    writer
        .begin(MediumOp::Read {
            offset: 0,
            length: 8,
        })
        .require();
    finish(&mut writer, &clock, Completion::Read(8));
    model.borrow_mut().bytes[0] = 0x21;
    assert_eq!(chip.read_asset(0, &mut [0; 1]), Ok(()));
    let before = model.borrow().log.len();
    let mut output = [0x91; 16];
    assert_eq!(
        writer.poll(&mut output),
        Poll::Ready(Ok(Completion::Read(8)))
    );
    assert_eq!(&output[..8], &[255; 8]);
    assert_eq!(&output[8..], &[0x91; 8]);
    let mut short = [0x91; 7];
    assert_eq!(writer.poll(&mut short), Poll::Ready(Err(Error::Io)));
    assert_eq!(short, [0x91; 7]);
    assert_eq!(model.borrow().log.len(), before);
}

// Missing an erase fault boundary or resetting effects on error would falsely release ownership.
#[test]
fn test_should_retain_erase_responsibility_after_each_transaction_fault() {
    for failure in 0..23 {
        let (chip, model, clock) = fixture(256);
        let mut writer = chip.claim_writer().require();
        open(&chip, &mut writer, &clock);
        model.borrow_mut().bytes[0x1000..0x2000].fill(0);
        model.borrow_mut().log.clear();
        model.borrow_mut().fail = Some(failure);
        writer.begin(MediumOp::Erase { offset: 0 }).require();
        let mut terminal = Poll::Pending;
        for _ in 0..100 {
            tick(&clock);
            terminal = writer.poll(&mut [0; 256]);
            if terminal.is_ready() {
                break;
            }
        }
        assert_eq!(terminal, Poll::Ready(Err(Error::Io)), "fault {failure}");
        if failure >= 1 {
            let effect = chip.outstanding_effect().require().require();
            assert_eq!((effect.offset, effect.length), (0, 4096));
            assert_eq!(effect.command_may_have_reached_chip, failure >= 3);
        }
        model.borrow_mut().fail = None;
        let token = chip.begin_recovery().require();
        writer.begin(MediumOp::Sync).require();
        let mut result = Poll::Pending;
        for _ in 0..200 {
            tick(&clock);
            result = writer.poll(&mut [0; 256]);
            if result.is_ready() {
                break;
            }
        }
        if matches!(failure, 1 | 2) {
            assert_eq!(result, Poll::Ready(Err(Error::Io)));
            assert!(chip.attest_recovery_complete(token).is_err());
        } else {
            assert_eq!(result, Poll::Ready(Ok(Completion::Synced)));
        }
        assert_eq!(
            model.borrow().log.iter().filter(|v| v[0] == 0x20).count(),
            usize::from(failure >= 3)
        );
        assert_eq!(chip.read_asset(0, &mut [0; 1]), Err(Fault::NeedsRecovery));
    }
}

// Dropping an active handle cannot cancel an already issued sector command or open another owner.
#[test]
fn test_should_not_release_chip_when_an_issued_operation_is_abandoned() {
    let (chip, model, clock) = fixture(256);
    let mut writer = chip.claim_writer().require();
    open(&chip, &mut writer, &clock);
    writer.begin(MediumOp::Erase { offset: 0 }).require();
    for _ in 0..4 {
        tick(&clock);
        assert_eq!(writer.poll(&mut [0; 256]), Poll::Pending);
    }
    assert_eq!(model.borrow().log.last().require()[0], 0x20);
    abandon(writer);
    assert!(chip.claim_writer().is_err());
    assert_eq!(chip.begin_recovery(), Err(Fault::Busy));
    assert!(chip.read_asset(0, &mut [0; 1]).is_err());
    assert!(
        chip.outstanding_effect()
            .require()
            .require()
            .command_may_have_reached_chip
    );
}

// Rechecking protection after WREN and rejecting unexpected preexisting WIP prevent illegal reads/writes.
#[test]
fn test_should_recheck_armed_protection_and_block_unexpected_busy_reads() {
    let (chip, model, clock) = fixture(256);
    let mut writer = chip.claim_writer().require();
    open(&chip, &mut writer, &clock);
    writer
        .begin(MediumOp::Program {
            offset: 0,
            bytes: &[0x5a; 256],
        })
        .require();
    for _ in 0..3 {
        tick(&clock);
        assert_eq!(writer.poll(&mut [0; 256]), Poll::Pending);
    }
    assert_eq!(model.borrow().log.last().require()[0], 0x06);
    model.borrow_mut().protection = 4;
    tick(&clock);
    assert_eq!(writer.poll(&mut [0; 256]), Poll::Ready(Err(Error::Io)));
    assert!(!model.borrow().log.iter().any(|v| v[0] == 0x02));
    assert!(
        chip.outstanding_effect()
            .require()
            .require()
            .write_enable_may_be_set
    );

    let (chip, model, clock) = fixture(1);
    let mut writer = chip.claim_writer().require();
    open(&chip, &mut writer, &clock);
    model.borrow_mut().log.clear();
    model.borrow_mut().faults.stuck = true;
    writer
        .begin(MediumOp::Read {
            offset: 0,
            length: 1,
        })
        .require();
    tick(&clock);
    assert_eq!(writer.poll(&mut [0; 256]), Poll::Ready(Err(Error::Io)));
    assert_eq!(
        model.borrow().log.iter().map(|v| v[0]).collect::<Vec<_>>(),
        [0x05]
    );
}

// A clock regression at any command boundary must stop before the next physical transaction.
#[test]
fn test_should_reject_clock_regression_at_each_program_boundary() {
    for boundary in 0..9 {
        let (chip, model, clock) = fixture(256);
        let mut writer = chip.claim_writer().require();
        open(&chip, &mut writer, &clock);
        writer
            .begin(MediumOp::Program {
                offset: 0,
                bytes: &[0x5a; 256],
            })
            .require();
        for _ in 0..boundary {
            tick(&clock);
            assert_eq!(writer.poll(&mut [0; 256]), Poll::Pending);
        }
        let before = model.borrow().log.len();
        clock.set(clock.get() - 1);
        let mut output = [0x91; 256];
        assert_eq!(writer.poll(&mut output), Poll::Ready(Err(Error::Io)));
        assert_eq!(output, [0x91; 256]);
        assert_eq!(model.borrow().log.len(), before);
        assert_eq!(chip.last_fault(), Ok(Some(Fault::ClockRegression)));
    }
}

// Poll spacing must suppress SPI while time has not reached the next scheduled status read.
#[test]
fn test_should_wait_between_status_polls_without_holding_the_chip_bus() {
    let (chip, model, clock) = fixture(256);
    let mut writer = chip.claim_writer().require();
    open(&chip, &mut writer, &clock);
    writer.begin(MediumOp::Erase { offset: 0 }).require();
    for _ in 0..4 {
        tick(&clock);
        assert_eq!(writer.poll(&mut [0; 256]), Poll::Pending);
    }
    let before = model.borrow().log.len();
    for _ in 0..20 {
        assert_eq!(writer.poll(&mut [0; 256]), Poll::Pending);
    }
    assert_eq!(model.borrow().log.len(), before);
    tick(&clock);
    assert_eq!(writer.poll(&mut [0; 256]), Poll::Pending);
    assert_eq!(model.borrow().log.len(), before + 1);
}

// Public assets cannot bypass failed reads, stale recovery epochs or private scan-only authority.
#[test]
fn test_should_keep_assets_staged_and_private_recovery_nonmutating() {
    let (chip, model, clock) = fixture(256);
    let mut writer = chip.claim_writer().require();
    open(&chip, &mut writer, &clock);
    let at = model.borrow().log.len();
    model.borrow_mut().fail = Some(at);
    let mut output = [0x91; 8];
    assert_eq!(chip.read_asset(0, &mut output), Err(Fault::Transport));
    assert_eq!(output, [0x91; 8]);
    model.borrow_mut().fail = None;
    let old = settle(&chip, &mut writer, &clock);
    let current = settle(&chip, &mut writer, &clock);
    assert!(chip.attest_recovery_complete(old).is_err());
    let before = model.borrow().log.len();
    assert_eq!(
        writer.begin(MediumOp::Erase { offset: 0 }),
        Err(StartError::Io)
    );
    assert_eq!(model.borrow().log.len(), before);
    assert!(chip.attest_recovery_complete(current).is_err());
    assert_eq!(chip.read_asset(0, &mut output), Err(Fault::NeedsRecovery));
}

// Status settlement alone cannot substitute the exact attested JEDEC identity.
#[test]
fn test_should_fail_identity_before_any_bank_scan_or_write() {
    let model = Rc::new(RefCell::new(Model::new()));
    let clock = Rc::new(Cell::new(0));
    let mut wrong = config(256);
    wrong.id[0] = 0x12;
    let chip = NorChip::new(
        Device(model.clone()),
        Clock(clock.clone()),
        Profile::validate(wrong).require(),
        Partition {
            start: 0x1000,
            length: 0x4000,
        },
        Attestation {
            part: "model",
            protection: "model",
            timing: "model",
            ownership: "model",
            durability: "model",
        },
    )
    .require();
    let mut writer = chip.claim_writer().require();
    let token = chip.begin_recovery().require();
    writer.begin(MediumOp::Sync).require();
    tick(&clock);
    assert_eq!(writer.poll(&mut [0; 256]), Poll::Pending);
    tick(&clock);
    assert_eq!(writer.poll(&mut [0; 256]), Poll::Ready(Err(Error::Io)));
    assert_eq!(chip.last_fault(), Ok(Some(Fault::Identity)));
    assert!(chip.attest_recovery_complete(token).is_err());
    assert_eq!(
        model.borrow().log.iter().map(|v| v[0]).collect::<Vec<_>>(),
        [0x05, 0x9f]
    );
}

#[derive(Debug)]
struct TimedDevice {
    device: Device,
    clock: Rc<Cell<u64>>,
    after: Rc<Cell<Option<u64>>>,
}
impl ErrorType for TimedDevice {
    type Error = embedded_hal::spi::ErrorKind;
}
impl SpiDevice for TimedDevice {
    fn transaction(&mut self, operations: &mut [Operation<'_, u8>]) -> Result<(), Self::Error> {
        let result = self.device.transaction(operations);
        if let Some(after) = self.after.take() {
            self.clock.set(after);
        }
        result
    }
}
type TimedChip = NorChip<TimedDevice, Clock>;
type TimedWriter<'a> = WritableNor<'a, TimedDevice, Clock>;
struct TimedFixture {
    chip: TimedChip,
    model: Rc<RefCell<Model>>,
    clock: Rc<Cell<u64>>,
    after: Rc<Cell<Option<u64>>>,
}
fn timed_fixture() -> TimedFixture {
    let model = Rc::new(RefCell::new(Model::new()));
    let clock = Rc::new(Cell::new(0));
    let after = Rc::new(Cell::new(None));
    let chip = NorChip::new(
        TimedDevice {
            device: Device(model.clone()),
            clock: clock.clone(),
            after: after.clone(),
        },
        Clock(clock.clone()),
        Profile::validate(config(1)).require(),
        Partition {
            start: 0x1000,
            length: 0x4000,
        },
        Attestation {
            part: "timed model",
            protection: "model",
            timing: "model",
            ownership: "model",
            durability: "model",
        },
    )
    .require();
    TimedFixture {
        chip,
        model,
        clock,
        after,
    }
}
fn open_timed(fixture: &TimedFixture, writer: &mut TimedWriter<'_>) {
    let token = fixture.chip.begin_recovery().require();
    writer.begin(MediumOp::Sync).require();
    assert_eq!(writer.poll(&mut [0; 256]), Poll::Pending);
    assert_eq!(
        writer.poll(&mut [0; 256]),
        Poll::Ready(Ok(Completion::Synced))
    );
    fixture.chip.attest_recovery_complete(token).require();
}
#[derive(Clone, Copy, Debug)]
enum TerminalKind {
    Read,
    Program,
    Erase,
    Sync,
}
impl TerminalKind {
    fn completion(self) -> Completion {
        match self {
            Self::Read => Completion::Read(1),
            Self::Program => Completion::Programmed,
            Self::Erase => Completion::Erased,
            Self::Sync => Completion::Synced,
        }
    }
    fn bound(self) -> u64 {
        if matches!(self, Self::Sync) {
            100
        } else {
            1000
        }
    }
}
fn admit_timed(
    fixture: &TimedFixture,
    writer: &mut TimedWriter<'_>,
    kind: TerminalKind,
) -> Result<Option<RecoveryToken>, StartError> {
    let token = if matches!(kind, TerminalKind::Sync) {
        Some(fixture.chip.begin_recovery().require())
    } else {
        None
    };
    let operation = match kind {
        TerminalKind::Read => MediumOp::Read {
            offset: 0,
            length: 1,
        },
        TerminalKind::Program => MediumOp::Program {
            offset: 0,
            bytes: &[0x5a],
        },
        TerminalKind::Erase => MediumOp::Erase { offset: 0 },
        TerminalKind::Sync => MediumOp::Sync,
    };
    writer.begin(operation)?;
    Ok(token)
}
fn advance_timed(fixture: &TimedFixture, writer: &mut TimedWriter<'_>, count: usize) {
    for _ in 0..count {
        tick(&fixture.clock);
        assert_eq!(writer.poll(&mut [0; 256]), Poll::Pending);
    }
}
fn terminal_stage(
    fixture: &TimedFixture,
    writer: &mut TimedWriter<'_>,
    kind: TerminalKind,
    near_max: bool,
) -> (u64, Option<RecoveryToken>) {
    open_timed(fixture, writer);
    if matches!(kind, TerminalKind::Sync) {
        admit_timed(fixture, writer, TerminalKind::Program).require();
        advance_timed(fixture, writer, 8);
        tick(&fixture.clock);
        assert_eq!(
            writer.poll(&mut [0; 256]),
            Poll::Ready(Ok(Completion::Programmed))
        );
        assert!(fixture.chip.outstanding_effect().require().is_some());
    }
    if near_max {
        fixture.clock.set(u64::MAX - kind.bound());
    }
    let deadline = fixture.clock.get() + kind.bound();
    let token = admit_timed(fixture, writer, kind).require();
    let prefix = match kind {
        TerminalKind::Read | TerminalKind::Sync => 1,
        TerminalKind::Program => 8,
        TerminalKind::Erase => 22,
    };
    advance_timed(fixture, writer, prefix);
    (deadline, token)
}
#[derive(Clone, Copy, Debug)]
enum PostTimeFault {
    Overrun,
    Regression,
    Wrap,
}
fn reject_terminal_post_time(kind: TerminalKind, timing: PostTimeFault) {
    let fixture = timed_fixture();
    let mut writer = fixture.chip.claim_writer().require();
    let (deadline, token) = terminal_stage(
        &fixture,
        &mut writer,
        kind,
        matches!(timing, PostTimeFault::Wrap),
    );
    fixture.clock.set(deadline);
    let (after, diagnostic) = match timing {
        PostTimeFault::Overrun => (deadline + 1, Fault::Deadline),
        PostTimeFault::Regression => (deadline - 1, Fault::ClockRegression),
        PostTimeFault::Wrap => (0, Fault::ClockRegression),
    };
    fixture.after.set(Some(after));
    let before = fixture.model.borrow().log.len();
    let effect = fixture.chip.outstanding_effect().require();
    let mut output = [0x91; 256];
    assert_eq!(
        writer.poll(&mut output),
        Poll::Ready(Err(Error::Io)),
        "{kind:?} {timing:?}"
    );
    assert_eq!(output, [0x91; 256]);
    assert_eq!(fixture.model.borrow().log.len(), before + 1);
    assert_eq!(fixture.chip.last_fault(), Ok(Some(diagnostic)));
    assert_eq!(fixture.chip.outstanding_effect().require(), effect);
    if let Some(token) = token {
        assert!(fixture.chip.attest_recovery_complete(token).is_err());
    }
    assert_eq!(
        fixture.chip.read_asset(0, &mut output[..1]),
        Err(Fault::NeedsRecovery)
    );
    assert_eq!(fixture.model.borrow().log.len(), before + 1);
}
macro_rules! terminal_post_time_test {
    ($name:ident, $kind:ident, $timing:ident) => {
        #[test]
        fn $name() {
            reject_terminal_post_time(TerminalKind::$kind, PostTimeFault::$timing);
        }
    };
}
terminal_post_time_test!(test_should_reject_read_post_spi_overrun, Read, Overrun);
terminal_post_time_test!(
    test_should_reject_program_post_spi_overrun,
    Program,
    Overrun
);
terminal_post_time_test!(test_should_reject_erase_post_spi_overrun, Erase, Overrun);
terminal_post_time_test!(test_should_reject_sync_post_spi_overrun, Sync, Overrun);
terminal_post_time_test!(
    test_should_reject_read_post_spi_regression,
    Read,
    Regression
);
terminal_post_time_test!(
    test_should_reject_program_post_spi_regression,
    Program,
    Regression
);
terminal_post_time_test!(
    test_should_reject_erase_post_spi_regression,
    Erase,
    Regression
);
terminal_post_time_test!(
    test_should_reject_sync_post_spi_regression,
    Sync,
    Regression
);
terminal_post_time_test!(test_should_reject_read_post_spi_clock_wrap, Read, Wrap);
terminal_post_time_test!(
    test_should_reject_program_post_spi_clock_wrap,
    Program,
    Wrap
);
terminal_post_time_test!(test_should_reject_erase_post_spi_clock_wrap, Erase, Wrap);
terminal_post_time_test!(test_should_reject_sync_post_spi_clock_wrap, Sync, Wrap);

// Inclusive deadline permits finite transactions finishing exactly at the bound, including MAX.
#[test]
fn test_should_accept_exact_post_spi_deadline_without_overflow() {
    for kind in [
        TerminalKind::Read,
        TerminalKind::Program,
        TerminalKind::Erase,
        TerminalKind::Sync,
    ] {
        for near_max in [false, true] {
            let fixture = timed_fixture();
            let mut writer = fixture.chip.claim_writer().require();
            let (deadline, token) = terminal_stage(&fixture, &mut writer, kind, near_max);
            fixture.clock.set(deadline - 1);
            fixture.after.set(Some(deadline));
            let mut output = [0x91; 256];
            assert_eq!(writer.poll(&mut output), Poll::Ready(Ok(kind.completion())));
            if matches!(kind, TerminalKind::Read) {
                assert_eq!(output[0], 255);
            } else {
                assert_eq!(output, [0x91; 256]);
            }
            if let Some(token) = token {
                fixture.chip.attest_recovery_complete(token).require();
            }
        }
    }
}

// All four admissions must reject an unrepresentable absolute bound without SPI.
#[test]
fn test_should_reject_overflowing_terminal_operation_admission_without_spi() {
    for kind in [
        TerminalKind::Read,
        TerminalKind::Program,
        TerminalKind::Erase,
        TerminalKind::Sync,
    ] {
        let fixture = timed_fixture();
        let mut writer = fixture.chip.claim_writer().require();
        open_timed(&fixture, &mut writer);
        fixture.clock.set(u64::MAX - kind.bound() + 1);
        let before = fixture.model.borrow().log.len();
        assert_eq!(
            admit_timed(&fixture, &mut writer, kind),
            Err(StartError::Io)
        );
        assert_eq!(fixture.chip.last_fault(), Ok(Some(Fault::DeadlineOverflow)));
        assert_eq!(fixture.model.borrow().log.len(), before);
    }
}

fn reject_intermediate_post_time(
    kind: TerminalKind,
    prefix: usize,
    before: u64,
    after: u64,
    opcode: u8,
    armed: bool,
    issued: bool,
) {
    let fixture = timed_fixture();
    let mut writer = fixture.chip.claim_writer().require();
    open_timed(&fixture, &mut writer);
    admit_timed(&fixture, &mut writer, kind).require();
    advance_timed(&fixture, &mut writer, prefix);
    fixture.clock.set(before);
    fixture.after.set(Some(after));
    let count = fixture.model.borrow().log.len();
    let mut output = [0x91; 256];
    assert_eq!(writer.poll(&mut output), Poll::Ready(Err(Error::Io)));
    assert_eq!(fixture.chip.last_fault(), Ok(Some(Fault::Deadline)));
    assert_eq!(output, [0x91; 256]);
    assert_eq!(fixture.model.borrow().log.len(), count + 1);
    assert_eq!(fixture.model.borrow().log.last().require()[0], opcode);
    let effect = fixture.chip.outstanding_effect().require();
    if armed {
        let effect = effect.require();
        assert_eq!(
            (effect.offset, effect.length),
            (
                0,
                if matches!(kind, TerminalKind::Erase) {
                    4096
                } else {
                    1
                }
            )
        );
        assert_eq!(effect.command_may_have_reached_chip, issued);
        if opcode == 0x05 && issued {
            assert!(effect.write_enable_may_be_set);
        }
    } else {
        assert!(effect.is_none());
    }
    assert_eq!(
        fixture.chip.read_asset(0, &mut output[..1]),
        Err(Fault::NeedsRecovery)
    );
    assert_eq!(fixture.model.borrow().log.len(), count + 1);
}
#[test]
fn test_should_reject_status_crossing_before_accepting_idle() {
    reject_intermediate_post_time(TerminalKind::Program, 0, 1000, 1001, 0x05, false, false);
}
#[test]
fn test_should_reject_old_read_crossing_before_wren() {
    reject_intermediate_post_time(TerminalKind::Program, 1, 1000, 1001, 0x03, false, false);
}
#[test]
fn test_should_retain_armed_effect_after_wren_crosses_deadline() {
    reject_intermediate_post_time(TerminalKind::Program, 2, 1000, 1001, 0x06, true, false);
}
#[test]
fn test_should_reject_wel_status_crossing_before_array_command() {
    reject_intermediate_post_time(TerminalKind::Program, 3, 1000, 1001, 0x05, true, false);
}
#[test]
fn test_should_retain_issued_effect_after_spi_crosses_physical_issue_bound() {
    reject_intermediate_post_time(TerminalKind::Program, 4, 20, 121, 0x02, true, true);
}
#[test]
fn test_should_not_discard_issue_deadline_after_late_idle_status() {
    reject_intermediate_post_time(TerminalKind::Program, 7, 105, 106, 0x05, true, true);
}
#[test]
fn test_should_reject_intermediate_erase_readback_crossing() {
    reject_intermediate_post_time(TerminalKind::Erase, 7, 1000, 1001, 0x03, true, true);
}

// The same SPI read helper must not publish assets after a late/regressed transaction.
#[test]
fn test_should_keep_asset_output_private_when_post_spi_time_is_invalid() {
    for after in [1002, 0] {
        let fixture = timed_fixture();
        let mut writer = fixture.chip.claim_writer().require();
        open_timed(&fixture, &mut writer);
        fixture.clock.set(1);
        fixture.after.set(Some(after));
        let mut output = [0x91; 8];
        let expected = if after == 0 {
            Fault::ClockRegression
        } else {
            Fault::Deadline
        };
        assert_eq!(fixture.chip.read_asset(0, &mut output), Err(expected));
        assert_eq!(output, [0x91; 8]);
        assert_eq!(
            fixture.chip.read_asset(0, &mut output),
            Err(Fault::NeedsRecovery)
        );
    }
}

#[cfg(feature = "experimental-candidate")]
#[test]
fn test_should_keep_experimental_policy_closed_and_check_real_identity_before_recovery() {
    let model = Rc::new(RefCell::new(Model::new()));
    let time = Rc::new(Cell::new(0));
    let mut policy = config(256);
    policy.id = [0xee, 0x40, 0x10];
    let chip = NorChip::new_experimental(
        Device(model.clone()),
        Clock(time.clone()),
        policy,
        Partition {
            start: 0x1000,
            length: 0x4000,
        },
    )
    .require();
    let mut writer = chip.claim_writer().require();
    assert!(
        writer
            .begin(MediumOp::Program {
                offset: 0,
                bytes: &[0; 256]
            })
            .is_err()
    );
    let _token = chip.begin_recovery().require();
    writer.begin(MediumOp::Sync).require();
    for _ in 0..30 {
        tick(&time);
        if let Poll::Ready(result) = writer.poll(&mut [0; 256]) {
            assert_eq!(result, Err(Error::Io));
            assert_eq!(chip.last_fault().require(), Some(Fault::Identity));
            assert!(
                model
                    .borrow()
                    .log
                    .iter()
                    .all(|c| !matches!(c[0], 0x01 | 0x06 | 0x02 | 0x20))
            );
            return;
        }
    }
    panic!("identity check never terminated");
}

#[test]
fn test_should_validate_expected_completion_before_publishing_read_bytes() {
    let (chip, _, clock) = fixture(256);
    let mut writer = chip.claim_writer().require();
    open(&chip, &mut writer, &clock);
    writer
        .begin(MediumOp::Read {
            offset: 0,
            length: 7,
        })
        .require();
    let mut out = [0x91; 256];
    let mut ended = false;
    for _ in 0..20 {
        tick(&clock);
        match writer.poll_expected(Completion::Synced, &mut out) {
            Poll::Pending => assert_eq!(out, [0x91; 256]),
            Poll::Ready(result) => {
                assert!(result.is_err());
                ended = true;
                break;
            }
        }
    }
    assert!(ended);
    assert_eq!(out, [0x91; 256]);
    open(&chip, &mut writer, &clock);
    writer
        .begin(MediumOp::Read {
            offset: 0,
            length: 7,
        })
        .require();
    ended = false;
    for _ in 0..20 {
        tick(&clock);
        match writer.poll_expected(Completion::Read(7), &mut out) {
            Poll::Pending => assert_eq!(out, [0x91; 256]),
            Poll::Ready(result) => {
                assert_eq!(result, Ok(Completion::Read(7)));
                ended = true;
                break;
            }
        }
    }
    assert!(ended);
    assert_eq!(&out[..7], &[255; 7]);
    assert_eq!(&out[7..], &[0x91; 249]);
}

#[test]
fn test_should_retry_expected_read_after_short_output_without_advancing_spi() {
    let (chip, model, clock) = fixture(256);
    let mut writer = chip.claim_writer().require();
    open(&chip, &mut writer, &clock);
    writer
        .begin(MediumOp::Read {
            offset: 0,
            length: 7,
        })
        .require();
    let calls = model.borrow().log.len();
    assert_eq!(
        writer.poll_expected(Completion::Read(7), &mut [0x91; 6]),
        Poll::Ready(Err(Error::Capacity))
    );
    assert_eq!(model.borrow().log.len(), calls);
    let mut out = [0x91; 7];
    let mut ended = false;
    for _ in 0..20 {
        tick(&clock);
        if let Poll::Ready(result) = writer.poll_expected(Completion::Read(7), &mut out) {
            assert_eq!(result, Ok(Completion::Read(7)));
            ended = true;
            break;
        }
    }
    assert!(ended);
    assert_eq!(out, [255; 7]);
}

#[test]
fn test_should_not_retain_two_transfer_pages_in_the_chip_owner() {
    assert!(
        core::mem::size_of::<Chip>() <= 512,
        "chip contains a duplicate resident transfer page"
    );
}
