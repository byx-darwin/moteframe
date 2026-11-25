//! Legacy `CONNECT_IND` validation and the pinned ROM connection register subset.
use crate::{
    Bank, ByteRegisters, ConnectionUpdate, Radio, RadioError, ReceivedPacket, Register,
    StaticRandomAddress,
};

/// A received request cannot be used for this legacy connection experiment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvalidConnection {
    /// Wrong PDU type, metadata, length or unsupported channel selection mode.
    Shape,
    /// `AdvA` or its random/public flag does not match this advertiser.
    Advertiser,
    /// The 1M data access address fails its structural constraints.
    AccessAddress,
    /// Interval/window/latency/supervision parameters are invalid or inconsistent.
    Timing,
    /// Reserved channel map bits are set or fewer than two channels are usable.
    ChannelMap,
    /// Hop increment is outside5..16.
    Hop,
}

/// Validated owned legacy connection request; Debug omits peer/payload bytes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ConnectionRequest {
    bytes: [u8; 34],
    interval: u16,
    latency: u16,
    timeout: u16,
    offset: u16,
}
impl core::fmt::Debug for ConnectionRequest {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ConnectionRequest")
            .field("payload_len", &34)
            .finish_non_exhaustive()
    }
}
impl ConnectionRequest {
    /// Validates a legacy CSA1 request for this static random advertiser.
    /// # Errors
    /// Rejects incorrect shape/address, malformed1M AA, channel/hop fields or
    /// inconsistent timing before a `ConnectionRequest` can reach any MMIO.
    pub fn parse(
        packet: &ReceivedPacket,
        own: StaticRandomAddress,
    ) -> Result<Self, InvalidConnection> {
        let header = packet.fifo_header()[0];
        let payload = packet.payload();
        if packet.is_data_channel() || header & 0x3f != 5 || payload.len() != 34 {
            return Err(InvalidConnection::Shape);
        }
        if header & 0x80 == 0 || payload[6..12] != own.0 {
            return Err(InvalidConnection::Advertiser);
        }
        let bytes: [u8; 34] = payload.try_into().map_err(|_| InvalidConnection::Shape)?;
        let aa = u32::from_le_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
        if !valid_access_address(aa) {
            return Err(InvalidConnection::AccessAddress);
        }
        if bytes[32] & 0xe0 != 0 || bytes[28..33].iter().map(|b| b.count_ones()).sum::<u32>() < 2 {
            return Err(InvalidConnection::ChannelMap);
        }
        if !(5..=16).contains(&(bytes[33] & 31)) {
            return Err(InvalidConnection::Hop);
        }
        let offset = u16::from_le_bytes([bytes[20], bytes[21]]);
        let interval = u16::from_le_bytes([bytes[22], bytes[23]]);
        let latency = u16::from_le_bytes([bytes[24], bytes[25]]);
        let timeout = u16::from_le_bytes([bytes[26], bytes[27]]);
        if !(6..=3200).contains(&interval)
            || latency > 499
            || !(10..=3200).contains(&timeout)
            || !(1..=8).contains(&bytes[19])
            || u16::from(bytes[19]) >= interval
            || offset > interval
            || u32::from(timeout) * 4 <= (u32::from(latency) + 1) * u32::from(interval)
        {
            return Err(InvalidConnection::Timing);
        }
        Ok(Self {
            bytes,
            interval,
            latency,
            timeout,
            offset,
        })
    }
    /// Connection interval in1.25ms units.
    #[must_use]
    pub const fn interval_units(&self) -> u16 {
        self.interval
    }
    /// Supervision timeout in10ms units.
    #[must_use]
    pub const fn timeout_units(&self) -> u16 {
        self.timeout
    }
    /// Transmit window offset in1.25ms units.
    #[must_use]
    pub const fn window_offset_units(&self) -> u16 {
        self.offset
    }
    /// Worst-case central clock accuracy from the request's SCA index.
    #[must_use]
    pub fn central_clock_accuracy_ppm(&self) -> u16 {
        [500, 250, 150, 100, 75, 50, 30, 20][usize::from(self.bytes[33] >> 5)]
    }
}

fn valid_access_address(aa: u32) -> bool {
    let octets = aa.to_le_bytes();
    if (aa ^ 0x8e89_bed6).count_ones() <= 1 || octets.iter().all(|b| *b == octets[0]) {
        return false;
    }
    let mut run = 1;
    let mut transitions = 0;
    let mut high_transitions = 0;
    for bit in 1..32 {
        if (aa >> bit) & 1 == (aa >> (bit - 1)) & 1 {
            run += 1;
            if run > 6 {
                return false;
            }
        } else {
            run = 1;
            transitions += 1;
            if bit >= 27 {
                high_transitions += 1;
            }
        }
    }
    transitions <= 24 && high_transitions >= 2
}

/// Pinned-board ceiling/offset-zero/minus-one window policy, in hardware ticks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnectionWindows {
    widening: u16,
    setup: u8,
    minus_count: u8,
    over_half: bool,
}
impl ConnectionWindows {
    /// Evaluates the pinned ROM float32-promoted constants as an exact rational.
    ///
    /// Matches this board's ceiling=true, offset=0, minus-one=true policy. The
    /// denominator/numerator reproduce `(ppm*T*float32(.001)/30.25*1.25)+float32(.48)`.
    /// Hardware timing and oscillator accuracy still require physical evidence.
    /// # Errors
    /// Rejects a first-window or reciprocal counter above255 instead of the
    /// ROM's silent8-bit truncation; all arithmetic uses boundedu64 inputs.
    pub fn for_board_clock(
        request: &ConnectionRequest,
        peripheral_ppm: u16,
    ) -> Result<Self, InvalidConnection> {
        Self::for_timing(
            request.interval,
            request.offset + 1,
            request.central_clock_accuracy_ppm(),
            peripheral_ppm,
        )
    }
    /// Window arithmetic for an update; setup spans old interval plus adjusted offset.
    /// # Errors
    /// Rejects unsupported board timings/counters instead of truncating hardware fields.
    pub fn for_update(
        update: ConnectionUpdate,
        old_interval: u16,
        central_ppm: u16,
        peripheral_ppm: u16,
    ) -> Result<Self, InvalidConnection> {
        if !(6..=3200).contains(&old_interval) || central_ppm > 500 {
            return Err(InvalidConnection::Timing);
        }
        Self::for_timing(
            update.interval,
            old_interval + update.offset.saturating_sub(1),
            central_ppm,
            peripheral_ppm,
        )
    }
    fn for_timing(
        interval: u16,
        setup_time: u16,
        central_ppm: u16,
        peripheral_ppm: u16,
    ) -> Result<Self, InvalidConnection> {
        const D: u64 = 1_039_382_085_632;
        let ppm = u64::from(peripheral_ppm) + u64::from(central_ppm);
        let numerator = |t: u16| ppm * u64::from(t) * 42_949_675 + 498_903_389_952;
        let n = numerator(interval);
        let q = n.div_ceil(D);
        let setup = numerator(setup_time).div_ceil(D);
        let widening = u16::try_from(q).map_err(|_| InvalidConnection::Timing)?;
        let setup = u8::try_from(setup).map_err(|_| InvalidConnection::Timing)?;
        let gap = q * D - n;
        let (over_half, count) = if gap == 0 || gap == D {
            (false, 31)
        } else if gap > D / 2 {
            (true, D / (D - gap))
        } else {
            (false, D / gap)
        };
        let minus_count = u8::try_from(count)
            .map_err(|_| InvalidConnection::Timing)?
            .min(31);
        Ok(Self {
            widening,
            setup,
            minus_count,
            over_half,
        })
    }
    /// Connection window widening register value.
    #[must_use]
    pub const fn widening_ticks(self) -> u16 {
        self.widening
    }
    /// Initial transmit-window widening register value.
    #[must_use]
    pub const fn setup_ticks(self) -> u8 {
        self.setup
    }
}

impl<R: ByteRegisters> Radio<R> {
    /// Schedules a legacy `LL_CHANNEL_MAP_IND` through the pending-map registers.
    ///
    /// Reproduces the ROM map-at0xca and instant-at0x38, then update bit1.
    /// Does not overwrite the live map at0x98 early. Counter sampling is bounded
    /// high/low/high to avoid a torn event count near a byte rollover.
    /// # Errors
    /// Rejects malformed maps, past instants and an already-pending update before
    /// writes; returns adapter failures or `StatusTimeout` for unstable sampling.
    pub fn apply_channel_map_indication(
        &mut self,
        payload: &[u8],
    ) -> Result<(), RadioError<R::Error>> {
        if payload.len() != 8
            || payload[0] != 1
            || payload[5] & 0xe0 != 0
            || payload[1..6].iter().map(|b| b.count_ones()).sum::<u32>() < 2
        {
            return Err(RadioError::InvalidParameters);
        }
        let instant = u16::from_le_bytes([payload[6], payload[7]]);
        let (current, _) = self.update_control(2, instant)?;
        let distance = instant.wrapping_sub(current);
        if distance == 0 || distance >= 0x7fff {
            return Err(RadioError::InvalidParameters);
        }
        for (offset, &value) in (0xca..=0xce).zip(payload[1..6].iter()) {
            self.write(Register::new(Bank::Ll, offset), value)?;
        }
        self.write(Register::new(Bank::Ll, 0x38), payload[6])?;
        self.write(Register::new(Bank::Ll, 0x39), payload[7])?;
        self.arm_update(2, current, instant)
    }

    /// Applies validated pinned-board window values, preserving unrelated bits.
    /// # Errors
    /// Returns adapter failures; partial hardware writes require reset recovery.
    pub fn configure_connection_windows(
        &mut self,
        windows: ConnectionWindows,
    ) -> Result<(), RadioError<R::Error>> {
        self.configure_widening(windows)?;
        self.write(Register::new(Bank::Ll, 0x36), windows.setup)?;
        self.write(Register::new(Bank::Ll, 0x37), 0)
    }
    pub(crate) fn configure_widening(
        &mut self,
        windows: ConnectionWindows,
    ) -> Result<(), RadioError<R::Error>> {
        let over = Register::new(Bank::Ll, 0x79);
        let previous = self.read(over)?;
        self.write(over, (previous & !1) | u8::from(windows.over_half))?;
        let count = Register::new(Bank::Ll, 0x78);
        let previous = self.read(count)?;
        self.write(count, (previous & !31) | windows.minus_count)?;
        let [lo, hi] = windows.widening.to_le_bytes();
        self.write(Register::new(Bank::Ll, 0x3a), lo)?;
        self.write(Register::new(Bank::Ll, 0x3b), hi)?;
        Ok(())
    }
    /// Writes confirmed core connection parameters with deterministic zero latency.
    ///
    /// Applies ROM's offset+1/timeout+2 exactly once. This subset does not compute
    /// window widening or imply a link exists; caller must separately configure
    /// its clock/window policy and verify real connection/data events.
    /// # Errors
    /// Rejects nonzero latency for this polling experiment before MMIO; otherwise
    /// returns adapter failures with partial register changes requiring reset.
    pub fn configure_connection_parameters(
        &mut self,
        request: &ConnectionRequest,
    ) -> Result<(), RadioError<R::Error>> {
        if request.latency != 0 {
            return Err(RadioError::InvalidParameters);
        }
        self.write(Register::new(Bank::Ll, 0x25), request.bytes[33] & 31)?;
        for (offset, values) in [
            (0x98, &request.bytes[28..33]),
            (0x94, &request.bytes[16..19]),
            (0x88, &request.bytes[12..16]),
        ] {
            for (index, &byte) in values.iter().enumerate() {
                let index = u8::try_from(index).map_err(|_| RadioError::InvalidParameters)?;
                self.write(Register::new(Bank::Ll, offset + index), byte)?;
            }
        }
        for (offset, value) in [
            (0x26, request.offset + 1),
            (0x2a, request.interval),
            (0x2c, 0),
            (0x2e, request.timeout + 2),
        ] {
            let [lo, hi] = value.to_le_bytes();
            self.write(Register::new(Bank::Ll, offset), lo)?;
            self.write(Register::new(Bank::Ll, offset + 1), hi)?;
        }
        self.write(Register::new(Bank::Ll, 0x28), request.bytes[19])
    }
}
