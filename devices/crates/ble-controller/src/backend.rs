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
#[allow(
    clippy::large_enum_variant,
    reason = "Transfers ownership directly to the polling caller without an allocator"
)]
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
    /// Effective TX and RX PHYs, in peripheral order and HCI encoding.
    PhyUpdated(u8, u8),
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
    /// Preferred maximum outgoing ACL payload per radio packet.
    ///
    /// The Controller splits longer packets into link-layer fragments, with
    /// a minimum limit of four bytes for the first L2CAP header. This does
    /// not change the Host ACL buffer size or the receive limit.
    fn max_acl_fragment_len(&self) -> u8 {
        27
    }
    /// Whether the backend implements 251-octet TX, RX and negotiated airtime.
    /// The Controller enables DLE when this is true and
    /// [`data_length_clock`](Self::data_length_clock) supplies a clock.
    fn supports_data_length(&self) -> bool {
        false
    }
    /// Latest native Bluetooth clock sample (28-bit, one tick per 312.5 µs).
    /// DLE backends return a clock even while idle and refresh it during radio
    /// service. Without a clock the Controller cannot time local LL procedures.
    fn data_length_clock(&self) -> Option<u32> {
        None
    }
    /// Apply the effective limits in peripheral order to future radio PDUs.
    fn data_length_updated(&mut self, _limits: crate::DataLength) {}
    /// Optional peripheral PHY procedure support; legacy backends remain 1M-only.
    fn supports_two_m(&self) -> bool {
        false
    }
    /// Record the remote LE 2M capability.
    fn peer_two_m(&mut self, _supported: bool) {}
    /// Preferred TX and RX mask in a response to the central.
    fn phy_response_mask(&mut self) -> u8 {
        1
    }
    /// Request one feature exchange if this central has not supplied its capabilities.
    fn take_feature_query(&mut self) -> bool {
        false
    }
    /// Poll the next local PHY preference when control queue space is available.
    fn take_phy_request(&mut self) -> Option<u8> {
        None
    }
    /// Record the rejection of a pending local PHY procedure.
    fn phy_rejected(&mut self) {}
    /// Stage the indicated PHYs for the specified connection event.
    fn schedule_phy(&mut self, _instant: u16, _central_tx: u8, _peripheral_tx: u8) -> bool {
        false
    }
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
    /// Advances radio timing without handing an observation to a saturated Host.
    /// Queued observations remain backend-owned until the controller polls again.
    /// Interrupt-driven backends with autonomous scheduling need no additional work.
    /// # Errors
    /// Returns a hardware or scheduling failure.
    #[cfg(feature = "data-length")]
    fn service_backpressure(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
    /// Fills caller-owned storage, allowing packet RAM to be copied without a second frame.
    /// # Errors
    /// Returns the same hardware and protocol errors as [`Self::poll`].
    #[cfg(feature = "data-length")]
    fn poll_into(&mut self, out: &mut LinkEvent) -> Result<bool, Self::Error> {
        if let Some(event) = self.poll()? {
            *out = event;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    /// Tries one FIFO commit. An ACK must carry this token, including across reset.
    /// # Errors
    /// Returns partial-commit or hardware failures; Busy means nothing was written.
    fn transmit(&mut self, token: u32, frame: LinkFrame) -> Result<TxAdmission, Self::Error>;
    /// Commits validated borrowed bytes. On success the backend owns a copy until ACK.
    /// # Errors
    /// Returns the same failures as [`Self::transmit`].
    #[cfg(feature = "data-length")]
    fn transmit_payload(
        &mut self,
        token: u32,
        kind: crate::FrameKind,
        more_data: bool,
        payload: &[u8],
    ) -> Result<TxAdmission, Self::Error> {
        let Ok(frame) = LinkFrame::new(kind, payload) else {
            return Ok(TxAdmission::Busy);
        };
        self.transmit(token, frame.with_more_data(more_data))
    }
    /// Validates and schedules the complete LL connection update indication.
    /// # Errors
    /// Returns unsupported timing, invalid instant or access failures.
    fn connection_update(&mut self, indication: &[u8]) -> Result<(), Self::Error>;
    /// Validates and schedules the complete LL channel map indication.
    /// # Errors
    /// Returns invalid map/instant or access failures.
    fn channel_map(&mut self, indication: &[u8]) -> Result<(), Self::Error>;
}
