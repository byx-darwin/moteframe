//! Legacy update staging recovered from pinned ROM `0x5b5c` and `0x139c`.
use crate::{
    Bank, ByteRegisters, ConnectionWindows, InvalidConnection, Radio, RadioError, Register,
};

/// Validated latency-zero update fields, excluding any peer data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConnectionUpdate {
    pub(crate) interval: u16,
    pub(crate) offset: u16,
    timeout: u16,
    size: u8,
    instant: u16,
}
impl ConnectionUpdate {
    /// Validates complete12-byte opcode0 PDU before any MMIO.
    /// # Errors
    /// Rejects malformed/reserved timings and nonzero latency unsupported by this fixture.
    pub fn parse(payload: &[u8]) -> Result<Self, InvalidConnection> {
        if payload.len() != 12 || payload[0] != 0 {
            return Err(InvalidConnection::Shape);
        }
        let offset = u16::from_le_bytes([payload[2], payload[3]]);
        let interval = u16::from_le_bytes([payload[4], payload[5]]);
        let latency = u16::from_le_bytes([payload[6], payload[7]]);
        let timeout = u16::from_le_bytes([payload[8], payload[9]]);
        if !(6..=3200).contains(&interval)
            || !(1..=8).contains(&payload[1])
            || u16::from(payload[1]) >= interval
            || offset > interval
            || latency != 0
            || !(10..=3200).contains(&timeout)
            || u32::from(timeout) * 4 <= u32::from(interval)
        {
            return Err(InvalidConnection::Timing);
        }
        Ok(Self {
            interval,
            offset,
            timeout,
            size: payload[1],
            instant: u16::from_le_bytes([payload[10], payload[11]]),
        })
    }
    /// Interval, latency and supervision timeout in standard controller units.
    #[must_use]
    pub const fn parameters(self) -> [u16; 3] {
        [self.interval, 0, self.timeout]
    }
    /// Connection event at which the staged values take effect.
    #[must_use]
    pub const fn instant(self) -> u16 {
        self.instant
    }
}
/// Explicit software follow-up, independent of ROM/shared SRAM state.
#[derive(Clone, Copy, Debug)]
pub struct PendingConnectionUpdate {
    update: ConnectionUpdate,
    setup: u8,
    setup_applied: bool,
}
impl PendingConnectionUpdate {
    /// Requested parameters, used only after observed hardware completion.
    #[must_use]
    pub const fn parameters(self) -> [u16; 3] {
        self.update.parameters()
    }
}
/// Distinguishes admission, timed setup and hardware instant completion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateProgress {
    /// Waiting for the required event.
    Pending,
    /// Setup was written after the preceding event ended.
    SetupApplied,
    /// Instant reached and hardware update-enable cleared.
    Complete,
}

/// Acknowledged status with an independently qualified timing token.
#[derive(Clone, Copy, Debug)]
pub struct ConnectionEvents {
    counter: u16,
    bits: u8,
    ended: Option<u16>,
    timing_ambiguity: u8,
}
impl ConnectionEvents {
    /// Latest stable counter sampled after the status acknowledgement.
    #[must_use]
    pub const fn counter(self) -> u16 {
        self.counter
    }
    /// Exactly the interrupt status bits acknowledged in this snapshot.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.bits
    }
    /// Qualified END identity; use `setup_counter` for END/SLEEP window setup.
    #[must_use]
    pub const fn ended_counter(self) -> Option<u16> {
        self.ended
    }
    /// Counter qualified for delayed update setup at END or SLEEP.
    ///
    /// Pinned ROM0x6334..6364 invokes setup via0x16c0 on END and0x16cc on
    /// SLEEP. SLEEP may be observed at instant-minus-one while END appears
    /// only at the next counter. Both require the same coherent ACK gates.
    #[must_use]
    pub const fn setup_counter(self) -> Option<u16> {
        if self.bits & 0x60 != 0 && self.timing_ambiguity == 0 {
            Some(self.counter)
        } else {
            None
        }
    }
    /// Timing ambiguity: bit0 service gap, bit1 ACK crossing, bit2 pre-ACK crossing.
    ///
    /// These withhold END/SLEEP timing tokens without invalidating acknowledged status.
    #[must_use]
    pub const fn timing_ambiguity(self) -> u8 {
        self.timing_ambiguity
    }
}

impl<R: ByteRegisters> Radio<R> {
    pub(crate) fn connection_counter(&mut self) -> Result<u16, RadioError<R::Error>> {
        for _ in 0..4 {
            let high = self.read(Register::new(Bank::Ll, 0x21))?;
            let low = self.read(Register::new(Bank::Ll, 0x20))?;
            if self.read(Register::new(Bank::Ll, 0x21))? == high {
                return Ok(u16::from_le_bytes([low, high]));
            }
        }
        Err(RadioError::StatusTimeout)
    }
    /// Samples status and acknowledges exactly once; qualifies END separately.
    ///
    /// A service gap above one event or counter advance during ACK withholds the
    /// END/SLEEP setup token. Ordinary traffic may continue; a pending update still
    /// fails if its required setup event is missed. The busy-loop fixture must
    /// service END/SLEEP continuously, as the ROM handler does.
    /// # Errors
    /// Rejects access errors or exhausted bounded coherent counter sampling.
    pub fn take_connection_events(
        &mut self,
        previous: Option<u16>,
    ) -> Result<ConnectionEvents, RadioError<R::Error>> {
        let mut pre_ack_crossed = false;
        for _ in 0..4 {
            let a = self.connection_counter()?;
            let bits = self.read(Register::new(Bank::Ll, 1))?;
            let b = self.connection_counter()?;
            if a != b {
                pre_ack_crossed = true;
                continue;
            }
            let gap = previous.is_some_and(|p| b.wrapping_sub(p) > 1);
            self.write(Register::new(Bank::Ll, 1), bits)?;
            let after = self.connection_counter()?;
            let timing_ambiguity =
                u8::from(gap) | (u8::from(after != b) << 1) | (u8::from(pre_ack_crossed) << 2);
            let ended = if bits & 32 != 0 && timing_ambiguity == 0 {
                Some(b)
            } else {
                None
            };
            return Ok(ConnectionEvents {
                counter: after,
                bits,
                ended,
                timing_ambiguity,
            });
        }
        Err(RadioError::StatusTimeout)
    }
    pub(crate) fn update_control(
        &mut self,
        own: u8,
        instant: u16,
    ) -> Result<(u16, u8), RadioError<R::Error>> {
        let current = self.connection_counter()?;
        if !(1..0x7fff).contains(&instant.wrapping_sub(current)) {
            return Err(RadioError::InvalidParameters);
        }
        let flags = self.read(Register::new(Bank::Ll, 0x22))?;
        if flags & own != 0 {
            return Err(RadioError::UpdateBusy);
        }
        let (other, offset) = if own == 1 { (2, 0x38) } else { (1, 0x30) };
        if flags & other != 0 {
            let low = self.read(Register::new(Bank::Ll, offset))?;
            let high = self.read(Register::new(Bank::Ll, offset + 1))?;
            let distance = u16::from_le_bytes([low, high]).wrapping_sub(current);
            if !(2..0x7fff).contains(&distance) {
                return Err(RadioError::UpdateBusy);
            }
        }
        Ok((current, flags))
    }
    pub(crate) fn arm_update(
        &mut self,
        own: u8,
        staged_counter: u16,
        instant: u16,
    ) -> Result<(), RadioError<R::Error>> {
        let (current, flags) = match self.update_control(own, instant) {
            Err(RadioError::UpdateBusy) => return Err(RadioError::StatusTimeout),
            other => other?,
        };
        if current != staged_counter || !(1..0x7fff).contains(&instant.wrapping_sub(current)) {
            return Err(RadioError::StatusTimeout);
        }
        self.write(Register::new(Bank::Ll, 0x22), flags | own)?;
        if self.connection_counter()? != current {
            return Err(RadioError::StatusTimeout);
        }
        Ok(())
    }
    /// Stages pinned-ROM offset-minus-one/timeout-plus-two update, with separate instant.
    /// # Errors
    /// Rejects invalid windows, stale instants and pending connection updates before writes.
    /// Adapter errors after staging require link reset; never retry a partial operation.
    pub fn schedule_connection_update(
        &mut self,
        update: ConnectionUpdate,
        old_interval: u16,
        central_ppm: u16,
        peripheral_ppm: u16,
    ) -> Result<PendingConnectionUpdate, RadioError<R::Error>> {
        let windows =
            ConnectionWindows::for_update(update, old_interval, central_ppm, peripheral_ppm)
                .map_err(|_| RadioError::InvalidParameters)?;
        let (current, _) = self.update_control(1, update.instant)?;
        let distance = update.instant.wrapping_sub(current);
        if distance == 0 || distance >= 0x7fff {
            return Err(RadioError::InvalidParameters);
        }
        for (offset, value) in [
            (0x2a, update.interval),
            (0x2c, 0),
            (0x2e, update.timeout + 2),
            (0x26, update.offset.saturating_sub(1)),
            (0x30, update.instant),
        ] {
            let [lo, hi] = value.to_le_bytes();
            self.write(Register::new(Bank::Ll, offset), lo)?;
            self.write(Register::new(Bank::Ll, offset + 1), hi)?;
        }
        self.write(Register::new(Bank::Ll, 0x28), update.size)?;
        // As in ROM: arm parameters, then refresh interval widening; setup is delayed.
        self.arm_update(1, current, update.instant)?;
        self.configure_widening(windows)?;
        Ok(PendingConnectionUpdate {
            update,
            setup: windows.setup_ticks(),
            setup_applied: false,
        })
    }
    /// Applies first-window setup on a qualified `instant-1` END/SLEEP boundary, then observes completion.
    /// # Errors
    /// A missed setup deadline, stuck enable or unstable counter requires reset.
    pub fn poll_connection_update(
        &mut self,
        pending: &mut PendingConnectionUpdate,
        setup_boundary: Option<u16>,
    ) -> Result<UpdateProgress, RadioError<R::Error>> {
        let current = self.connection_counter()?;
        let trigger = pending.update.instant.wrapping_sub(1);
        let since = current.wrapping_sub(trigger);
        if !pending.setup_applied {
            if since == 0 && setup_boundary == Some(trigger) {
                self.write(Register::new(Bank::Ll, 0x36), pending.setup)?;
                self.write(Register::new(Bank::Ll, 0x37), 0)?;
                if self.connection_counter()? != current {
                    return Err(RadioError::UpdateSetupCrossed);
                }
                pending.setup_applied = true;
                return Ok(UpdateProgress::SetupApplied);
            }
            if since != 0 && since < 0x8000 {
                return Err(RadioError::UpdateSetupTimeout {
                    counter: current,
                    instant: pending.update.instant,
                });
            }
            return Ok(UpdateProgress::Pending);
        }
        let since = current.wrapping_sub(pending.update.instant);
        if since < 0x8000 {
            if since > 4 {
                return Err(RadioError::UpdateCompletionTimeout);
            }
            if self.read(Register::new(Bank::Ll, 0x22))? & 1 == 0 {
                return Ok(UpdateProgress::Complete);
            }
        }
        Ok(UpdateProgress::Pending)
    }
}
