//! Contract for a backend that owns radio timing and SN/NESN acknowledgement.
use crate::{
    AdvertisingData, AdvertisingParameters, ConnectionParameters, LinkFrame, RandomAddress,
};

/// Peer identity in Bluetooth byte order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Peer {
    address: [u8; 6],
    random: bool,
}
impl Peer {
    /// Captures the address and its public/static-random type.
    #[must_use]
    pub const fn new(address: [u8; 6], random: bool) -> Self {
        Self { address, random }
    }
    /// Address bytes.
    #[must_use]
    pub const fn address(self) -> [u8; 6] {
        self.address
    }
    /// Whether the address type is random.
    #[must_use]
    pub const fn is_random(self) -> bool {
        self.random
    }
}
/// Complete owned configuration for one connectable legacy advertiser.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Advertisement {
    /// Advertiser identity.
    pub address: RandomAddress,
    /// Interval and primary channels.
    pub parameters: AdvertisingParameters,
    /// Advertising data.
    pub data: AdvertisingData,
    /// Scan response data.
    pub scan_response: AdvertisingData,
}
impl Advertisement {
    /// Combines previously validated standard parameters.
    #[must_use]
    pub const fn new(
        address: RandomAddress,
        parameters: AdvertisingParameters,
        data: AdvertisingData,
        scan_response: AdvertisingData,
    ) -> Self {
        Self {
            address,
            parameters,
            data,
            scan_response,
        }
    }
}
/// Hardware FIFO admission, distinct from acknowledgement over the air.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TxAdmission {
    /// No bytes were committed; ownership remains with the Controller.
    Busy,
    /// Backend owns the packet until exactly one matching ACK or disconnect.
    Committed,
}
/// Ordered backend observations with owned payloads.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LinkEvent {
    /// Connection configuration has actually been accepted by hardware.
    Connected(Peer, ConnectionParameters),
    /// Newly received data; duplicates must have been removed by the backend.
    Received(LinkFrame),
    /// Previously committed transmit token acknowledged by the peer.
    Acknowledged(u32),
    /// Hardware connection has stopped, with an HCI reason code.
    Disconnected(u8),
    /// An incoming update actually reached its instant and took effect.
    ParametersUpdated(ConnectionParameters),
}
/// Radio backend with hardware-assisted timing, retransmission and SN/NESN.
///
/// A radio-only chip needs an additional software link engine implementing this
/// contract. Methods are serialized by the caller. No temporary FIFO pointers
/// escape. Failure after partial hardware access must return an error, not Busy.
/// The backend must discard old IRQ/status observations on reset and disconnect.
/// IRQ integrations must also wake the HCI reader after publishing observations.
pub trait LinkBackend: core::fmt::Debug {
    /// Hardware or access error.
    type Error: core::fmt::Debug;
    /// Resets hardware and discards pending status and tokens.
    /// # Errors
    /// Returns any hardware failure; successful reset is required after a fault.
    fn reset(&mut self) -> Result<(), Self::Error>;
    /// Starts advertising with all parameters applied atomically or faults.
    /// # Errors
    /// Returns invalid/unsupported hardware configuration or access failures.
    fn advertise(&mut self, advertisement: &Advertisement) -> Result<(), Self::Error>;
    /// Stops radio activity and discards prior connection status and tokens.
    /// # Errors
    /// Returns a hardware access failure.
    fn stop(&mut self) -> Result<(), Self::Error>;
    /// Returns one ordered observation without exposing register representations.
    /// # Errors
    /// Returns access, malformed radio data or lost/ambiguous status errors.
    fn poll(&mut self) -> Result<Option<LinkEvent>, Self::Error>;
    /// Tries one FIFO commit. An ACK must carry this token, including across reset.
    /// # Errors
    /// Returns partial-commit or hardware failures; Busy means nothing was written.
    fn transmit(&mut self, token: u32, frame: LinkFrame) -> Result<TxAdmission, Self::Error>;
    /// Validates and schedules the complete LL connection update indication.
    /// # Errors
    /// Returns unsupported timing, invalid instant or access failures.
    fn connection_update(&mut self, indication: &[u8]) -> Result<(), Self::Error>;
    /// Validates and schedules the complete LL channel map indication.
    /// # Errors
    /// Returns invalid map/instant or access failures.
    fn channel_map(&mut self, indication: &[u8]) -> Result<(), Self::Error>;
}
