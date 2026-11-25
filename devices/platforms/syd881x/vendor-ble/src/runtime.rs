//! Safe vendor-independent state machine around a version-specific backend.
#![forbid(unsafe_code)]

use crate::{decode_event, word};
use device_ble::{Advertisement, Channel, Event, EventQueue, Peripheral};

/// Conservative TX capacity: default ATT MTU 23 minus the notification header.
/// The pinned library can negotiate larger MTUs, but exposes no public MTU event.
pub const TX_CAPACITY: usize = 20;

/// Runtime failures. Vendor rejection does not imply that retrying is harmless.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Zero/aliased profile identifiers.
    InvalidProfile,
    /// SDK advertising mask must select at least one of its three channels.
    InvalidAdvertisingChannels,
    /// Stack initialization has not completed.
    NotInitialized,
    /// No active connection.
    Disconnected,
    /// Peer has not enabled notifications on the matching CCCD.
    NotSubscribed,
    /// Public channel does not match the configured profile.
    UnknownChannel,
    /// Above the conservative negotiated-safe TX bound.
    PayloadTooLarge,
    /// Vendor call rejected; no automatic retry is performed.
    VendorRejected,
    /// Callback event is malformed for the pinned ABI.
    InvalidEvent,
    /// Callback FIFO overflow; latched until reset and TX permission revoked.
    EventOverflow,
    /// Connection changed between admission and the final backend check.
    StaleConnection,
    /// Calls requiring foreground/unmasked IRQs were made in another context.
    WrongContext,
    /// A different advertisement would overwrite ROM-retained storage.
    AdvertisementLocked,
    /// The singleton was already claimed or initialization cannot be repeated.
    AlreadyTaken,
}

/// Selects the conservative MTU response for a copied SDK ATT event.
///
/// The SDK uses a two-byte code/size prefix. Only an exact two-byte MTU request
/// is handled here; unrelated ATT events are left to their existing owner.
/// # Errors
/// Returns `InvalidEvent` for a missing prefix, malformed request or MTU below23.
pub fn mtu_response_for_att_event(event: &[u8]) -> Result<Option<u16>, Error> {
    let prefix = event.get(..2).ok_or(Error::InvalidEvent)?;
    if prefix[0] != 2 {
        return Ok(None);
    }
    if prefix[1] != 2 || event.len() != 4 {
        return Err(Error::InvalidEvent);
    }
    if u16::from_le_bytes([event[2], event[3]]) < 23 {
        return Err(Error::InvalidEvent);
    }
    Ok(Some(23))
}

/// Chip-specific legacy advertising channel mask; portable BLE APIs stay unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdvertisingChannels(u8);
impl AdvertisingChannels {
    /// All three legacy advertising channels.
    pub const ALL: Self = Self(7);
    /// Validates a nonempty three-bit SDK channel mask.
    /// # Errors
    /// Returns `InvalidAdvertisingChannels` for zero or reserved bits.
    pub const fn new(mask: u8) -> Result<Self, Error> {
        if mask == 0 || mask > 7 {
            Err(Error::InvalidAdvertisingChannels)
        } else {
            Ok(Self(mask))
        }
    }
    /// Raw SDK channel mask, for the chip adapter only.
    #[must_use]
    pub const fn mask(self) -> u8 {
        self.0
    }
}

/// Characteristic UUID and value handle, taken from the actual boot/GATT profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Attribute {
    uuid: u16,
    handle: u16,
}
impl Attribute {
    /// Pairs a UUID and handle. Profile construction validates nonzero/uniqueness.
    #[must_use]
    pub const fn new(uuid: u16, handle: u16) -> Self {
        Self { uuid, handle }
    }
}

/// One generic duplex channel mapped to the actual vendor GATT profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Profile {
    pub(crate) service: u16,
    pub(crate) rx: Attribute,
    pub(crate) tx: Attribute,
    pub(crate) cccd: u16,
    channel: Channel,
}
impl Profile {
    /// Validates the board-supplied GATT mapping. This does not install a profile.
    /// # Errors
    /// Rejects zero identifiers and overlapping RX/TX/CCCD handles.
    pub const fn new(
        service: u16,
        rx: Attribute,
        tx: Attribute,
        cccd: u16,
        channel: Channel,
    ) -> Result<Self, Error> {
        if service == 0
            || rx.uuid == 0
            || tx.uuid == 0
            || rx.handle == 0
            || tx.handle == 0
            || cccd == 0
            || rx.handle == tx.handle
            || rx.handle == cccd
            || tx.handle == cccd
        {
            return Err(Error::InvalidProfile);
        }
        Ok(Self {
            service,
            rx,
            tx,
            cccd,
            channel,
        })
    }
    /// TX attribute handle (for the raw SDK descriptor only).
    #[must_use]
    pub const fn tx_handle(self) -> u16 {
        self.tx.handle
    }
    /// TX characteristic UUID (for the raw SDK descriptor only).
    #[must_use]
    pub const fn tx_uuid(self) -> u16 {
        self.tx.uuid
    }
}

/// Current callback-side authorization, updated before events are queued.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LinkState {
    connected: bool,
    subscribed: bool,
    epoch: u32,
}
impl LinkState {
    /// Generation changes on both connect and disconnect, including queue overflow.
    #[must_use]
    pub const fn epoch(self) -> u32 {
        self.epoch
    }
    /// Whether connection and matching notification subscription are active.
    #[must_use]
    pub const fn can_notify(self) -> bool {
        self.connected && self.subscribed
    }
}

/// Owned callback inbox. Synchronization is supplied by the ARM backend.
#[derive(Debug)]
pub struct Inbox<const N: usize> {
    profile: Profile,
    events: EventQueue<N>,
    state: LinkState,
    fault: Option<Error>,
    advertising_ended: bool,
}
impl<const N: usize> Inbox<N> {
    /// Initializes a disconnected inbox with no notification permission.
    #[must_use]
    pub const fn new(profile: Profile) -> Self {
        Self {
            profile,
            events: EventQueue::new(),
            state: LinkState {
                connected: false,
                subscribed: false,
                epoch: 0,
            },
            fault: None,
            advertising_ended: false,
        }
    }
    /// Returns the immediate callback-side state, not a delayed foreground view.
    /// # Errors
    /// Reports a latched malformed-event or overflow fault.
    pub fn state(&self) -> Result<LinkState, Error> {
        self.fault.map_or(Ok(self.state), Err)
    }
    /// Revokes TX and permanently records the first ingress failure until reset.
    pub fn fail(&mut self, error: Error) {
        self.fault = self.fault.or(Some(error));
        self.advertising_ended = false;
        self.state.connected = false;
        self.state.subscribed = false;
        self.state.epoch = self.state.epoch.wrapping_add(1);
    }
    fn accept(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let event = decode_event(bytes).map_err(|_| Error::InvalidEvent)?;
        let code = bytes.get(1..5).ok_or(Error::InvalidEvent)?;
        if bytes.first() == Some(&0) && code == [1, 0, 0, 0] {
            self.advertising_ended = !self.state.connected;
        }
        if bytes.first() == Some(&0) && code == [0, 0x20, 0, 0] {
            let service = word(bytes, 5).map_err(|_| Error::InvalidEvent)?;
            let handle = word(bytes, 7).map_err(|_| Error::InvalidEvent)?;
            let value = word(bytes, 9).map_err(|_| Error::InvalidEvent)?;
            if self.state.connected
                && service == self.profile.service
                && handle == self.profile.cccd
            {
                self.state.subscribed = value & 1 != 0;
            }
        }
        let event = match event {
            Some(Event::Connected) => {
                self.advertising_ended = false;
                self.state = LinkState {
                    connected: true,
                    subscribed: false,
                    epoch: self.state.epoch.wrapping_add(1),
                };
                Event::Connected
            }
            Some(Event::Disconnected { reason }) => {
                self.advertising_ended = false;
                self.state = LinkState {
                    connected: false,
                    subscribed: false,
                    epoch: self.state.epoch.wrapping_add(1),
                };
                Event::Disconnected { reason }
            }
            Some(Event::Received { channel, packet }) => {
                if !self.state.connected
                    || channel.0 != self.profile.rx.handle
                    || word(bytes, 5).map_err(|_| Error::InvalidEvent)? != self.profile.service
                    || word(bytes, 7).map_err(|_| Error::InvalidEvent)? != self.profile.rx.uuid
                {
                    return Ok(());
                }
                Event::Received {
                    channel: self.profile.channel,
                    packet,
                }
            }
            None => return Ok(()),
        };
        self.events.push(event).map_err(|_| Error::EventOverflow)
    }
    /// Copies a verified-size callback slice, validates ABI and maps its channel.
    /// # Errors
    /// Latches malformed input/overflow and immediately revokes TX permission.
    pub fn ingest(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.state()?;
        let result = self.accept(bytes);
        if let Err(error) = result {
            self.fail(error);
        }
        result
    }
    /// Consumes a positively observed SDK advertising-end event, while disconnected.
    /// # Errors
    /// A latched callback failure forbids any automatic restart.
    pub fn take_advertising_end(&mut self) -> Result<bool, Error> {
        self.state()?;
        Ok(core::mem::take(&mut self.advertising_ended) && !self.state.connected)
    }
    /// Takes an owned portable event even after a fault, allowing diagnostics.
    pub fn pop(&mut self) -> Option<Event> {
        self.events.pop()
    }
}

/// Backend contract. The real ARM implementation owns all vendor FFI interaction.
///
/// Implementations must copy callback data, serialize access and make `send`
/// synchronously consume its input. SDK acceptance is not peer delivery.
pub trait Backend {
    /// Initializes once, including callback registration before enabling operation.
    /// # Errors
    /// Reports initialization failure; resources must remain alive on failure.
    fn initialize(&mut self, profile: Profile) -> Result<(), Error>;
    /// Configures or restarts advertising using backend-owned stable storage.
    /// # Errors
    /// Rejects incompatible reconfiguration or vendor/context failures.
    fn advertise(&mut self, advertisement: &Advertisement<'_>) -> Result<(), Error>;
    /// Reads current callback-side state and latched faults.
    /// # Errors
    /// Reports ingress/overflow failure.
    fn state(&self) -> Result<LinkState, Error>;
    /// Checks epoch/subscription immediately before synchronous-copy transmission.
    /// # Errors
    /// Rejects stale connections, context failures or vendor rejection.
    fn send(&mut self, profile: Profile, epoch: u32, data: &[u8]) -> Result<(), Error>;
    /// Invokes the vendor scheduler once, never while holding an inbox borrow.
    /// # Errors
    /// Reports context/ingress failures. Vendor worst-case timing needs hardware checks.
    fn service(&mut self) -> Result<(), Error>;
    /// Removes one copied event.
    fn next_event(&mut self) -> Option<Event>;
}

/// Safe, portable BLE peripheral state machine over an exclusively owned backend.
#[derive(Debug)]
pub struct Runtime<B> {
    backend: B,
    profile: Profile,
}
impl<B: Backend> Runtime<B> {
    /// Initializes the backend and takes exclusive ownership of it.
    /// # Errors
    /// Reports backend failure without releasing any ROM-retained storage.
    pub fn new(mut backend: B, profile: Profile) -> Result<Self, Error> {
        backend.initialize(profile)?;
        Ok(Self { backend, profile })
    }
}
impl<B: Backend> Peripheral for Runtime<B> {
    type Error = Error;
    fn advertise(&mut self, advertisement: &Advertisement<'_>) -> Result<(), Error> {
        self.backend.state()?;
        self.backend.advertise(advertisement)
    }
    fn notify(&mut self, channel: Channel, data: &[u8]) -> Result<(), Error> {
        let state = self.backend.state()?;
        if !state.connected {
            return Err(Error::Disconnected);
        }
        if !state.subscribed {
            return Err(Error::NotSubscribed);
        }
        if channel != self.profile.channel {
            return Err(Error::UnknownChannel);
        }
        if data.len() > TX_CAPACITY {
            return Err(Error::PayloadTooLarge);
        }
        self.backend.send(self.profile, state.epoch, data)
    }
    fn service(&mut self) -> Result<(), Error> {
        self.backend.service()?;
        self.backend.state().map(|_| ())
    }
    fn next_event(&mut self) -> Option<Event> {
        self.backend.next_event()
    }
}

/// Remembers immutable advertising identity after the ROM may retain its pointers.
#[derive(Debug, Default)]
pub struct AdvertisementLock {
    key: Option<AdvertisementKey>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct AdvertisementKey {
    data: [u8; 31],
    scan: [u8; 31],
    length: u8,
    scan_length: u8,
    interval: u16,
}
impl AdvertisementLock {
    /// Whether immutable advertising storage has been handed off or reserved.
    #[must_use]
    pub const fn is_bound(&self) -> bool {
        self.key.is_some()
    }

    /// Creates an unbound lock.
    #[must_use]
    pub const fn new() -> Self {
        Self { key: None }
    }
    /// Binds once; true means first submission, false means restart the same data.
    /// # Errors
    /// Rejects changed bytes/interval forever, including after stop/failure/drop.
    pub fn bind(&mut self, advertisement: &Advertisement<'_>) -> Result<bool, Error> {
        let mut key = AdvertisementKey {
            data: [0; 31],
            scan: [0; 31],
            length: 0,
            scan_length: 0,
            interval: advertisement.interval(),
        };
        key.data[..advertisement.data().len()].copy_from_slice(advertisement.data());
        key.scan[..advertisement.scan_response().len()]
            .copy_from_slice(advertisement.scan_response());
        key.length =
            u8::try_from(advertisement.data().len()).map_err(|_| Error::PayloadTooLarge)?;
        key.scan_length = u8::try_from(advertisement.scan_response().len())
            .map_err(|_| Error::PayloadTooLarge)?;
        match self.key {
            None => {
                self.key = Some(key);
                Ok(true)
            }
            Some(previous) if previous == key => Ok(false),
            Some(_) => Err(Error::AdvertisementLocked),
        }
    }
}
