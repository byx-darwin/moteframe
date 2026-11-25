//! Portable BLE peripheral contracts and owned events, without a vendor runtime.
#![no_std]

use core::fmt;

/// Maximum owned payload stored by this framework (ATT MTU 247 minus 3).
pub const MAX_PAYLOAD: usize = 244;

/// A payload or advertising parameter exceeds the interface limits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidInput;

/// Backend-independent channel identifier. A board maps it to its GATT profile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Channel(pub u16);

/// Owned BLE bytes. Debug output omits the contents.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Packet {
    bytes: [u8; MAX_PAYLOAD],
    len: u8,
}
impl Default for Packet {
    fn default() -> Self {
        Self {
            bytes: [0; MAX_PAYLOAD],
            len: 0,
        }
    }
}
impl fmt::Debug for Packet {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Packet")
            .field("len", &self.len)
            .finish_non_exhaustive()
    }
}
impl Packet {
    /// Copies data into bounded owned storage; no borrowed vendor memory escapes.
    /// # Errors
    /// Rejects payloads larger than `MAX_PAYLOAD`.
    pub fn try_from_slice(data: &[u8]) -> Result<Self, InvalidInput> {
        if data.len() > MAX_PAYLOAD {
            return Err(InvalidInput);
        }
        let mut packet = Self::default();
        packet.bytes[..data.len()].copy_from_slice(data);
        packet.len = u8::try_from(data.len()).map_err(|_| InvalidInput)?;
        Ok(packet)
    }
    /// Valid bytes, without trailing storage.
    #[must_use]
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes[..usize::from(self.len)]
    }
}

/// Legacy connectable advertising data and interval in 0.625 ms units.
#[derive(Clone, Copy, Debug)]
pub struct Advertisement<'a> {
    data: &'a [u8],
    scan_response: &'a [u8],
    interval: u16,
}
impl<'a> Advertisement<'a> {
    /// Constructs bounded legacy advertising parameters.
    /// # Errors
    /// Rejects either data section above 31 bytes or intervals outside 32..=16384.
    pub fn new(
        data: &'a [u8],
        scan_response: &'a [u8],
        interval: u16,
    ) -> Result<Self, InvalidInput> {
        if data.len() > 31 || scan_response.len() > 31 || !(32..=16384).contains(&interval) {
            return Err(InvalidInput);
        }
        Ok(Self {
            data,
            scan_response,
            interval,
        })
    }
    /// Raw standard advertising structures, with no application protocol added.
    #[must_use]
    pub const fn data(&self) -> &'a [u8] {
        self.data
    }
    /// Scan response structures.
    #[must_use]
    pub const fn scan_response(&self) -> &'a [u8] {
        self.scan_response
    }
    /// Advertising interval in 0.625 ms units.
    #[must_use]
    pub const fn interval(&self) -> u16 {
        self.interval
    }
}

/// Events independent of a particular controller or event-loop implementation.
#[allow(
    clippy::large_enum_variant,
    reason = "Owned fixed-size events avoid heap allocation and borrowed vendor buffers"
)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// A peer connected.
    Connected,
    /// A peer disconnected with the controller's reason code.
    Disconnected {
        /// HCI/stack reason code.
        reason: u8,
    },
    /// Owned bytes received on a mapped channel.
    Received {
        /// Board/profile channel mapping.
        channel: Channel,
        /// Copied packet.
        packet: Packet,
    },
}

/// Queue at capacity; the existing events are preserved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueFull;

/// A fixed-capacity FIFO; it is not an interrupt synchronization primitive.
///
/// The board must serialize callback and foreground access. A full queue returns
/// `QueueFull`; the caller must record/report that failure or apply backpressure.
#[derive(Debug)]
pub struct EventQueue<const N: usize> {
    events: [Option<Event>; N],
    head: usize,
    len: usize,
}
impl<const N: usize> Default for EventQueue<N> {
    fn default() -> Self {
        Self::new()
    }
}
impl<const N: usize> EventQueue<N> {
    /// Creates an empty FIFO. A zero-capacity FIFO rejects all pushes.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            events: [None; N],
            head: 0,
            len: 0,
        }
    }
    /// Queues an owned event.
    /// # Errors
    /// Returns `QueueFull` without overwriting queued events.
    pub fn push(&mut self, event: Event) -> Result<(), QueueFull> {
        if self.len == N {
            return Err(QueueFull);
        }
        self.events[(self.head + self.len) % N] = Some(event);
        self.len += 1;
        Ok(())
    }
    /// Removes the oldest event, if any.
    pub fn pop(&mut self) -> Option<Event> {
        if self.len == 0 {
            return None;
        }
        let event = self.events[self.head].take();
        self.head = (self.head + 1) % N;
        self.len -= 1;
        event
    }
}

/// Hardware-independent BLE peripheral interface, implemented by board adapters.
///
/// The API is synchronous and requires one serialized owner. Implementations
/// must copy TX bytes before returning, or report busy; never retain the slice.
/// Notifications require connection, negotiated MTU and enabled peer subscription.
pub trait Peripheral {
    /// Explicit initialization, busy, capacity, subscription or transport error.
    type Error;
    /// Starts legacy connectable advertising after board/stack initialization.
    /// # Errors
    /// Reports initialization, parameter or stack failure.
    fn advertise(&mut self, advertisement: &Advertisement<'_>) -> Result<(), Self::Error>;
    /// Sends one notification on the board-mapped channel.
    /// # Errors
    /// Reports disconnected, unsubscribed, busy, MTU or stack errors.
    fn notify(&mut self, channel: Channel, data: &[u8]) -> Result<(), Self::Error>;
    /// Pumps a bounded amount of controller work. Call frequently between display chunks.
    /// # Errors
    /// Reports stack failures and callback queue overflow.
    fn service(&mut self) -> Result<(), Self::Error>;
    /// Takes an owned event; no vendor pointer survives the callback.
    fn next_event(&mut self) -> Option<Event>;
}
