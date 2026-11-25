//! Observe bounded RX FIFO ownership, metadata and connectable PDU encoding.
#![allow(clippy::unwrap_used, reason = "Unexpected test setup aborts the test")]
use std::collections::VecDeque;
use syd881x_radio::{
    AdvertisingPdu, Bank, ByteRegisters, DataPdu, Radio, RadioError, Register, StaticRandomAddress,
};

#[derive(Debug)]
struct Fifo {
    bytes: VecDeque<u8>,
    empty: bool,
    reads: usize,
    fail_at: Option<usize>,
    releases: usize,
}
impl Fifo {
    fn new(bytes: &[u8]) -> Self {
        Self {
            bytes: bytes.iter().copied().collect(),
            empty: false,
            reads: 0,
            fail_at: None,
            releases: 0,
        }
    }
}
impl ByteRegisters for Fifo {
    type Error = ();
    fn read8(&mut self, register: Register) -> Result<u8, ()> {
        assert_eq!(register.bank(), Bank::Ll);
        if register.offset() == 3 {
            return Ok(u8::from(self.empty) << 3);
        }
        assert_eq!(register.offset(), 0xc4);
        self.reads += 1;
        if self.fail_at == Some(self.reads) {
            return Err(());
        }
        self.bytes.pop_front().ok_or(())
    }
    fn write8(&mut self, register: Register, value: u8) -> Result<(), ()> {
        assert_eq!(
            (register.bank(), register.offset(), value),
            (Bank::Ll, 0xc9, 1)
        );
        self.releases += 1;
        Ok(())
    }
}

#[test]
fn test_should_not_pop_or_release_an_empty_receive_fifo() {
    let mut io = Fifo::new(&[]);
    io.empty = true;
    let mut radio = Radio::new(io, 2).unwrap();
    assert_eq!(radio.receive().unwrap(), None);
    let io = radio.into_inner();
    assert_eq!((io.reads, io.releases), (0, 0));
}

#[test]
fn test_should_receive_owned_advertising_payload_and_release_exactly_once() {
    let mut bytes = vec![0x85, 38];
    bytes.extend(0..38);
    let mut radio = Radio::new(Fifo::new(&bytes), 2).unwrap();
    let packet = radio.receive().unwrap().unwrap();
    assert_eq!(packet.fifo_header(), [0x85, 38]);
    assert_eq!(packet.payload(), &(0..38).collect::<Vec<_>>());
    assert!(!packet.is_data_channel());
    assert_eq!(packet.mic_status_bit(), None);
    assert_eq!(
        format!("{packet:?}"),
        "ReceivedPacket { payload_len: 38, data_channel: false, .. }"
    );
    let io = radio.into_inner();
    assert_eq!((io.reads, io.releases), (40, 1));
}

#[test]
fn test_should_separate_data_channel_and_mic_metadata_from_payload_length() {
    let mut radio = Radio::new(Fifo::new(&[2, 0xc3, 9, 8, 7]), 2).unwrap();
    let packet = radio.receive().unwrap().unwrap();
    assert_eq!(packet.payload(), &[9, 8, 7]);
    assert!(packet.is_data_channel());
    assert_eq!(packet.mic_status_bit(), Some(true));
    assert_eq!(packet.fifo_header(), [2, 0xc3]);
    assert_eq!(radio.into_inner().releases, 1);
}

#[test]
fn test_should_release_rejected_length_without_reading_unbounded_payload() {
    let mut radio = Radio::new(Fifo::new(&[5, 39]), 2).unwrap();
    assert_eq!(
        radio.receive(),
        Err(RadioError::InvalidReceiveLength { length: 39 })
    );
    let io = radio.into_inner();
    assert_eq!((io.reads, io.releases), (2, 1));
}

#[test]
fn test_should_report_partial_fifo_transport_failure_for_reset_recovery() {
    let mut io = Fifo::new(&[5, 3, 1, 2, 3]);
    io.fail_at = Some(4);
    let mut radio = Radio::new(io, 2).unwrap();
    assert_eq!(radio.receive(), Err(RadioError::Access(())));
    let io = radio.into_inner();
    assert_eq!((io.reads, io.releases), (4, 0));
}

#[test]
fn test_should_encode_connectable_advertising_without_changing_nonconnectable_pdu() {
    let identity = StaticRandomAddress::new([0x43, 0x52, 0x53, 0x54, 0, 0xc0]).unwrap();
    let connectable = AdvertisingPdu::connectable(identity, &[2, 1, 6]).unwrap();
    let old = AdvertisingPdu::nonconnectable(identity, &[2, 1, 6]).unwrap();
    assert_eq!(connectable.as_slice()[0], 0x40);
    assert_eq!(old.as_slice()[0], 0x42);
    assert_eq!(&connectable.as_slice()[1..], &old.as_slice()[1..]);
    assert!(AdvertisingPdu::connectable(identity, &[0; 32]).is_err());
}

#[derive(Debug, Default)]
struct Tx {
    writes: Vec<(u8, u8)>,
    full: bool,
}
impl ByteRegisters for Tx {
    type Error = ();
    fn read8(&mut self, register: Register) -> Result<u8, ()> {
        assert_eq!((register.bank(), register.offset()), (Bank::Ll, 3));
        Ok(u8::from(self.full))
    }
    fn write8(&mut self, register: Register, value: u8) -> Result<(), ()> {
        assert_eq!(register.bank(), Bank::Ll);
        self.writes.push((register.offset(), value));
        Ok(())
    }
}

#[test]
fn test_should_preload_address_only_scan_response_through_fifo_one_then_commit() {
    let identity = StaticRandomAddress::new([1, 2, 3, 4, 5, 0xc6]).unwrap();
    let pdu = AdvertisingPdu::scan_response(identity, &[]).unwrap();
    assert_eq!(pdu.as_slice(), &[0x44, 6, 1, 2, 3, 4, 5, 0xc6]);
    let mut radio = Radio::new(Tx::default(), 2).unwrap();
    radio.queue_scan_response(&pdu).unwrap();
    let mut expected: Vec<_> = pdu.as_slice().iter().map(|&b| (0xc1, b)).collect();
    expected.push((0xc8, 1));
    assert_eq!(radio.into_inner().writes, expected);
}

#[test]
fn test_should_reject_wrong_pdu_kind_and_full_scan_response_fifo_without_writes() {
    let identity = StaticRandomAddress::new([1, 2, 3, 4, 5, 0xc6]).unwrap();
    let ad = AdvertisingPdu::connectable(identity, &[]).unwrap();
    let rsp = AdvertisingPdu::scan_response(identity, &[]).unwrap();
    let mut radio = Radio::new(Tx::default(), 2).unwrap();
    assert_eq!(
        radio.queue_scan_response(&ad),
        Err(RadioError::InvalidParameters)
    );
    assert_eq!(
        radio.queue_advertisement(&rsp),
        Err(RadioError::InvalidParameters)
    );
    assert!(radio.into_inner().writes.is_empty());
    let mut radio = Radio::new(
        Tx {
            full: true,
            ..Tx::default()
        },
        2,
    )
    .unwrap();
    assert_eq!(radio.queue_scan_response(&rsp), Err(RadioError::FifoFull));
    assert!(radio.into_inner().writes.is_empty());
}

#[test]
fn test_should_encode_legacy_data_without_copying_rx_metadata_and_commit_once() {
    let pdu = DataPdu::control(&[0x0c, 6, 0xff, 0xff, 1, 0]).unwrap();
    assert_eq!(pdu.as_slice(), &[3, 6, 0x0c, 6, 0xff, 0xff, 1, 0]);
    let mut radio = Radio::new(Tx::default(), 2).unwrap();
    radio.queue_data(&pdu).unwrap();
    let mut expected: Vec<_> = pdu.as_slice().iter().map(|&b| (0xc0, b)).collect();
    expected.push((0xc8, 1));
    assert_eq!(radio.into_inner().writes, expected);
    assert!(DataPdu::control(&[]).is_err());
    assert!(DataPdu::control(&[0; 28]).is_err());
    let att = DataPdu::l2cap(4, &[0x03, 23, 0]).unwrap();
    assert_eq!(att.as_slice(), &[2, 7, 3, 0, 4, 0, 0x03, 23, 0]);
    assert!(DataPdu::l2cap(4, &[0; 24]).is_err());
}
