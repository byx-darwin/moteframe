//! Initialization gate behavior on the real algorithm and an observable chip-byte model.
#![allow(
    clippy::unwrap_used,
    reason = "Unexpected fixture construction should fail the test"
)]
use syd881x_radio::{
    Bank, ByteRegisters, ConfigurationTable, InitializationFailure, InitializationStage, Radio,
    RadioError, Register,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Event {
    DirectRead(Bank, u8),
    DirectWrite(Bank, u8, u8),
    RfRead(u8, u8),
    RfWrite(u8, u8, u8),
}
#[derive(Debug)]
struct Chip {
    bytes: [[u8; 256]; 5],
    rf: [[u8; 128]; 3],
    bank: u8,
    address: u8,
    samples: u8,
    transaction: usize,
    fault_transaction: Option<usize>,
    bridge_stuck: bool,
    reset_stuck: bool,
    pre_ready: u8,
    post_ready: u8,
    post_pending_reads: usize,
    events: Vec<Event>,
}
impl Default for Chip {
    fn default() -> Self {
        let mut chip = Self {
            bytes: [[0; 256]; 5],
            rf: [[0; 128]; 3],
            bank: 0,
            address: 0,
            samples: 0,
            transaction: 0,
            fault_transaction: None,
            bridge_stuck: false,
            reset_stuck: false,
            pre_ready: 2,
            post_ready: 4,
            post_pending_reads: 0,
            events: Vec::new(),
        };
        chip.bytes[0][0x1a] = 0xff;
        chip.bytes[3][0] = 0xb8;
        chip.bytes[3][8] = 0xff;
        chip.bytes[3][0x0a] = 0xff;
        chip.rf[0][0x3d] = 0x95;
        chip.rf[2][0] = 0xab;
        chip
    }
}
const fn index(bank: Bank) -> usize {
    match bank {
        Bank::Ll => 0,
        Bank::Modem => 1,
        Bank::Misc => 2,
        Bank::PowerOn => 3,
        Bank::Interface => 4,
    }
}
impl ByteRegisters for Chip {
    type Error = ();
    fn read8(&mut self, register: Register) -> Result<u8, ()> {
        let (bank, offset) = (register.bank(), register.offset());
        if bank == Bank::Misc && offset == 0x1a {
            return Ok(u8::from(
                !(self.bridge_stuck || self.fault_transaction == Some(self.transaction)),
            ));
        }
        if bank == Bank::Misc && offset == 0x1c {
            self.samples += 1;
            if self.samples == 2 {
                self.events.push(Event::RfRead(self.bank, self.address));
            }
            return Ok(match (self.bank, self.address) {
                (0, 0x3b) => self.pre_ready,
                (1, 0x0a) => {
                    if self.post_pending_reads > 0 {
                        if self.samples == 2 {
                            self.post_pending_reads -= 1;
                        }
                        2
                    } else {
                        self.post_ready
                    }
                }
                _ => self.rf[usize::from(self.bank)][usize::from(self.address)],
            });
        }
        self.events.push(Event::DirectRead(bank, offset));
        Ok(self.bytes[index(bank)][usize::from(offset)])
    }
    fn write8(&mut self, register: Register, value: u8) -> Result<(), ()> {
        let (bank, offset) = (register.bank(), register.offset());
        if bank == Bank::Misc {
            match offset {
                0x18 => {
                    self.address = value;
                    self.samples = 0;
                    self.transaction += 1;
                }
                0x19 => {
                    self.events
                        .push(Event::RfWrite(self.bank, self.address, value));
                    if self.address == 0x7f {
                        self.bank = value;
                    } else {
                        self.rf[usize::from(self.bank)][usize::from(self.address)] = value;
                    }
                }
                _ => {}
            }
            return Ok(());
        }
        self.events.push(Event::DirectWrite(bank, offset, value));
        self.bytes[index(bank)][usize::from(offset)] =
            if bank == Bank::Ll && offset == 0 && !self.reset_stuck {
                value & !1
            } else {
                value
            };
        Ok(())
    }
}
fn table(header: u8) -> [u8; 15] {
    [
        0xff, header, 0, 0, 3, 0, 3, 0x7f, 2, 3, 0x10, 0x12, 0, 0x0b, 0x40,
    ]
}

#[test]
fn test_should_wake_a_sleeping_radio_before_the_first_rf_bridge_transaction() {
    let mut chip = Chip::default();
    chip.bytes[3][8] = 0x62; // preserve reserved bit6, manual sleep
    let mut radio = Radio::new(chip, 2).unwrap();
    let mut delays = Vec::new();
    radio.prepare_standby(|us| delays.push(us)).unwrap();
    let chip = radio.into_inner();
    assert_eq!(delays, [100, 500]);
    assert_eq!(chip.bytes[3][8], 0xe4);
    assert_eq!(chip.bytes[3][0x0a] & 1, 0);
    assert_eq!(chip.bytes[0][0x1a] & 7, 0);
    let writes: Vec<_> = chip
        .events
        .iter()
        .filter_map(|event| match event {
            Event::DirectWrite(bank, offset, value) => Some((*bank, *offset, *value)),
            _ => None,
        })
        .collect();
    assert_eq!(
        writes,
        [
            (Bank::Ll, 0x1a, 0xf8),
            (Bank::PowerOn, 0x0a, 0xfe),
            (Bank::PowerOn, 8, 0xe2),
            (Bank::Ll, 0, 1),
            (Bank::PowerOn, 8, 0xe4),
            (Bank::PowerOn, 8, 0xe4),
        ]
    );
    assert!(
        !chip
            .events
            .iter()
            .any(|event| matches!(event, Event::RfRead(..) | Event::RfWrite(..)))
    );
}

#[test]
fn test_should_initialize_ordered_hardware_gates_and_disable_sleep_without_enabling_adv() {
    let bytes = table(0x20);
    let mut radio = Radio::new(Chip::default(), 2).unwrap();
    let report = radio
        .initialize(&ConfigurationTable::new(&bytes).unwrap())
        .unwrap();
    assert_eq!(
        (
            report.reset_polls,
            report.pre_calibration_polls,
            report.post_calibration_polls
        ),
        (1, 1, 1)
    );
    let chip = radio.into_inner();
    assert_eq!(chip.bytes[0][0x10] & 1, 0);
    assert_eq!(chip.bytes[0][0x1a], 0xf8);
    assert_eq!(chip.bytes[3][0x0a], 0xfe);
    assert_eq!(chip.bytes[3][8], 0xdf);
    assert_eq!(chip.bytes[3][0], 0xb8);
    assert_eq!(chip.rf[0][0x3d], 0x95);
    assert_eq!(chip.rf[0][0x40], 0);
    assert_eq!(chip.rf[2][0], 0xaa);
    assert_eq!((chip.rf[0][3], chip.rf[0][4]), (0, 15));
    assert_eq!(chip.bank, 0);
    let writes: Vec<_> = chip
        .events
        .iter()
        .filter(|event| matches!(event, Event::RfWrite(..) | Event::DirectWrite(..)))
        .copied()
        .collect();
    assert_eq!(
        writes,
        [
            Event::DirectWrite(Bank::Ll, 0, 1),
            Event::RfWrite(0, 0x7f, 0),
            Event::RfWrite(0, 0x3d, 0xd5),
            Event::RfWrite(0, 0x7f, 0),
            Event::RfWrite(0, 0x3b, 1),
            Event::RfWrite(0, 0x7f, 0),
            Event::RfWrite(0, 0x3d, 0x95),
            Event::RfWrite(0, 0x7f, 0),
            Event::RfWrite(0, 0x40, 0),
            Event::DirectWrite(Bank::PowerOn, 0, 0xb8),
            Event::RfWrite(0, 0x7f, 2),
            Event::RfWrite(2, 0x10, 0x12),
            Event::DirectWrite(Bank::Ll, 0x0b, 0x40),
            Event::RfWrite(2, 0x7f, 1),
            Event::RfWrite(1, 0x0a, 2),
            Event::RfWrite(1, 0x7f, 2),
            Event::RfWrite(2, 0, 0xaa),
            Event::RfWrite(2, 0x7f, 0),
            Event::RfWrite(0, 3, 0),
            Event::RfWrite(0, 4, 15),
            Event::DirectWrite(Bank::Ll, 0x1a, 0xf8),
            Event::DirectWrite(Bank::PowerOn, 0x0a, 0xfe),
            Event::DirectWrite(Bank::PowerOn, 8, 0xdf)
        ]
    );
}
#[test]
fn test_should_handle_xosc_and_each_lpo_branch_without_cached_rom_state() {
    for (header, mcu, lpo, rf3, rf4) in [(0x08, 8, 0, 0, 0), (0x17, 7, 1, 15, 0)] {
        let bytes = table(header);
        let mut radio = Radio::new(Chip::default(), 1).unwrap();
        radio
            .initialize(&ConfigurationTable::new(&bytes).unwrap())
            .unwrap();
        let chip = radio.into_inner();
        assert_eq!(chip.rf[0][0x40], u8::from(mcu == 8));
        assert_eq!(chip.bytes[3][0], if mcu == 8 { 0xb8 } else { 0xbf });
        assert_eq!(chip.rf[2][0], 0xa8 | lpo);
        assert_eq!((chip.rf[0][3], chip.rf[0][4]), (rf3, rf4));
    }
}
#[test]
fn test_should_reject_unsupported_clock_headers_before_all_mmio() {
    for header in [0x09, 0x0f, 0x30, 0x70] {
        let bytes = table(header);
        let mut radio = Radio::new(Chip::default(), 1).unwrap();
        let error = radio
            .initialize(&ConfigurationTable::new(&bytes).unwrap())
            .unwrap_err();
        assert_eq!(error.stage, InitializationStage::Validate);
        assert!(matches!(
            error.cause,
            InitializationFailure::InvalidHeader { .. }
        ));
        let chip = radio.into_inner();
        assert!(chip.events.is_empty());
        assert_eq!(chip.transaction, 0);
    }
}
#[test]
fn test_should_bound_reset_self_clear_and_stop_before_rf() {
    let bytes = table(0);
    let mut radio = Radio::new(
        Chip {
            reset_stuck: true,
            ..Chip::default()
        },
        1,
    )
    .unwrap();
    let error = radio
        .initialize(&ConfigurationTable::new(&bytes).unwrap())
        .unwrap_err();
    assert_eq!(error.stage, InitializationStage::Reset);
    assert_eq!(
        error.cause,
        InitializationFailure::ResetTimeout {
            last_status: 1,
            polls: 4096
        }
    );
    let chip = radio.into_inner();
    assert_eq!(chip.transaction, 0);
    assert_eq!(
        chip.events
            .iter()
            .filter(|&&event| event == Event::DirectRead(Bank::Ll, 0))
            .count(),
        4097
    );
}
#[test]
fn test_should_bound_precalibration_and_clear_bit_six_on_failure() {
    let bytes = table(0);
    let mut radio = Radio::new(
        Chip {
            pre_ready: 1,
            ..Chip::default()
        },
        1,
    )
    .unwrap();
    let error = radio
        .initialize(&ConfigurationTable::new(&bytes).unwrap())
        .unwrap_err();
    assert_eq!(error.stage, InitializationStage::PreCalibration);
    assert_eq!(
        error.cause,
        InitializationFailure::PreCalibrationTimeout {
            last_status: 1,
            polls: 256
        }
    );
    assert_eq!(error.calibration_cleanup, None);
    assert_eq!(error.advertising_cleanup, None);
    let chip = radio.into_inner();
    assert_eq!(
        chip.events
            .iter()
            .filter(|&&event| event == Event::RfRead(0, 0x3b))
            .count(),
        256
    );
    assert_eq!(chip.rf[0][0x3d] & 0x40, 0);
    assert!(
        !chip
            .events
            .contains(&Event::DirectWrite(Bank::Ll, 0x0b, 0x40))
    );
    assert_eq!(
        chip.events.last(),
        Some(&Event::DirectWrite(Bank::Ll, 0x10, 0))
    );
}
#[test]
fn test_should_bound_postcalibration_and_omit_lpo_sleep_changes_after_failure() {
    let bytes = table(0x20);
    let mut radio = Radio::new(
        Chip {
            post_ready: 8,
            ..Chip::default()
        },
        1,
    )
    .unwrap();
    let error = radio
        .initialize(&ConfigurationTable::new(&bytes).unwrap())
        .unwrap_err();
    assert_eq!(error.stage, InitializationStage::PostCalibration);
    assert_eq!(
        error.cause,
        InitializationFailure::PostCalibrationTimeout {
            last_status: 8,
            polls: 256
        }
    );
    let chip = radio.into_inner();
    assert_eq!(
        chip.events
            .iter()
            .filter(|&&event| event == Event::RfRead(1, 0x0a))
            .count(),
        256
    );
    assert_eq!(chip.rf[2][0], 0xab);
    assert_eq!(chip.bytes[0][0x1a], 0xff);
}
#[test]
fn test_should_distinguish_bridge_failure_and_preserve_secondary_cleanup_failure() {
    let bytes = table(0);
    let mut radio = Radio::new(
        Chip {
            fault_transaction: Some(6),
            ..Chip::default()
        },
        1,
    )
    .unwrap();
    let error = radio
        .initialize(&ConfigurationTable::new(&bytes).unwrap())
        .unwrap_err();
    assert_eq!(error.stage, InitializationStage::PreCalibration);
    assert_eq!(
        error.cause,
        InitializationFailure::Radio(RadioError::RfTimeout)
    );
    assert_eq!(error.calibration_cleanup, None);
    assert_eq!(radio.into_inner().rf[0][0x3d] & 0x40, 0);
    let mut radio = Radio::new(
        Chip {
            bridge_stuck: true,
            ..Chip::default()
        },
        1,
    )
    .unwrap();
    let error = radio
        .initialize(&ConfigurationTable::new(&bytes).unwrap())
        .unwrap_err();
    assert_eq!(
        error.cause,
        InitializationFailure::Radio(RadioError::RfTimeout)
    );
    assert_eq!(error.calibration_cleanup, Some(RadioError::RfTimeout));
    assert_eq!(error.advertising_cleanup, None);
}

#[test]
fn test_should_allow_finite_slow_calibration_without_claiming_an_elapsed_time() {
    use core::num::NonZeroU16;
    let bytes = table(0);
    let make = || Chip {
        post_pending_reads: 300,
        ..Chip::default()
    };
    let mut baseline = Radio::new(make(), 1).unwrap();
    assert_eq!(
        baseline
            .initialize(&ConfigurationTable::new(&bytes).unwrap())
            .unwrap_err()
            .stage,
        InitializationStage::PostCalibration
    );
    let mut radio = Radio::new(make(), 1).unwrap();
    let report = radio
        .initialize_with_calibration_budget(
            &ConfigurationTable::new(&bytes).unwrap(),
            NonZeroU16::new(512).unwrap(),
        )
        .unwrap();
    assert_eq!(report.post_calibration_polls, 301);
    let mut stuck = Radio::new(
        Chip {
            post_ready: 2,
            ..Chip::default()
        },
        1,
    )
    .unwrap();
    let error = stuck
        .initialize_with_calibration_budget(
            &ConfigurationTable::new(&bytes).unwrap(),
            NonZeroU16::new(512).unwrap(),
        )
        .unwrap_err();
    assert_eq!(
        error.cause,
        InitializationFailure::PostCalibrationTimeout {
            last_status: 2,
            polls: 512
        }
    );
}
