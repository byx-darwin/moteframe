//! Connection-local LE 1M/2M policy and FR8029D data-channel rate fields.

const DEGRADED_EVENTS: u8 = 8;
const INITIAL_SETTLE_EVENTS: u8 = 16;
const RECOVERY_EVENTS: u8 = 128;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum PeerRates {
    #[default]
    Unknown,
    Requested,
    OneM,
    OneAndTwoM,
}

/// Peripheral PHY state. New legacy connections start in 1M in both directions.
#[derive(Debug, Default)]
pub struct LinkPhy {
    rates: u8,
    pending: Option<(u16, u8)>,
    peer_rates: PeerRates,
    negotiating: bool,
    prefer_one_m: bool,
    bad_events: u8,
    good_events: u8,
    retry_delay: u8,
}

impl LinkPhy {
    /// Record the remote LE 2M feature bit.
    pub fn peer_features(&mut self, two_m: bool) {
        self.peer_rates = if two_m {
            PeerRates::OneAndTwoM
        } else {
            PeerRates::OneM
        };
    }

    /// Give the central the first opportunity, then query once on this connection.
    pub fn take_feature_query(&mut self) -> bool {
        if self.peer_rates != PeerRates::Unknown || self.good_events < INITIAL_SETTLE_EVENTS {
            return false;
        }
        self.peer_rates = PeerRates::Requested;
        true
    }

    /// Allowed rates for a peer-initiated PHY procedure.
    pub fn respond(&mut self) -> u8 {
        self.negotiating = true;
        if self.prefer_one_m { 1 } else { 3 }
    }

    /// A rejected local preference leaves the current air rate intact.
    pub fn rejected(&mut self) {
        self.retry_delay = RECOVERY_EVENTS;
        self.clear_procedure();
    }

    fn clear_procedure(&mut self) {
        self.negotiating = false;
        self.good_events = 0;
        self.bad_events = 0;
    }

    /// Validate the central's indication, including separate directions and counter wrap.
    pub fn schedule(&mut self, next: u16, instant: u16, central_tx: u8, peripheral_tx: u8) -> bool {
        if self.pending.is_some() {
            return false;
        }
        // Reserved, unsupported or multi-bit selections preserve that direction.
        if !(1..=2).contains(&central_tx) && !(1..=2).contains(&peripheral_tx) {
            self.rejected();
            return true;
        }
        if instant.wrapping_sub(next) >= 0x8000 {
            return false;
        }
        let field = |choice, current| match choice {
            1 => 0,
            2 => 1,
            _ => current,
        };
        let tx = field(peripheral_tx, self.rates & 3);
        let rx = field(central_tx, self.rates >> 2);
        self.pending = Some((instant, tx | (rx << 2)));
        true
    }

    /// Return the new low nibble of the connection rate register at the exact event.
    pub fn apply(&mut self, event: u16) -> Option<u8> {
        let (instant, rates) = self.pending?;
        if event != instant {
            return None;
        }
        self.pending = None;
        let changed = self.rates != rates;
        self.rates = rates;
        self.retry_delay = 0;
        self.clear_procedure();
        changed.then_some(rates)
    }

    /// Whether the peripheral receives this connection event on LE 2M.
    #[must_use]
    pub const fn rx_two_m(&self) -> bool {
        self.rates & 4 != 0
    }

    /// Count consecutive impaired or healthy completed events, excluding skipped slots.
    pub fn observe_event(&mut self, impaired: bool) {
        self.retry_delay = self.retry_delay.saturating_sub(1);
        if impaired {
            self.bad_events = self.bad_events.saturating_add(1);
            self.good_events = 0;
        } else {
            self.good_events = self.good_events.saturating_add(1);
            self.bad_events = 0;
        }
    }

    /// Desired PHY for a new local procedure; leave time for a central-initiated update.
    pub fn take_request(&mut self) -> Option<u8> {
        if self.peer_rates != PeerRates::OneAndTwoM
            || self.negotiating
            || self.pending.is_some()
            || self.retry_delay != 0
        {
            return None;
        }
        let rate = if self.rates != 0 && self.bad_events >= DEGRADED_EVENTS {
            self.prefer_one_m = true;
            1
        } else if self.rates != 5
            && self.good_events
                >= if self.prefer_one_m {
                    RECOVERY_EVENTS
                } else {
                    INITIAL_SETTLE_EVENTS
                }
        {
            self.prefer_one_m = false;
            2
        } else {
            return None;
        };
        self.negotiating = true;
        Some(rate)
    }
}
