//! Owned GPIO and polling SPI drivers; the same logic is tested on the host.
#![forbid(unsafe_code)]

use crate::registers::{Register8, Register32, Registers};
use core::cell::Cell;
use device_core::{OutputPin, SpiWrite};

/// Board validation, ownership or hardware progress failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Pin is outside 0..32.
    InvalidPin,
    /// A pin or controller already has an owner.
    ResourceBusy,
    /// SPI polling exceeded its explicit budget.
    Timeout,
    /// A timed-out SPI instance cannot safely continue.
    SpiFault,
    /// Zero polling budget or invalid PWM ratio.
    InvalidConfiguration,
}

/// Validated physical GPIO number.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PinId(u8);
impl PinId {
    /// Validates a physical pin number.
    /// # Errors
    /// Rejects numbers above 31.
    pub const fn new(number: u8) -> Result<Self, Error> {
        if number < 32 {
            Ok(Self(number))
        } else {
            Err(Error::InvalidPin)
        }
    }
    /// Physical pin number.
    #[must_use]
    pub const fn number(self) -> u8 {
        self.0
    }
    const fn mask(self) -> u32 {
        1u32 << self.0
    }
}

/// Package input pull configuration.
#[derive(Clone, Copy, Debug)]
pub enum Pull {
    /// Enable the mapped pull-up resistor.
    Up,
    /// Disable the pull-up; no pull-down is implied.
    Floating,
}

/// SDK SPI divider selectors. Actual frequency depends on the MCU clock.
#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum SpiSpeed {
    /// SDK selector 0, named 32M by the vendor.
    Mhz32 = 0,
    /// SDK selector 1.
    Mhz16 = 1,
    /// SDK selector 2.
    Mhz8 = 2,
    /// SDK selector 3 (actual rate requires hardware measurement).
    Slow = 3,
}

/// Synchronous SPI configuration, including a finite busy-poll budget per byte.
#[derive(Clone, Copy, Debug)]
pub struct SpiConfig {
    speed: SpiSpeed,
    polarity: bool,
    phase: bool,
    polls: u32,
}
impl SpiConfig {
    /// Creates configuration; zero poll budgets are rejected by `Board::spi`.
    #[must_use]
    pub const fn new(speed: SpiSpeed, polarity: bool, phase: bool, polls: u32) -> Self {
        Self {
            speed,
            polarity,
            phase,
            polls,
        }
    }
}

/// Explicit verified SPI wiring; no product pins are selected by the chip owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpiPins {
    mosi: PinId,
    clock: PinId,
    miso: Option<PinId>,
}
impl SpiPins {
    /// Validates that all selected wires have distinct owners.
    /// Callers supply pins whose chip mux capabilities and board wiring are verified.
    /// # Errors
    /// Rejects overlapping pins before any register access.
    pub fn new(mosi: PinId, clock: PinId, miso: Option<PinId>) -> Result<Self, Error> {
        if mosi == clock || miso.is_some_and(|p| p == mosi || p == clock) {
            return Err(Error::InvalidConfiguration);
        }
        Ok(Self { mosi, clock, miso })
    }
    fn mask(self) -> u32 {
        self.mosi.mask() | self.clock.mask() | self.miso.map_or(0, PinId::mask)
    }
}

/// Chip peripheral resource owner. Its Cell makes handles foreground-only, not Sync/Send.
#[derive(Debug)]
pub struct Board<R> {
    registers: R,
    claimed: Cell<u32>,
    controllers: Cell<u8>,
}
impl<R: Registers> Board<R> {
    /// Owns a register backend. Hardware construction uses the unsafe MMIO claim.
    #[must_use]
    pub const fn new(registers: R) -> Self {
        Self {
            registers,
            claimed: Cell::new(0),
            controllers: Cell::new(0),
        }
    }
    /// Register access for diagnostics; GPIO handles still share one ownership map.
    #[must_use]
    pub const fn registers(&self) -> &R {
        &self.registers
    }
    fn reserve(&self, mask: u32) -> Result<(), Error> {
        if self.claimed.get() & mask != 0 {
            return Err(Error::ResourceBusy);
        }
        self.claimed.set(self.claimed.get() | mask);
        Ok(())
    }
    fn release(&self, mask: u32) {
        self.claimed.set(self.claimed.get() & !mask);
    }
    fn reserve_controller(&self, controller: u8, pins: u32) -> Result<(), Error> {
        if self.controllers.get() & controller != 0 {
            return Err(Error::ResourceBusy);
        }
        self.reserve(pins)?;
        self.controllers.set(self.controllers.get() | controller);
        Ok(())
    }
    fn release_controller(&self, controller: u8, pins: u32) {
        self.release(pins);
        self.controllers.set(self.controllers.get() & !controller);
    }
    fn detach(&self, pin: PinId) {
        // Input direction is established before removing the peripheral mux,
        // avoiding an output-latch pulse while ownership is released.
        self.registers
            .update32(Register32::Direction, 0, pin.mask());
        self.registers
            .update32(Register32::InputEnable, 0, pin.mask());
        self.mux(pin, 0);
    }
    fn mux(&self, pin: PinId, function: u8) {
        self.registers.update8(Register8::Mux(pin), 0x3f, function);
    }
    /// Configures a GPIO output without a direction-before-latch glitch.
    /// # Errors
    /// Rejects an already-owned pin.
    pub fn output(&self, pin: PinId, high: bool) -> Result<Output<'_, R>, Error> {
        self.reserve(pin.mask())?;
        self.registers.update32(
            Register32::Output,
            pin.mask(),
            if high { pin.mask() } else { 0 },
        );
        self.mux(pin, 0);
        self.registers
            .update32(Register32::InputEnable, pin.mask(), 0);
        self.registers
            .update32(Register32::Direction, pin.mask(), 0);
        Ok(Output { board: self, pin })
    }
    /// Configures an input and package-specific pull-up bit.
    /// # Errors
    /// Rejects an already-owned pin.
    pub fn input(&self, pin: PinId, pull: Pull) -> Result<Input<'_, R>, Error> {
        self.reserve(pin.mask())?;
        self.mux(pin, 0);
        self.registers
            .update32(Register32::Direction, 0, pin.mask());
        self.registers.update32(Register32::Invert, pin.mask(), 0);
        if pin.0 < 24 {
            const MAP: [u8; 24] = [
                0, 1, 2, 3, 4, 5, 6, 9, 10, 7, 8, 17, 21, 18, 22, 23, 24, 19, 25, 20, 26, 27, 30,
                31,
            ];
            let mask = 1u32 << MAP[usize::from(pin.0)];
            self.registers.update32(
                Register32::PullLow,
                mask,
                if matches!(pull, Pull::Up) { mask } else { 0 },
            );
        } else {
            let mask = 1u8 << (pin.0 - 24);
            self.registers.update8(
                Register8::PullHigh,
                mask,
                if matches!(pull, Pull::Up) { mask } else { 0 },
            );
        }
        self.registers
            .update32(Register32::InputEnable, 0, pin.mask());
        Ok(Input { board: self, pin })
    }
    /// Reserves explicit verified SPI wires and configures the chip peripheral.
    /// # Errors
    /// Rejects conflicting owners or a zero polling budget before register writes.
    pub fn spi(&self, config: SpiConfig, pins: SpiPins) -> Result<Spi<'_, R>, Error> {
        let receive = pins.miso.is_some();
        if config.polls == 0 {
            return Err(Error::InvalidConfiguration);
        }
        self.reserve_controller(1, pins.mask())?;
        if let Some(miso) = pins.miso {
            self.registers
                .update32(Register32::Direction, 0, miso.mask());
            self.registers.update32(Register32::Invert, miso.mask(), 0);
            self.registers
                .update32(Register32::InputEnable, 0, miso.mask());
            self.mux(miso, 10);
        }
        self.mux(pins.mosi, 9);
        self.mux(pins.clock, 8);
        self.registers.write8(
            Register8::SpiMode,
            u8::from(config.polarity) | (u8::from(config.phase) << 1),
        );
        self.registers
            .write8(Register8::SpiControl, 0x88 | config.speed as u8);
        Ok(Spi {
            board: self,
            config,
            receive,
            pins,
            failed: false,
        })
    }
    /// Configures PWM0 on the caller-verified mux pin, initially off.
    /// # Errors
    /// Rejects a conflicting pin owner or a zero denominator.
    pub fn pwm0(&self, pin: PinId, period: u8) -> Result<Backlight<'_, R>, Error> {
        if period == 0 {
            return Err(Error::InvalidConfiguration);
        }
        self.reserve_controller(2, pin.mask())?;
        self.registers.update8(Register8::PwmControl, 0x80, 0);
        self.registers.write8(Register8::PwmPeriod, period);
        self.registers.write8(Register8::PwmDuty, 0);
        self.registers.update8(Register8::PwmControl, 0x30, 0x10);
        self.mux(pin, 15);
        Ok(Backlight {
            board: self,
            period,
            pin,
        })
    }
    /// Acknowledges only unmasked pending GPIO sources; returns their bitmap.
    #[must_use]
    pub fn gpio_interrupt(&self) -> u32 {
        let active = self.registers.read32(Register32::GpioPending)
            & !self.registers.read32(Register32::GpioMask);
        if active != 0 {
            self.registers.write32(Register32::GpioClear, active);
        }
        active
    }
}

/// Exclusively owned GPIO output.
#[derive(Debug)]
pub struct Output<'a, R: Registers> {
    board: &'a Board<R>,
    pin: PinId,
}
impl<R: Registers> OutputPin for Output<'_, R> {
    type Error = Error;
    fn set_level(&mut self, high: bool) -> Result<(), Error> {
        self.board.registers.update32(
            Register32::Output,
            self.pin.mask(),
            if high { self.pin.mask() } else { 0 },
        );
        Ok(())
    }
}
impl<R: Registers> Drop for Output<'_, R> {
    fn drop(&mut self) {
        self.board.release(self.pin.mask());
    }
}

/// Exclusively owned input. Interrupt polarity is explicit, not guessed from pull.
#[derive(Debug)]
pub struct Input<'a, R: Registers> {
    board: &'a Board<R>,
    pin: PinId,
}
impl<R: Registers> Input<'_, R> {
    /// Current logical input level.
    #[must_use]
    pub fn is_high(&self) -> bool {
        self.board.registers.read32(Register32::Input) & self.pin.mask() != 0
    }
    /// Configures only this pin's peripheral event latch and polarity.
    ///
    /// Clears stale pending state for this pin, unmasks its source and the GPIO
    /// top-level gate, and preserves all NVIC registers. For polling, the caller
    /// must keep GPIO NVIC disabled continuously and own foreground pending ACKs.
    /// This does not install an ISR or grant concurrent acknowledgement ownership.
    pub fn configure_polled_event(&mut self, inverted: bool) {
        let io = &self.board.registers;
        io.update32(
            Register32::Invert,
            self.pin.mask(),
            if inverted { self.pin.mask() } else { 0 },
        );
        io.write32(Register32::GpioClear, self.pin.mask());
        io.update32(Register32::GpioMask, self.pin.mask(), 0);
        io.update8(Register8::GpioTopMask, 1, 0);
    }
    /// Enables the vendor input event source, optionally inverted for falling detection.
    pub fn enable_interrupt(&mut self, inverted: bool) {
        self.configure_polled_event(inverted);
        self.board
            .registers
            .write32(Register32::NvicEnable, 1 << 12);
    }
    /// Masks this pin without affecting other GPIO owners.
    pub fn disable_interrupt(&mut self) {
        self.board
            .registers
            .update32(Register32::GpioMask, 0, self.pin.mask());
    }
}
impl<R: Registers> Drop for Input<'_, R> {
    fn drop(&mut self) {
        self.disable_interrupt();
        self.board.release(self.pin.mask());
    }
}

/// Owned SPI writer. Timeout poisons the handle and retains its pin reservation.
#[derive(Debug)]
pub struct Spi<'a, R: Registers> {
    board: &'a Board<R>,
    config: SpiConfig,
    receive: bool,
    pins: SpiPins,
    failed: bool,
}
impl<R: Registers> Spi<'_, R> {
    fn idle(&self) -> Result<(), Error> {
        for _ in 0..self.config.polls {
            if self.board.registers.read8(Register8::SpiControl) & 0x40 == 0 {
                return Ok(());
            }
        }
        Err(Error::Timeout)
    }
    fn transfer(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if bytes.is_empty() {
            return Ok(());
        }
        self.idle()?;
        for byte in bytes {
            self.board.registers.write8(Register8::SpiOutput, *byte);
            // START also writes DONE=0. The previous byte was already awaited.
            self.board
                .registers
                .write8(Register8::SpiControl, 0xc8 | self.config.speed as u8);
            self.idle()?;
        }
        // Exclusive SPI owner: preserve configured speed/MSB/enable and clear
        // DONE once at the synchronous call boundary, without a per-byte RMW.
        self.board
            .registers
            .write8(Register8::SpiControl, 0x88 | self.config.speed as u8);
        Ok(())
    }
}
impl<R: Registers> SpiWrite for Spi<'_, R> {
    type Error = Error;
    fn write(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if self.failed {
            return Err(Error::SpiFault);
        }
        let result = self.transfer(bytes);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
}
impl<R: Registers> Drop for Spi<'_, R> {
    fn drop(&mut self) {
        if !self.failed {
            self.board.detach(self.pins.mosi);
            self.board.detach(self.pins.clock);
            if let Some(miso) = self.pins.miso {
                self.board.detach(miso);
            }
            self.board.release_controller(1, self.pins.mask());
        }
    }
}

/// Caller-selected PWM0 output. Duty is a ratio, not a brightness calibration.
#[derive(Debug)]
pub struct Backlight<'a, R: Registers> {
    board: &'a Board<R>,
    period: u8,
    pin: PinId,
}
impl<R: Registers> Backlight<'_, R> {
    /// Sets numerator 0..=period, disables PWM at zero.
    /// # Errors
    /// Rejects duty beyond the denominator without changing hardware.
    pub fn set_duty(&mut self, duty: u8) -> Result<(), Error> {
        if duty > self.period {
            return Err(Error::InvalidConfiguration);
        }
        self.board.registers.write8(Register8::PwmDuty, duty);
        self.board.registers.update8(
            Register8::PwmControl,
            0xc0,
            if duty == 0 { 0 } else { 0xc0 },
        );
        Ok(())
    }
}
impl<R: Registers> Drop for Backlight<'_, R> {
    fn drop(&mut self) {
        self.board.registers.update8(Register8::PwmControl, 0x80, 0);
        self.board.detach(self.pin);
        self.board.release_controller(2, self.pin.mask());
    }
}

impl<R: Registers> device_core::Board for Board<R> {
    type Error = Error;
    /// Initializes board GPIO after vendor clock setup; does not own BLE/ROM IRQs.
    fn initialize(&mut self) -> Result<(), Error> {
        if self.claimed.get() != 0 || self.controllers.get() != 0 {
            return Err(Error::ResourceBusy);
        }
        self.registers.write32(Register32::NvicDisable, 1 << 12);
        self.registers.write32(Register32::NvicClear, 1 << 12);
        self.registers.write32(Register32::GpioMask, u32::MAX);
        self.registers.update8(Register8::GpioTopMask, 0, 1);
        self.registers.write32(Register32::GpioClear, u32::MAX);
        Ok(())
    }
}

/// Open-drain software I2C transport for read-only peripheral bringup.
pub mod i2c;

impl embedded_hal::spi::Error for Error {
    fn kind(&self) -> embedded_hal::spi::ErrorKind {
        embedded_hal::spi::ErrorKind::Other
    }
}
impl<R: Registers> embedded_hal::spi::ErrorType for Spi<'_, R> {
    type Error = Error;
}
impl<R: Registers> Spi<'_, R> {
    fn receive_guard(&self, count: usize) -> Result<(), Error> {
        if self.failed {
            return Err(Error::SpiFault);
        }
        if count > 0 && !self.receive {
            return Err(Error::InvalidConfiguration);
        }
        Ok(())
    }
    fn exchange(&mut self, value: u8) -> Result<u8, Error> {
        self.board.registers.write8(Register8::SpiOutput, value);
        self.board
            .registers
            .write8(Register8::SpiControl, 0xc8 | self.config.speed as u8);
        self.idle()?;
        Ok(self.board.registers.read8(Register8::SpiInput))
    }
    fn exchange_done(&mut self, result: Result<(), Error>) -> Result<(), Error> {
        if result.is_err() {
            self.failed = true;
        } else {
            self.board
                .registers
                .write8(Register8::SpiControl, 0x88 | self.config.speed as u8);
        }
        result
    }
}
impl<R: Registers> embedded_hal::spi::SpiBus for Spi<'_, R> {
    fn read(&mut self, out: &mut [u8]) -> Result<(), Error> {
        embedded_hal::spi::SpiBus::transfer(self, out, &[])
    }
    fn write(&mut self, data: &[u8]) -> Result<(), Error> {
        SpiWrite::write(self, data)
    }
    fn transfer(&mut self, out: &mut [u8], data: &[u8]) -> Result<(), Error> {
        self.receive_guard(out.len())?;
        if out.is_empty() {
            return SpiWrite::write(self, data);
        }
        let result = (|| {
            self.idle()?;
            for i in 0..out.len().max(data.len()) {
                let value = self.exchange(data.get(i).copied().unwrap_or(0xff))?;
                if let Some(byte) = out.get_mut(i) {
                    *byte = value;
                }
            }
            Ok(())
        })();
        self.exchange_done(result)
    }
    fn transfer_in_place(&mut self, data: &mut [u8]) -> Result<(), Error> {
        self.receive_guard(data.len())?;
        if data.is_empty() {
            return Ok(());
        }
        let result = (|| {
            self.idle()?;
            for byte in data {
                *byte = self.exchange(*byte)?;
            }
            Ok(())
        })();
        self.exchange_done(result)
    }
    fn flush(&mut self) -> Result<(), Error> {
        self.receive_guard(0)?;
        let result = self.idle();
        self.exchange_done(result)
    }
}
