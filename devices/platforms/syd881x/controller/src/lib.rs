//! SYD radio adaptation for the portable Controller contract.
#![no_std]
#![forbid(unsafe_code)]

use ble_controller::{
    Advertisement, ConnectionParameters, FrameKind, LinkBackend, LinkEvent, LinkFrame, OwnedQueue,
    Peer, TxAdmission,
};
use core::num::NonZeroU16;
use syd881x_radio::{
    AdvertisingPdu, ByteRegisters, ConfigurationTable, ConnectionRequest, ConnectionUpdate,
    ConnectionWindows, DataPdu, InitializationError, PendingConnectionUpdate, Radio, RadioError,
    StaticRandomAddress, UpdateProgress,
};

/// Chip adapter failure; no uncertainty is converted to a successful HCI response.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SydError<E> {
    /// Radio register/bridge/FIFO access failed.
    Radio(RadioError<E>),
    /// Reset or RF calibration failed.
    Initialization(InitializationError<E>),
    /// Unsupported or inconsistent chip profile/procedure.
    Protocol,
    /// A rejected operation with a stable diagnostic location (1..24).
    /// The original generic variant remains available for existing callers.
    ProtocolAt {
        /// Rejection location, translated by the Mac diagnostic decoder.
        site: u8,
    },
    /// The bounded observation storage was exhausted.
    Capacity,
}
impl<E> From<RadioError<E>> for SydError<E> {
    fn from(e: RadioError<E>) -> Self {
        Self::Radio(e)
    }
}
/// SYD hardware-assisted backend. Board initialization supplies exclusive byte
/// access, the exact validated board table and a conservative settling delay.
///
/// This adapter busy-polls and initially supports latency zero, LE1M and CSA1.
/// SDK TXINT is documented as transmit-and-ACK; one owned FIFO0 packet is allowed
/// in flight and completion additionally requires `TX_EMPTY`. That combination
/// still requires validation on each silicon revision; it is not a sniffer trace.
#[derive(Debug)]
pub struct SydBackend<'a, R> {
    radio: Radio<R>,
    table: ConfigurationTable<'a>,
    peripheral_ppm: u16,
    settle: fn(u32),
    advertisement: Option<Advertisement>,
    request: Option<(Peer, ConnectionParameters)>,
    connected: bool,
    setup_seen: bool,
    deferred_rx: Option<LinkFrame>,
    token: Option<u32>,
    last_counter: Option<u16>,
    events: OwnedQueue<LinkEvent, 16>,
    interval: u16,
    central_ppm: u16,
    waiting: Option<ConnectionUpdate>,
    pending: Option<PendingConnectionUpdate>,
    map: Option<[u8; 8]>,
}
impl<'a, R: ByteRegisters> SydBackend<'a, R> {
    /// Takes exclusive ownership without accessing hardware. Reset performs RF initialization.
    #[must_use]
    pub const fn new(
        radio: Radio<R>,
        table: ConfigurationTable<'a>,
        peripheral_ppm: u16,
        settle: fn(u32),
    ) -> Self {
        Self {
            radio,
            table,
            peripheral_ppm,
            settle,
            advertisement: None,
            request: None,
            connected: false,
            setup_seen: false,
            deferred_rx: None,
            token: None,
            last_counter: None,
            events: OwnedQueue::new(),
            interval: 0,
            central_ppm: 0,
            waiting: None,
            pending: None,
            map: None,
        }
    }
    /// Returns the register owner after stopping or a fault, for board recovery.
    #[must_use]
    pub fn into_radio(self) -> Radio<R> {
        self.radio
    }
    fn clear(&mut self) {
        self.advertisement = None;
        self.request = None;
        self.connected = false;
        self.setup_seen = false;
        self.deferred_rx = None;
        self.token = None;
        self.last_counter = None;
        self.events.clear();
        self.waiting = None;
        self.pending = None;
        self.map = None;
    }
    fn emit(&mut self, event: LinkEvent) -> Result<(), SydError<R::Error>> {
        self.events.push(event).map_err(|_| SydError::Capacity)
    }
    fn queue_advertising(&mut self) -> Result<(), SydError<R::Error>> {
        let ad = self
            .advertisement
            .as_ref()
            .ok_or(SydError::ProtocolAt { site: 1 })?;
        let address = StaticRandomAddress::new(ad.address.bytes())
            .map_err(|_| SydError::ProtocolAt { site: 2 })?;
        let pdu = AdvertisingPdu::connectable(address, ad.data.as_slice())
            .map_err(|_| SydError::ProtocolAt { site: 3 })?;
        let scan = AdvertisingPdu::scan_response(address, ad.scan_response.as_slice())
            .map_err(|_| SydError::ProtocolAt { site: 4 })?;
        self.radio.queue_advertisement(&pdu)?;
        self.radio.queue_scan_response(&scan)?;
        self.radio.start_advertising()?;
        Ok(())
    }
    fn configure_advertising(&mut self, ad: &Advertisement) -> Result<(), SydError<R::Error>> {
        let address = StaticRandomAddress::new(ad.address.bytes())
            .map_err(|_| SydError::ProtocolAt { site: 16 })?;
        let parameters = syd881x_radio::AdvertisingParameters::new(
            1,
            ad.parameters.min_interval(),
            ad.parameters.channels(),
        )
        .map_err(|_| SydError::ProtocolAt { site: 17 })?;
        self.radio.configure_advertising(parameters, address)?;
        self.advertisement = Some(*ad);
        self.queue_advertising()
    }
    // Loss before establishment is an aborted attempt, not a fatal Host error.
    // Keep its reconfiguration scratch out of the normal RX polling frame.
    #[inline(never)]
    fn link_lost(&mut self) -> Result<(), SydError<R::Error>> {
        let connected = self.connected;
        let advertisement = self.advertisement.take();
        self.clear();
        self.radio.stop_link()?;
        if connected {
            self.emit(LinkEvent::Disconnected(8))
        } else {
            self.clear_aborted_receive()?;
            if let Some(advertisement) = advertisement {
                self.configure_advertising(&advertisement)
            } else {
                Ok(())
            }
        }
    }
    // SR_P is not assumed to flush FIFO/INTST. Discard only stopped-link RX,
    // with a finite packet budget, and verify a quiescent epoch before restart.
    #[inline(never)]
    fn clear_aborted_receive(&mut self) -> Result<(), SydError<R::Error>> {
        for _ in 0..32 {
            if self.radio.receive()?.is_none() {
                break;
            }
        }
        let stopped = self.radio.poll_status()?;
        if stopped.fifo_status & 8 == 0 {
            return Err(SydError::Capacity);
        }
        if stopped.fifo_status & 1 != 0 {
            return Err(RadioError::FifoFull.into());
        }
        if stopped.fifo_status & 2 == 0 || stopped.advertising_control & 1 != 0 {
            return Err(RadioError::StatusTimeout.into());
        }
        self.radio.acknowledge_interrupt()?;
        let fresh = self.radio.poll_status()?;
        if fresh.interrupt_status != 0
            || fresh.fifo_status & 0b1011 != 0b1010
            || fresh.advertising_control & 1 != 0
        {
            return Err(RadioError::StatusTimeout.into());
        }
        Ok(())
    }
    fn establish(&mut self) -> Result<(), SydError<R::Error>> {
        if self.setup_seen
            && !self.connected
            && let Some((peer, parameters)) = self.request
        {
            self.connected = true;
            self.advertisement = None;
            self.emit(LinkEvent::Connected(peer, parameters))?;
        }
        if self.connected
            && let Some(frame) = self.deferred_rx.take()
        {
            self.emit(LinkEvent::Received(frame))?;
        }
        Ok(())
    }
    // RX parsing, update handling and advertising refill execute serially. Keep
    // their packet/window temporaries out of each other's polling stack.
    #[inline(never)]
    fn receive(&mut self) -> Result<(), SydError<R::Error>> {
        if self.deferred_rx.is_some() {
            return Ok(());
        }
        for _ in 0..8 {
            // Leave capacity for connection, ACK and update observations.
            if self.events.len() > 12 {
                break;
            }
            let Some(packet) = self.radio.receive()? else {
                return Ok(());
            };
            if packet.is_data_channel() {
                self.establish()?;
                let kind = match packet.fifo_header()[0] & 3 {
                    1 => FrameKind::Continuation,
                    2 => FrameKind::Start,
                    3 => FrameKind::Control,
                    _ => return Err(SydError::ProtocolAt { site: 5 }),
                };
                let frame = LinkFrame::new(kind, packet.payload())
                    .map_err(|_| SydError::ProtocolAt { site: 6 })?;
                if !self.connected {
                    if self.request.is_none() {
                        return Err(SydError::ProtocolAt { site: 7 });
                    }
                    // CONNSET can become visible after the snapshot. Keep this owned
                    // first packet until a later capture establishes causal ordering.
                    self.deferred_rx = Some(frame);
                    return Ok(());
                }
                if !frame.payload().is_empty() {
                    self.emit(LinkEvent::Received(frame))?;
                }
            } else if packet.fifo_header()[0] & 15 == 5 {
                let ad = self
                    .advertisement
                    .as_ref()
                    .ok_or(SydError::ProtocolAt { site: 8 })?;
                let address = StaticRandomAddress::new(ad.address.bytes())
                    .map_err(|_| SydError::ProtocolAt { site: 9 })?;
                let request = ConnectionRequest::parse(&packet, address)
                    .map_err(|_| SydError::ProtocolAt { site: 10 })?;
                let parameters =
                    ConnectionParameters::new(request.interval_units(), 0, request.timeout_units())
                        .map_err(|_| SydError::ProtocolAt { site: 11 })?;
                let windows = ConnectionWindows::for_board_clock(&request, self.peripheral_ppm)
                    .map_err(|_| SydError::ProtocolAt { site: 12 })?;
                self.radio.configure_connection_parameters(&request)?;
                self.radio.configure_connection_windows(windows)?;
                self.interval = request.interval_units();
                self.central_ppm = request.central_clock_accuracy_ppm();
                let bytes = packet.payload();
                self.request = Some((
                    Peer::new(
                        [bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5]],
                        packet.fifo_header()[0] & 0x40 != 0,
                    ),
                    parameters,
                ));
                self.establish()?;
            }
        }
        Ok(())
    }
    #[inline(never)]
    fn updates(&mut self, boundary: Option<u16>) -> Result<(), SydError<R::Error>> {
        if let Some(payload) = self.map {
            match self.radio.apply_channel_map_indication(&payload) {
                Ok(()) => self.map = None,
                Err(RadioError::UpdateBusy) => {}
                Err(e) => return Err(e.into()),
            }
        }
        if let Some(update) = self.waiting {
            match self.radio.schedule_connection_update(
                update,
                self.interval,
                self.central_ppm,
                self.peripheral_ppm,
            ) {
                Ok(p) => {
                    self.pending = Some(p);
                    self.waiting = None;
                }
                Err(RadioError::UpdateBusy) => {}
                Err(e) => return Err(e.into()),
            }
        }
        if let Some(pending) = self.pending.as_mut()
            && self.radio.poll_connection_update(pending, boundary)? == UpdateProgress::Complete
        {
            let p = pending.parameters();
            self.interval = p[0];
            self.pending = None;
            self.emit(LinkEvent::ParametersUpdated(
                ConnectionParameters::new(p[0], p[1], p[2])
                    .map_err(|_| SydError::ProtocolAt { site: 13 })?,
            ))?;
        }
        Ok(())
    }
    fn sample(&mut self) -> Result<(), SydError<R::Error>> {
        let status = self.radio.poll_status()?;
        let mut boundary = None;
        let mut bits = 0;
        if status.interrupt_status != 0 {
            let event = self.radio.take_connection_events(self.last_counter)?;
            bits = event.bits();
            boundary = event.setup_counter();
            if self.connected || bits & 8 != 0 {
                self.last_counter = Some(event.counter());
            }
            self.setup_seen |= bits & 8 != 0;
            if bits & 16 != 0 {
                return self.link_lost();
            }
            if bits & 1 != 0
                && self.connected
                && self.radio.poll_status()?.fifo_status & 2 != 0
                && let Some(token) = self.token.take()
            {
                self.emit(LinkEvent::Acknowledged(token))?;
            }
        }
        self.receive()?;
        self.establish()?;
        self.updates(boundary)?;
        if bits & 4 != 0
            && self.advertisement.is_some()
            && self.request.is_none()
            && !self.setup_seen
        {
            let fresh = self.radio.poll_status()?;
            if fresh.interrupt_status & 8 == 0 && fresh.fifo_status & 8 != 0 {
                self.queue_advertising()?;
            }
        }
        Ok(())
    }
}
impl<R> LinkBackend for SydBackend<'_, R>
where
    R: ByteRegisters + core::fmt::Debug,
    R::Error: core::fmt::Debug,
{
    type Error = SydError<R::Error>;
    fn reset(&mut self) -> Result<(), Self::Error> {
        self.clear();
        self.radio.prepare_standby(self.settle)?;
        (self.settle)(2000);
        self.radio
            .initialize_with_calibration_budget(
                &self.table,
                NonZeroU16::new(4096).unwrap_or(NonZeroU16::MIN),
            )
            .map_err(SydError::Initialization)?;
        self.radio.acknowledge_interrupt()?;
        Ok(())
    }
    fn advertise(&mut self, ad: &Advertisement) -> Result<(), Self::Error> {
        if self.connected || self.advertisement.is_some() {
            return Err(SydError::ProtocolAt { site: 15 });
        }
        self.configure_advertising(ad)
    }
    fn stop(&mut self) -> Result<(), Self::Error> {
        self.radio.stop_link()?;
        self.radio.acknowledge_interrupt()?;
        self.clear();
        Ok(())
    }
    fn poll(&mut self) -> Result<Option<LinkEvent>, Self::Error> {
        if let Some(e) = self.events.pop() {
            return Ok(Some(e));
        }
        self.sample()?;
        Ok(self.events.pop())
    }
    fn transmit(&mut self, token: u32, frame: LinkFrame) -> Result<TxAdmission, Self::Error> {
        if !self.connected {
            return Err(SydError::ProtocolAt { site: 18 });
        }
        if self.token.is_some() {
            return Ok(TxAdmission::Busy);
        }
        // Consume stale TXINT and preserve all other observations before FIFO commit.
        self.sample()?;
        if !self.events.is_empty() {
            return Ok(TxAdmission::Busy);
        }
        if !self.connected {
            return Err(SydError::ProtocolAt { site: 19 });
        }
        let pdu = DataPdu::fragment(frame.kind() as u8, frame.payload())
            .map_err(|_| SydError::ProtocolAt { site: 20 })?
            .with_more_data(frame.as_pdu()[0] & 0x10 != 0);
        match self.radio.queue_data(&pdu) {
            Ok(()) => {
                self.token = Some(token);
                Ok(TxAdmission::Committed)
            }
            Err(RadioError::FifoFull) => Ok(TxAdmission::Busy),
            Err(e) => Err(e.into()),
        }
    }
    fn connection_update(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        if !self.connected || self.waiting.is_some() || self.pending.is_some() {
            return Err(SydError::ProtocolAt { site: 21 });
        }
        self.waiting =
            Some(ConnectionUpdate::parse(bytes).map_err(|_| SydError::ProtocolAt { site: 22 })?);
        Ok(())
    }
    fn channel_map(&mut self, bytes: &[u8]) -> Result<(), Self::Error> {
        if !self.connected || self.map.is_some() || bytes.len() != 8 {
            return Err(SydError::ProtocolAt { site: 23 });
        }
        self.map = Some(
            bytes
                .try_into()
                .map_err(|_| SydError::ProtocolAt { site: 24 })?,
        );
        Ok(())
    }
}
