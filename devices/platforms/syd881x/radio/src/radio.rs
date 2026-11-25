//! Ordered algorithms recovered from the pinned ROM; no ROM is executed.
use crate::{Bank, ByteRegisters, Register};

/// Bounded algorithm failure. A successful operation does not prove radio emission.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RadioError<E> {
    /// Register adapter failed; partial writes may already have occurred.
    Access(E),
    /// RF bridge bit zero did not become one within the read budget.
    RfTimeout,
    /// No requested raw interrupt bit was observed within the snapshot budget.
    StatusTimeout,
    /// The required instant-minus-one END token was not observed in time.
    UpdateSetupTimeout {
        /// Observed event counter when the deadline was missed.
        counter: u16,
        /// Requested parameter update instant.
        instant: u16,
    },
    /// The event counter advanced while first-window setup was being written.
    UpdateSetupCrossed,
    /// Hardware completion was not observed within the bounded instant window.
    UpdateCompletionTimeout,
    /// FIFO was full; no FIFO bytes or commit were written.
    FifoFull,
    /// Another update is pending or too close to its instant for safe flag RMW.
    UpdateBusy,
    /// A zero budget or invalid advertising parameter was supplied.
    InvalidParameters,
    /// RX hardware declared a payload length outside the ROM's bounded format.
    InvalidReceiveLength {
        /// Declared payload bytes, after separating hardware metadata flags.
        length: u8,
    },
}

/// Serialized owner of a chip register adapter and a finite RF poll budget.
#[derive(Debug)]
pub struct Radio<R> {
    registers: R,
    rf_poll_limit: u16,
}
impl<R: ByteRegisters> Radio<R> {
    /// Creates a bounded radio owner. Budget counts completion reads per RF call.
    /// # Errors
    /// Returns `InvalidParameters` for a zero budget, without accessing registers.
    pub fn new(registers: R, rf_poll_limit: u16) -> Result<Self, RadioError<R::Error>> {
        if rf_poll_limit == 0 {
            return Err(RadioError::InvalidParameters);
        }
        Ok(Self {
            registers,
            rf_poll_limit,
        })
    }
    /// Returns the adapter, including any observed partial state after an error.
    #[must_use]
    pub fn into_inner(self) -> R {
        self.registers
    }
    /// Disables AD/scan/DTM and requests ROM-supported LL state reset `SR_P`.
    ///
    /// Unlike clearing only `ADV_EN`, resetting the LL state machine also ends
    /// connection processing. Caller still performs the experiment's full-chip
    /// recovery; this gate only observes bit1 self-clear within4096 reads.
    /// # Errors
    /// Returns adapter failures or `StatusTimeout` if reset stays asserted.
    pub fn stop_link(&mut self) -> Result<u16, RadioError<R::Error>> {
        for offset in [0x10, 0x60, 0x40] {
            let register = Register::new(Bank::Ll, offset);
            let previous = self.read(register)?;
            self.write(register, previous & !1)?;
        }
        let reset = Register::new(Bank::Ll, 0);
        let previous = self.read(reset)?;
        self.write(reset, previous | 2)?;
        for polls in 1..=4096 {
            if self.read(reset)? & 2 == 0 {
                return Ok(polls);
            }
        }
        Err(RadioError::StatusTimeout)
    }
    /// Reproduces ROM `0x61e4` RF bridge writes with a completion read budget.
    /// # Errors
    /// Returns adapter errors or `RfTimeout`; never retries the write implicitly.
    pub fn rf_write(&mut self, address: u8, value: u8) -> Result<(), RadioError<R::Error>> {
        self.begin_rf(address)?;
        self.write(Register::new(Bank::Misc, 0x19), value)?;
        self.wait_rf()
    }
    /// Reproduces ROM `0x61c4` RF bridge reads, including the initial discarded read.
    /// # Errors
    /// Returns adapter errors or `RfTimeout` without returning stale initial data.
    pub fn rf_read(&mut self, address: u8) -> Result<u8, RadioError<R::Error>> {
        self.begin_rf(address)?;
        let _initial_sample = self.read(Register::new(Bank::Misc, 0x1c))?;
        self.wait_rf()?;
        self.read(Register::new(Bank::Misc, 0x1c))
    }
    fn begin_rf(&mut self, address: u8) -> Result<(), RadioError<R::Error>> {
        let control = Register::new(Bank::Misc, 0x1a);
        let value = self.read(control)?;
        self.write(control, value | 1)?;
        self.write(Register::new(Bank::Misc, 0x18), address)
    }
    fn wait_rf(&mut self) -> Result<(), RadioError<R::Error>> {
        for _ in 0..self.rf_poll_limit {
            if self.read(Register::new(Bank::Misc, 0x1a))? & 1 != 0 {
                return Ok(());
            }
        }
        Err(RadioError::RfTimeout)
    }
    pub(crate) fn read(&mut self, register: Register) -> Result<u8, RadioError<R::Error>> {
        self.registers.read8(register).map_err(RadioError::Access)
    }
    pub(crate) fn write(
        &mut self,
        register: Register,
        value: u8,
    ) -> Result<(), RadioError<R::Error>> {
        self.registers
            .write8(register, value)
            .map_err(RadioError::Access)
    }
}
