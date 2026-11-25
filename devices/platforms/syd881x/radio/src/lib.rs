//! Chip-specific, bounded radio algorithms; no vendor or ROM calls.
#![no_std]
#![forbid(unsafe_code)]

mod advertise;
mod config;
mod connection;
mod data;
mod initialize;
mod radio;
mod receive;
mod register;
mod update;
pub use advertise::{AdvertisingParameters, Status};
pub use config::{ConfigurationTable, InvalidConfiguration};
pub use connection::{ConnectionRequest, ConnectionWindows, InvalidConnection};
pub use data::{DataPdu, InvalidDataPdu};
pub use initialize::{
    InitializationError, InitializationFailure, InitializationReport, InitializationStage,
};
pub use radio::{Radio, RadioError};
pub use receive::ReceivedPacket;
pub use register::{Bank, ByteRegisters, Register};
pub use update::{ConnectionEvents, ConnectionUpdate, PendingConnectionUpdate, UpdateProgress};

/// Invalid advertising address or data length.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidAdvertisement {
    /// The random address does not have a valid static random bit pattern.
    Address,
    /// Advertising data is longer than 31 bytes.
    DataLength,
}

/// A validated little-endian static random advertiser address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StaticRandomAddress([u8; 6]);
impl StaticRandomAddress {
    /// Validates static random bits and the nonconstant lower 46 bits.
    /// # Errors
    /// Returns `Address` if the address cannot be used as static random.
    pub fn new(bytes: [u8; 6]) -> Result<Self, InvalidAdvertisement> {
        let random_high = bytes[5] & 0x3f;
        let all_zero = bytes[..5].iter().all(|&byte| byte == 0) && random_high == 0;
        let all_one = bytes[..5].iter().all(|&byte| byte == 0xff) && random_high == 0x3f;
        if bytes[5] & 0xc0 != 0xc0 || all_zero || all_one {
            return Err(InvalidAdvertisement::Address);
        }
        Ok(Self(bytes))
    }
}

/// Owned complete legacy advertising PDU, without CRC or access address.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AdvertisingPdu {
    bytes: [u8; 39],
    len: u8,
}
impl core::fmt::Debug for AdvertisingPdu {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter
            .debug_struct("AdvertisingPdu")
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}
impl AdvertisingPdu {
    /// Encodes `SCAN_RSP`, including `AdvA` even when scan data is empty.
    /// # Errors
    /// Returns `DataLength` if `data` exceeds 31 bytes.
    pub fn scan_response(
        address: StaticRandomAddress,
        data: &[u8],
    ) -> Result<Self, InvalidAdvertisement> {
        let mut pdu = Self::nonconnectable(address, data)?;
        pdu.bytes[0] = 0x44;
        Ok(pdu)
    }

    /// Encodes a legacy `ADV_IND` with a static random address.
    ///
    /// This changes only the transmitted PDU; a working connection additionally
    /// requires RX, parameter validation and hardware connection control.
    /// # Errors
    /// Returns `DataLength` if `data` exceeds 31 bytes.
    pub fn connectable(
        address: StaticRandomAddress,
        data: &[u8],
    ) -> Result<Self, InvalidAdvertisement> {
        let mut pdu = Self::nonconnectable(address, data)?;
        pdu.bytes[0] = 0x40;
        Ok(pdu)
    }

    /// Encodes two header bytes, the six-byte address and at most 31 AD bytes.
    /// # Errors
    /// Returns `DataLength` if `data` exceeds 31 bytes.
    pub fn nonconnectable(
        address: StaticRandomAddress,
        data: &[u8],
    ) -> Result<Self, InvalidAdvertisement> {
        if data.len() > 31 {
            return Err(InvalidAdvertisement::DataLength);
        }
        let payload_len =
            u8::try_from(data.len() + 6).map_err(|_| InvalidAdvertisement::DataLength)?;
        let mut pdu = Self {
            bytes: [0; 39],
            len: payload_len + 2,
        };
        pdu.bytes[0] = 0x42;
        pdu.bytes[1] = payload_len;
        pdu.bytes[2..8].copy_from_slice(&address.0);
        pdu.bytes[8..usize::from(pdu.len)].copy_from_slice(data);
        Ok(pdu)
    }
    /// Complete FIFO bytes; header payload length excludes the two header bytes.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }
}

#[cfg(test)]
extern crate std;
#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    reason = "Tests abort on unexpected setup or operation failure"
)]
mod tests {
    use super::*;
    #[test]
    fn test_should_encode_complete_nonconnectable_pdu() {
        let address = StaticRandomAddress::new([1, 2, 3, 4, 5, 0xc6]).unwrap();
        let pdu = AdvertisingPdu::nonconnectable(address, &[2, 1, 6]).unwrap();
        assert_eq!(pdu.as_slice(), &[0x42, 9, 1, 2, 3, 4, 5, 0xc6, 2, 1, 6]);
        assert_eq!(std::format!("{pdu:?}"), "AdvertisingPdu { len: 11, .. }");
    }
    #[test]
    fn test_should_bound_ad_data_and_include_headers_in_fifo_length() {
        let address = StaticRandomAddress::new([1, 0, 0, 0, 0, 0xc0]).unwrap();
        let empty = AdvertisingPdu::nonconnectable(address, &[]).unwrap();
        assert_eq!(empty.as_slice(), &[0x42, 6, 1, 0, 0, 0, 0, 0xc0]);
        let full = AdvertisingPdu::nonconnectable(address, &[0xaa; 31]).unwrap();
        assert_eq!(full.as_slice().len(), 39);
        assert_eq!(full.as_slice()[1], 37);
        assert_eq!(&full.as_slice()[8..], &[0xaa; 31]);
        assert_eq!(
            AdvertisingPdu::nonconnectable(address, &[0; 32]),
            Err(InvalidAdvertisement::DataLength)
        );
    }
    #[test]
    fn test_should_accept_valid_static_random_address() {
        assert!(StaticRandomAddress::new([1, 2, 3, 4, 5, 0xc6]).is_ok());
    }
    #[test]
    fn test_should_reject_nonstatic_and_constant_random_parts() {
        for address in [[1, 2, 3, 4, 5, 0x86], [0, 0, 0, 0, 0, 0xc0], [0xff; 6]] {
            assert_eq!(
                StaticRandomAddress::new(address),
                Err(InvalidAdvertisement::Address)
            );
        }
    }
}
