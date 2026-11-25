//! Legacy LE 1M connection requests and FR8029D exchange-memory event records.

use crate::advertising::CLOCK_MASK;

const HALF_US_PER_TICK: i64 = 625;
const LAST_FINE_COUNT: u16 = 624;
const HALF_US_PER_CONN_UNIT: i64 = 2500;
const CONNECT_IND_AIRTIME_HALF_US: i64 = 704;
// The configured FR8029D RX profile has a 40 us base and a 7 us PHY delay.
const RX_TIMESTAMP_DELAY_HALF_US: i64 = 94;
const RECEIVE_GUARD_HALF_US: i64 = 64;
// The control record extends the scheduler's RX window by 14 us.
const RECEIVE_TAIL_HALF_US: i64 = 28;
// Legacy 1M RX and TX at 27 octets plus the base event turnaround budget.
const EVENT_BASE_HALF_US: i64 = 1932;
const CENTRAL_SCA_PPM: [i64; 8] = [500, 250, 150, 100, 75, 50, 30, 20];
// Use the controller's conservative oscillator allowance for the bring-up probe.
const LOCAL_SCA_PPM: i64 = 500;

/// A rejected legacy connection request on the static-random advertising set.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ConnectError {
    /// The PDU type, length or receiver address type did not match.
    Header,
    /// The receiver address was different from the local advertiser.
    Advertiser,
    /// The transmit window fell outside the link-layer limits.
    Window,
    /// The connection interval fell outside the legacy limits.
    Interval,
    /// Peripheral latency and supervision timeout were invalid or incompatible.
    Supervision,
    /// The map used reserved bits or included fewer than two data channels.
    ChannelMap,
    /// The CSA1 hop increment was outside 5 through 16.
    Hop,
    /// The update instant has passed or falls outside the forward counter range.
    Instant,
}

/// Parsed legacy `CONNECT_IND` from a central, with timing in link-layer units.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConnectionRequest {
    /// Initiator address in link-layer byte order.
    pub initiator: [u8; 6],
    /// True when `TxAdd` identifies a random initiator address.
    pub initiator_random: bool,
    /// Data channel access address.
    pub access_address: u32,
    /// Data channel CRC initialization value in the low 24 bits.
    pub crc_init: u32,
    /// Initial transmit window size in 1.25 ms units.
    pub window_size: u8,
    /// Initial transmit window offset in 1.25 ms units.
    pub window_offset: u16,
    /// Time between connection events in 1.25 ms units.
    pub interval: u16,
    /// Maximum number of consecutive events the peripheral may skip.
    pub latency: u16,
    /// Supervision timeout in 10 ms units.
    pub supervision_timeout: u16,
    /// Data channel map, least-significant channel first.
    pub channel_map: [u8; 5],
    /// Channel Selection Algorithm #1 hop increment.
    pub hop: u8,
    /// Three-bit central sleep clock accuracy code.
    pub sca: u8,
}

impl ConnectionRequest {
    /// Decodes a `CONNECT_IND` addressed to this static-random advertiser.
    ///
    /// Both receiver address bytes and `RxAdd` must match. The local
    /// advertiser offers CSA1, so the incoming `ChSel` bit is ignored.
    ///
    /// # Errors
    ///
    /// Returns a corresponding error for an incompatible header, destination,
    /// transmit window, interval, supervision setting, map or hop.
    pub fn decode(
        header: u16,
        payload: &[u8; 34],
        advertiser: [u8; 6],
    ) -> Result<Self, ConnectError> {
        let [flags, length] = header.to_le_bytes();
        if flags & 0x8f != 0x85 || length != 34 {
            return Err(ConnectError::Header);
        }
        if payload[6..12] != advertiser {
            return Err(ConnectError::Advertiser);
        }
        let window_size = payload[19];
        let window_offset = u16::from_le_bytes([payload[20], payload[21]]);
        let interval = u16::from_le_bytes([payload[22], payload[23]]);
        let latency = u16::from_le_bytes([payload[24], payload[25]]);
        let supervision_timeout = u16::from_le_bytes([payload[26], payload[27]]);
        validate_timing(
            window_size,
            window_offset,
            interval,
            latency,
            supervision_timeout,
        )?;
        let channel_map = [
            payload[28],
            payload[29],
            payload[30],
            payload[31],
            payload[32],
        ];
        if channel_map[4] & 0xe0 != 0
            || channel_map
                .iter()
                .map(|byte| byte.count_ones())
                .sum::<u32>()
                < 2
        {
            return Err(ConnectError::ChannelMap);
        }
        let hop = payload[33] & 0x1f;
        if !(5..=16).contains(&hop) {
            return Err(ConnectError::Hop);
        }
        Ok(Self {
            initiator: [
                payload[0], payload[1], payload[2], payload[3], payload[4], payload[5],
            ],
            initiator_random: flags & 0x40 != 0,
            access_address: u32::from_le_bytes([
                payload[12],
                payload[13],
                payload[14],
                payload[15],
            ]),
            crc_init: u32::from_le_bytes([payload[16], payload[17], payload[18], 0]),
            window_size,
            window_offset,
            interval,
            latency,
            supervision_timeout,
            channel_map,
            hop,
            sca: payload[33] >> 5,
        })
    }

    /// Schedules event 0 from the received `CONNECT_IND` descriptor timestamp.
    #[must_use]
    pub fn first_event(self, received: PacketTime) -> ConnectionEvent {
        let window_half_us = i64::from(self.window_size) * HALF_US_PER_CONN_UNIT;
        let window_start_half_us = CONNECT_IND_AIRTIME_HALF_US
            + (i64::from(self.window_offset) + 1) * HALF_US_PER_CONN_UNIT;
        ConnectionEvent::open_window(
            received.data_anchor(),
            window_start_half_us,
            window_half_us,
            self.sca,
        )
    }

    /// Schedules a subsequent event after the most recent measured data anchor.
    ///
    /// The elapsed interval count includes missed events. The caller enforces
    /// supervision expiry and programs the event counter for the chosen event.
    #[must_use]
    pub fn event_after(self, anchor: PacketTime, intervals: u16) -> ConnectionEvent {
        let elapsed_half_us =
            i64::from(self.interval) * i64::from(intervals) * HALF_US_PER_CONN_UNIT;
        ConnectionEvent::open_window(anchor, elapsed_half_us, 0, self.sca)
    }

    /// Encodes the initial control words for the single LE 1M peripheral link.
    ///
    /// Pass the matching hardware link ID and the word-aligned byte offset
    /// of the first TX descriptor in Packet RAM.
    #[must_use]
    pub fn control_words(self, link_id: u8, tx_offset: u16, event: ConnectionEvent) -> [u16; 74] {
        let [aa0, aa1, aa2, aa3] = self.access_address.to_le_bytes();
        let [crc0, crc1, crc2, _] = self.crc_init.to_le_bytes();
        let [ch0, ch1, ch2, ch3, ch4] = self.channel_map;
        let mut words = [0; 74];
        words[0] = 0x0003;
        words[1] = u16::from_le_bytes([0x20, link_id]);
        // TX/RX PHY both use code 0 for 1M. The peripheral role is in word 0.
        words[0x06 / 2] = 0x1100;
        words[0x0e / 2] = u16::from_le_bytes([aa0, aa1]);
        words[0x10 / 2] = u16::from_le_bytes([aa2, aa3]);
        words[0x12 / 2] = u16::from_le_bytes([crc0, crc1]);
        words[0x14 / 2] = u16::from(crc2);
        words[0x18 / 2] = 0x8000 | (u16::from(self.hop) << 8);
        words[0x1a / 2] = 7;
        words[0x1e / 2] = event.receive_window;
        words[0x24 / 2] = tx_offset / 4;
        // Event limits use 625 us slots. Allow the four ROM programming
        // half-slots and two additional half-slots before the next event.
        let event_limit = self.interval.saturating_mul(2).saturating_sub(3);
        words[0x2e / 2] = event_limit;
        words[0x30 / 2] = event_limit;
        words[0x32 / 2] = u16::from_le_bytes([ch0, ch1]);
        words[0x34 / 2] = u16::from_le_bytes([ch2, ch3]);
        words[0x36 / 2] = u16::from(ch4);
        words[0x38 / 2] = 0xfb;
        words
    }
}

/// Peripheral timing from a legacy `LL_CONNECTION_UPDATE_IND`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConnectionUpdate {
    /// Connection event counter at which the new timing applies.
    pub instant: u16,
    /// New connection interval in 1.25 ms units.
    pub interval: u16,
    /// New maximum number of connection events the peripheral may skip.
    pub latency: u16,
    /// New supervision timeout in 10 ms units.
    pub supervision_timeout: u16,
    window_size: u8,
    window_offset: u16,
}

impl ConnectionUpdate {
    /// Decodes the complete control PDU using the last completed event counter.
    ///
    /// # Errors
    ///
    /// Returns a corresponding error for the wrong opcode or length, invalid
    /// link timing or an instant that is outside the forward counter range.
    pub fn decode(indication: &[u8], counter: u16) -> Result<Self, ConnectError> {
        let &[
            0,
            size,
            wo0,
            wo1,
            int0,
            int1,
            lat0,
            lat1,
            to0,
            to1,
            evt0,
            evt1,
        ] = indication
        else {
            return Err(ConnectError::Header);
        };
        let instant = u16::from_le_bytes([evt0, evt1]);
        let distance = instant.wrapping_sub(counter);
        if !(1..0x7fff).contains(&distance) {
            return Err(ConnectError::Instant);
        }
        let window_offset = u16::from_le_bytes([wo0, wo1]);
        let interval = u16::from_le_bytes([int0, int1]);
        let latency = u16::from_le_bytes([lat0, lat1]);
        let supervision_timeout = u16::from_le_bytes([to0, to1]);
        validate_timing(size, window_offset, interval, latency, supervision_timeout)?;
        Ok(Self {
            instant,
            interval,
            latency,
            supervision_timeout,
            window_size: size,
            window_offset,
        })
    }

    /// Schedules the updated receive window from the most recent measured data anchor.
    ///
    /// The old interval and elapsed old intervals locate the event's original
    /// anchor. The update window offset starts at that anchor.
    #[must_use]
    pub fn event_at(
        self,
        anchor: PacketTime,
        old_interval: u16,
        elapsed_old_intervals: u16,
        sca: u8,
    ) -> ConnectionEvent {
        let nominal_delay = (i64::from(old_interval) * i64::from(elapsed_old_intervals)
            + i64::from(self.window_offset))
            * HALF_US_PER_CONN_UNIT;
        let width = i64::from(self.window_size) * HALF_US_PER_CONN_UNIT;
        ConnectionEvent::open_window(anchor, nominal_delay, width, sca)
    }
}

fn validate_timing(
    window_size: u8,
    window_offset: u16,
    interval: u16,
    latency: u16,
    supervision_timeout: u16,
) -> Result<(), ConnectError> {
    if !(6..=3200).contains(&interval) {
        return Err(ConnectError::Interval);
    }
    if !(1..=8).contains(&window_size)
        || u16::from(window_size) >= interval
        || window_offset > interval
    {
        return Err(ConnectError::Window);
    }
    if latency > 499
        || !(10..=3200).contains(&supervision_timeout)
        || u32::from(supervision_timeout) * 8 <= (u32::from(latency) + 1) * u32::from(interval) * 2
    {
        return Err(ConnectError::Supervision);
    }
    Ok(())
}

/// FR8029D native timestamp, wrapping every 2²⁸ coarse ticks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PacketTime {
    /// Coarse time in 312.5 us ticks, low 28 bits.
    pub coarse: u32,
    /// Fine countdown within the tick in half-microseconds, 624 down to 0.
    pub fine_countdown: u16,
}

impl PacketTime {
    /// Corrects a 1M data RX timestamp to the central's packet-start anchor.
    ///
    /// Pass the first CRC-valid data PDU timestamp for the connection event.
    #[must_use]
    pub fn data_anchor(self) -> Self {
        self.shift_half_us(-RX_TIMESTAMP_DELAY_HALF_US)
    }

    fn shift_half_us(self, half_us: i64) -> Self {
        let fine_elapsed = i64::from(LAST_FINE_COUNT) - i64::from(self.fine_countdown) + half_us;
        let coarse = (i64::from(self.coarse) + fine_elapsed.div_euclid(HALF_US_PER_TICK))
            & i64::from(CLOCK_MASK);
        let [a, b, c, d, ..] = coarse.to_le_bytes();
        Self {
            coarse: u32::from_le_bytes([a, b, c, d]),
            fine_countdown: LAST_FINE_COUNT
                - low_halfword(fine_elapsed.rem_euclid(HALF_US_PER_TICK)),
        }
    }
}

/// Timing for a single receive-first connection event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConnectionEvent {
    /// When the baseband starts the event and opens RX.
    pub start: PacketTime,
    /// Encoded receive window for control offset 0x1e.
    pub receive_window: u16,
    duration_half_us: i64,
}

impl ConnectionEvent {
    fn open_window(anchor: PacketTime, nominal_delay: i64, nominal_width: i64, sca: u8) -> Self {
        let ppm = LOCAL_SCA_PPM + CENTRAL_SCA_PPM[usize::from(sca & 7)];
        let elapsed = nominal_delay + nominal_width / 2;
        let guard = RECEIVE_GUARD_HALF_US + (elapsed * ppm + 999_999) / 1_000_000;
        let width = nominal_width + 2 * guard;
        Self {
            start: anchor.shift_half_us(nominal_delay - guard),
            receive_window: receive_window_word(width + RECEIVE_TAIL_HALF_US),
            duration_half_us: EVENT_BASE_HALF_US + width,
        }
    }

    /// Shifts this receive window by one interval in 1.25 ms units.
    ///
    /// Preserves the window when no packet has supplied a new anchor.
    #[must_use]
    pub fn one_interval_later(self, interval: u16) -> Self {
        Self {
            start: self
                .start
                .shift_half_us(i64::from(interval) * HALF_US_PER_CONN_UNIT),
            ..self
        }
    }

    /// Encodes the scheduler slot for the connection's control record.
    ///
    /// Pass the word-aligned byte offset of the control record in Packet RAM.
    #[must_use]
    pub fn slot_words(self, control_offset: u16) -> [u16; 8] {
        let [a, b, c, d] = (self.start.coarse & CLOCK_MASK).to_le_bytes();
        let duration = if self.duration_half_us < 0x8000 {
            low_halfword((self.duration_half_us + 1) / 2)
        } else {
            0x8000 | low_halfword((self.duration_half_us + HALF_US_PER_TICK) / HALF_US_PER_TICK)
        };
        [
            0x2802,
            u16::from_le_bytes([a, b]),
            u16::from_le_bytes([c, d]),
            self.start.fine_countdown,
            control_offset / 4,
            duration,
            0,
            0x0f00,
        ]
    }
}

fn receive_window_word(half_us: i64) -> u16 {
    if half_us + 1 < 0x8000 {
        low_halfword((half_us + 3) / 4)
    } else {
        let microseconds = (half_us + 1) / 2;
        0x8000 | low_halfword((microseconds + 624) / 625)
    }
}

fn low_halfword(value: i64) -> u16 {
    let [lo, hi, ..] = value.to_le_bytes();
    u16::from_le_bytes([lo, hi])
}
