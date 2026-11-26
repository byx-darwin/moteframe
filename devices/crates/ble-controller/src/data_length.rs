//! Directional LE data-channel limits for uncoded PHYs.
/// Maximum data octets and on-air times in peripheral transmit/receive order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DataLength {
    /// Maximum local transmit payload in octets.
    pub tx_octets: u16,
    /// Maximum local transmit packet duration in microseconds.
    pub tx_time: u16,
    /// Maximum local receive payload in octets.
    pub rx_octets: u16,
    /// Maximum local receive packet duration in microseconds.
    pub rx_time: u16,
}
impl DataLength {
    /// Initial effective limits on every new uncoded connection.
    pub const LEGACY: Self = Self {
        tx_octets: 27,
        tx_time: 328,
        rx_octets: 27,
        rx_time: 328,
    };
    /// Full 251-octet uncoded packets in both directions.
    pub const MAX: Self = Self {
        tx_octets: 251,
        tx_time: 2120,
        rx_octets: 251,
        rx_time: 2120,
    };
}

#[cfg(feature = "data-length")]
impl DataLength {
    pub(crate) fn from_peer(p: &[u8]) -> Option<Self> {
        let fields: &[u8; 8] = p.try_into().ok()?;
        let word = |i: usize| u16::from_le_bytes([fields[i], fields[i + 1]]);
        let peer = Self {
            rx_octets: word(0),
            rx_time: word(2),
            tx_octets: word(4),
            tx_time: word(6),
        };
        (Self::valid_pair(peer.rx_octets, peer.rx_time)
            && Self::valid_pair(peer.tx_octets, peer.tx_time))
        .then_some(peer)
    }

    pub(crate) fn valid_pair(octets: u16, time: u16) -> bool {
        (27..=251).contains(&octets) && (328..=17040).contains(&time)
    }

    pub(crate) fn effective(self, peer: Self) -> Self {
        Self {
            tx_octets: self.tx_octets.min(peer.rx_octets),
            tx_time: self.tx_time.min(peer.rx_time),
            rx_octets: self.rx_octets.min(peer.tx_octets),
            rx_time: self.rx_time.min(peer.tx_time),
        }
    }

    pub(crate) fn control(self, opcode: u8) -> [u8; 9] {
        let mut p = [0; 9];
        p[0] = opcode;
        for (field, bytes) in [self.rx_octets, self.rx_time, self.tx_octets, self.tx_time]
            .iter()
            .zip(p[1..].chunks_exact_mut(2))
        {
            bytes.copy_from_slice(&field.to_le_bytes());
        }
        p
    }
}

#[cfg(feature = "data-length")]
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Procedure {
    Wanted,
    Waiting,
    Idle,
    Unsupported,
}

#[cfg(feature = "data-length")]
#[derive(Debug)]
pub(crate) struct LengthState {
    pub local: DataLength,
    pub peer: DataLength,
    pub effective: DataLength,
    pub procedure: Procedure,
}
#[cfg(feature = "data-length")]
impl LengthState {
    pub const fn new(mut local: DataLength) -> Self {
        if local.tx_time > DataLength::MAX.tx_time {
            local.tx_time = DataLength::MAX.tx_time;
        }
        Self {
            local,
            peer: DataLength::LEGACY,
            effective: DataLength::LEGACY,
            procedure: Procedure::Wanted,
        }
    }
}
