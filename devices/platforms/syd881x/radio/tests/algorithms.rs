//! ROM-derived register algorithm behavior with an observable byte adapter.
#![allow(
    clippy::unwrap_used,
    reason = "Unexpected fixture or operation failures should abort tests"
)]
use std::collections::VecDeque;
use syd881x_radio::{
    AdvertisingParameters, AdvertisingPdu, Bank, ByteRegisters, ConfigurationTable,
    InvalidConfiguration, Radio, RadioError, Register, StaticRandomAddress, Status,
};

#[derive(Debug, PartialEq, Eq)]
enum Access {
    Read(Register),
    Write(Register, u8),
}
#[derive(Debug, Default)]
struct Registers {
    reads: VecDeque<(Register, Result<u8, ()>)>,
    accesses: Vec<Access>,
    fail_write: bool,
}
impl ByteRegisters for Registers {
    type Error = ();
    fn read8(&mut self, register: Register) -> Result<u8, ()> {
        self.accesses.push(Access::Read(register));
        let (expected, result) = self.reads.pop_front().unwrap();
        assert_eq!(register, expected, "unexpected access order");
        result
    }
    fn write8(&mut self, register: Register, value: u8) -> Result<(), ()> {
        self.accesses.push(Access::Write(register, value));
        if self.fail_write { Err(()) } else { Ok(()) }
    }
}
fn misc(offset: u8) -> Register {
    Register::new(Bank::Misc, offset)
}
fn fixture(reads: &[(Register, u8)]) -> Registers {
    Registers {
        reads: reads
            .iter()
            .map(|&(register, value)| (register, Ok(value)))
            .collect(),
        ..Registers::default()
    }
}

#[test]
fn test_should_write_rf_in_rom_order_preserving_control_bits() {
    let mut radio = Radio::new(
        fixture(&[(misc(0x1a), 0xa0), (misc(0x1a), 0), (misc(0x1a), 1)]),
        2,
    )
    .unwrap();
    assert_eq!(radio.rf_write(0x7f, 2), Ok(()));
    let registers = radio.into_inner();
    assert!(registers.reads.is_empty());
    assert_eq!(
        registers.accesses,
        [
            Access::Read(misc(0x1a)),
            Access::Write(misc(0x1a), 0xa1),
            Access::Write(misc(0x18), 0x7f),
            Access::Write(misc(0x19), 2),
            Access::Read(misc(0x1a)),
            Access::Read(misc(0x1a))
        ]
    );
}
#[test]
fn test_should_return_completed_rf_value_not_early_sample() {
    let mut radio = Radio::new(
        fixture(&[
            (misc(0x1a), 0),
            (misc(0x1c), 0xaa),
            (misc(0x1a), 1),
            (misc(0x1c), 0x55),
        ]),
        1,
    )
    .unwrap();
    assert_eq!(radio.rf_read(0x3d), Ok(0x55));
    assert_eq!(
        radio.into_inner().accesses,
        [
            Access::Read(misc(0x1a)),
            Access::Write(misc(0x1a), 1),
            Access::Write(misc(0x18), 0x3d),
            Access::Read(misc(0x1c)),
            Access::Read(misc(0x1a)),
            Access::Read(misc(0x1c))
        ]
    );
}
#[test]
fn test_should_stop_rf_polling_at_budget_without_stale_read_success() {
    let mut radio = Radio::new(
        fixture(&[
            (misc(0x1a), 0),
            (misc(0x1c), 0x77),
            (misc(0x1a), 0),
            (misc(0x1a), 0),
        ]),
        2,
    )
    .unwrap();
    assert_eq!(radio.rf_read(1), Err(RadioError::RfTimeout));
    assert!(radio.into_inner().reads.is_empty());
}
#[test]
fn test_should_reject_zero_budget_without_access_and_propagate_adapter_error() {
    assert!(matches!(
        Radio::new(Registers::default(), 0),
        Err(RadioError::InvalidParameters)
    ));
    let registers = Registers {
        fail_write: true,
        ..fixture(&[(misc(0x1a), 0)])
    };
    let mut radio = Radio::new(registers, 1).unwrap();
    assert_eq!(radio.rf_write(1, 2), Err(RadioError::Access(())));
    assert_eq!(radio.into_inner().accesses.len(), 2);
}

fn ll(offset: u8) -> Register {
    Register::new(Bank::Ll, offset)
}
fn pdu() -> AdvertisingPdu {
    AdvertisingPdu::nonconnectable(
        StaticRandomAddress::new([1, 2, 3, 4, 5, 0xc6]).unwrap(),
        &[2, 1, 6],
    )
    .unwrap()
}
#[test]
fn test_should_preserve_parameter_bits_write_identity_and_start_separately() {
    let mut radio = Radio::new(
        fixture(&[(ll(0x11), 0xff), (ll(0x10), 0x80), (ll(0x10), 0x82)]),
        1,
    )
    .unwrap();
    let address = StaticRandomAddress::new([1, 2, 3, 4, 5, 0xc6]).unwrap();
    assert_eq!(
        radio.configure_advertising(AdvertisingParameters::new(3, 0x1234, 1).unwrap(), address),
        Ok(())
    );
    assert_eq!(radio.start_advertising(), Ok(()));
    assert_eq!(
        radio.into_inner().accesses,
        [
            Access::Write(ll(0x80), 1),
            Access::Write(ll(0x81), 2),
            Access::Write(ll(0x82), 3),
            Access::Write(ll(0x83), 4),
            Access::Write(ll(0x84), 5),
            Access::Write(ll(0x85), 0xc6),
            Access::Write(ll(0x88), 0xd6),
            Access::Write(ll(0x89), 0xbe),
            Access::Write(ll(0x8a), 0x89),
            Access::Write(ll(0x8b), 0x8e),
            Access::Write(ll(0x15), 1),
            Access::Read(ll(0x11)),
            Access::Write(ll(0x11), 0x83),
            Access::Write(ll(0x12), 0x34),
            Access::Write(ll(0x13), 0x12),
            Access::Read(ll(0x10)),
            Access::Write(ll(0x10), 0x82),
            Access::Write(ll(0x16), 0),
            Access::Read(ll(0x10)),
            Access::Write(ll(0x10), 0x83)
        ]
    );
}
#[test]
fn test_should_reject_invalid_advertising_parameters() {
    for (rounds, interval, channel) in [(0, 1, 1), (128, 1, 1), (1, 0, 1), (1, 1, 0), (1, 1, 8)] {
        assert!(AdvertisingParameters::new(rounds, interval, channel).is_err());
    }
}
#[test]
fn test_should_write_whole_pdu_then_one_commit_and_never_write_when_full() {
    let mut radio = Radio::new(fixture(&[(ll(3), 0)]), 1).unwrap();
    assert_eq!(radio.queue_advertisement(&pdu()), Ok(()));
    assert_eq!(
        radio.into_inner().accesses,
        [
            Access::Read(ll(3)),
            Access::Write(ll(0xc0), 0x42),
            Access::Write(ll(0xc0), 9),
            Access::Write(ll(0xc0), 1),
            Access::Write(ll(0xc0), 2),
            Access::Write(ll(0xc0), 3),
            Access::Write(ll(0xc0), 4),
            Access::Write(ll(0xc0), 5),
            Access::Write(ll(0xc0), 0xc6),
            Access::Write(ll(0xc0), 2),
            Access::Write(ll(0xc0), 1),
            Access::Write(ll(0xc0), 6),
            Access::Write(ll(0xc8), 1)
        ]
    );
    let mut full = Radio::new(fixture(&[(ll(3), 1)]), 1).unwrap();
    assert_eq!(full.queue_advertisement(&pdu()), Err(RadioError::FifoFull));
    assert_eq!(full.into_inner().accesses, [Access::Read(ll(3))]);
}
#[test]
fn test_should_never_commit_after_fifo_adapter_error() {
    let mut radio = Radio::new(
        Registers {
            fail_write: true,
            ..fixture(&[(ll(3), 0)])
        },
        1,
    )
    .unwrap();
    assert_eq!(
        radio.queue_advertisement(&pdu()),
        Err(RadioError::Access(()))
    );
    assert_eq!(
        radio.into_inner().accesses,
        [Access::Read(ll(3)), Access::Write(ll(0xc0), 0x42)]
    );
}
#[test]
fn test_should_bound_status_polling_and_acknowledge_observed_byte_only() {
    let mut radio = Radio::new(
        fixture(&[
            (ll(1), 0),
            (ll(3), 2),
            (ll(0x10), 3),
            (ll(1), 4),
            (ll(3), 0),
            (ll(0x10), 2),
            (ll(1), 0x84),
        ]),
        1,
    )
    .unwrap();
    assert_eq!(
        radio.wait_interrupt(4, 2),
        Ok(Status {
            interrupt_status: 4,
            fifo_status: 0,
            advertising_control: 2
        })
    );
    assert_eq!(radio.acknowledge_interrupt(), Ok(0x84));
    assert_eq!(
        radio.into_inner().accesses.last(),
        Some(&Access::Write(ll(1), 0x84))
    );
    let mut radio = Radio::new(fixture(&[(ll(1), 0), (ll(3), 0), (ll(0x10), 1)]), 1).unwrap();
    assert_eq!(radio.wait_interrupt(4, 1), Err(RadioError::StatusTimeout));
    assert!(radio.into_inner().reads.is_empty());
    let mut radio = Radio::new(Registers::default(), 1).unwrap();
    assert_eq!(
        radio.wait_interrupt(0, 1),
        Err(RadioError::InvalidParameters)
    );
    assert_eq!(
        radio.wait_interrupt(1, 0),
        Err(RadioError::InvalidParameters)
    );
    assert!(radio.into_inner().accesses.is_empty());
}

#[test]
fn test_should_validate_entire_configuration_before_access() {
    assert_eq!(
        ConfigurationTable::new(&[]).unwrap_err(),
        InvalidConfiguration::Length
    );
    assert_eq!(
        ConfigurationTable::new(&[0, 0, 0, 0, 1, 0]).unwrap_err(),
        InvalidConfiguration::Length
    );
    assert_eq!(
        ConfigurationTable::new(&[0, 0, 0, 0, 0, 0, 0]).unwrap_err(),
        InvalidConfiguration::Length
    );
    assert_eq!(
        ConfigurationTable::new(&[0; 4097]).unwrap_err(),
        InvalidConfiguration::TooLarge
    );
    assert_eq!(
        ConfigurationTable::new(&[0, 0, 0, 0, 2, 0, 0, 1, 2, 6, 3, 4]).unwrap_err(),
        InvalidConfiguration::RecordType
    );
    let table = ConfigurationTable::new(&[0x23, 0x12, 0, 0, 0, 0]).unwrap();
    assert_eq!(table.header(), &[0x23, 0x12, 0, 0, 0, 0]);
    let mut radio = Radio::new(Registers::default(), 1).unwrap();
    assert_eq!(radio.apply_configuration(&table), Ok(()));
    assert!(radio.into_inner().accesses.is_empty());
}
#[test]
fn test_should_preserve_configuration_order_repeated_writes_and_rf_bank_effect() {
    let bytes = [
        0, 0, 0, 0, 8, 0, 3, 0x7f, 2, 3, 0x10, 0x12, 0, 0x11, 0x33, 0, 0x11, 0x44, 1, 2, 3, 2, 3,
        4, 4, 5, 6, 5, 7, 8,
    ];
    let table = ConfigurationTable::new(&bytes).unwrap();
    let mut radio = Radio::new(
        fixture(&[
            (misc(0x1a), 0),
            (misc(0x1a), 1),
            (misc(0x1a), 0),
            (misc(0x1a), 1),
        ]),
        1,
    )
    .unwrap();
    assert_eq!(radio.apply_configuration(&table), Ok(()));
    assert_eq!(
        radio.into_inner().accesses,
        [
            Access::Read(misc(0x1a)),
            Access::Write(misc(0x1a), 1),
            Access::Write(misc(0x18), 0x7f),
            Access::Write(misc(0x19), 2),
            Access::Read(misc(0x1a)),
            Access::Read(misc(0x1a)),
            Access::Write(misc(0x1a), 1),
            Access::Write(misc(0x18), 0x10),
            Access::Write(misc(0x19), 0x12),
            Access::Read(misc(0x1a)),
            Access::Write(ll(0x11), 0x33),
            Access::Write(ll(0x11), 0x44),
            Access::Write(Register::new(Bank::Modem, 2), 3),
            Access::Write(misc(3), 4),
            Access::Write(Register::new(Bank::PowerOn, 5), 6),
            Access::Write(Register::new(Bank::Interface, 7), 8)
        ]
    );
}
#[test]
fn test_should_stop_configuration_at_rf_timeout_without_later_write() {
    let bytes = [0, 0, 0, 0, 2, 0, 3, 0x7f, 1, 0, 0x10, 0xff];
    let table = ConfigurationTable::new(&bytes).unwrap();
    let mut radio = Radio::new(fixture(&[(misc(0x1a), 0), (misc(0x1a), 0)]), 1).unwrap();
    assert_eq!(
        radio.apply_configuration(&table),
        Err(RadioError::RfTimeout)
    );
    assert_eq!(
        radio.into_inner().accesses,
        [
            Access::Read(misc(0x1a)),
            Access::Write(misc(0x1a), 1),
            Access::Write(misc(0x18), 0x7f),
            Access::Write(misc(0x19), 1),
            Access::Read(misc(0x1a))
        ]
    );
}
