//! Compatibility owner applying wattch5 wiring to the chip HAL.
#![forbid(unsafe_code)]
use crate::registers::Registers;
use crate::{DISPLAY_PINS, SHARED_SLAVE_SELECTS, SPI_MISO};
use syd881x_hal::drivers::SpiPins;
pub use syd881x_hal::drivers::{
    Backlight, Error, Input, Output, PinId, Pull, Spi, SpiConfig, SpiSpeed,
};
/// Product owner retaining the original constructor and methods.
#[derive(Debug)]
pub struct Board<R> {
    inner: syd881x_hal::drivers::Board<R>,
}
impl<R: Registers> Board<R> {
    /// Owns the chip register backend with this product's wiring policy.
    #[must_use]
    pub const fn new(registers: R) -> Self {
        Self {
            inner: syd881x_hal::drivers::Board::new(registers),
        }
    }
    /// Borrows the same exclusive register owner.
    #[must_use]
    pub const fn registers(&self) -> &R {
        self.inner.registers()
    }
    /// Creates an exclusively owned output.
    /// # Errors
    /// Propagates chip pin-ownership errors.
    pub fn output(&self, pin: PinId, high: bool) -> Result<Output<'_, R>, Error> {
        self.inner.output(pin, high)
    }
    /// Creates an exclusively owned input.
    /// # Errors
    /// Propagates chip pin-ownership errors.
    pub fn input(&self, pin: PinId, pull: Pull) -> Result<Input<'_, R>, Error> {
        self.inner.input(pin, pull)
    }
    fn spi_pins(receive: bool) -> Result<SpiPins, Error> {
        SpiPins::new(
            PinId::new(DISPLAY_PINS.mosi)?,
            PinId::new(DISPLAY_PINS.clock)?,
            if receive {
                Some(PinId::new(SPI_MISO)?)
            } else {
                None
            },
        )
    }
    /// Configures write-only SPI using the actual display wires.
    /// # Errors
    /// Rejects invalid configuration or conflicting owners before writes.
    pub fn spi(&self, config: SpiConfig) -> Result<Spi<'_, R>, Error> {
        self.inner.spi(config, Self::spi_pins(false)?)
    }
    /// Configures full-duplex SPI using the board's shared MISO wire.
    /// # Errors
    /// Rejects invalid configuration or conflicting owners before writes.
    pub fn spi_full_duplex(&self, config: SpiConfig) -> Result<Spi<'_, R>, Error> {
        self.inner.spi(config, Self::spi_pins(true)?)
    }
    /// Starts the selected backlight PWM with the original period semantics.
    /// # Errors
    /// Rejects a zero period or conflicting pin owner.
    pub fn backlight(&self, period: u8) -> Result<Backlight<'_, R>, Error> {
        self.inner.pwm0(PinId::new(DISPLAY_PINS.backlight)?, period)
    }
    /// Acknowledges active GPIO sources without invoking user callbacks.
    #[must_use]
    pub fn gpio_interrupt(&self) -> u32 {
        self.inner.gpio_interrupt()
    }
    /// Reserves an initially released open-drain GPIO wire.
    /// # Errors
    /// Propagates duplicate pin ownership errors.
    pub fn open_drain(&self, pin: PinId) -> Result<i2c::OpenDrain<'_, R>, Error> {
        self.inner.open_drain(pin)
    }
}
impl<R: Registers> device_core::Board for Board<R> {
    type Error = Error;
    fn initialize(&mut self) -> Result<(), Error> {
        device_core::Board::initialize(&mut self.inner)?;
        for pin in SHARED_SLAVE_SELECTS {
            drop(self.output(PinId::new(pin)?, true)?);
        }
        Ok(())
    }
}
/// Legacy paths for the generic bitbang bus and HAL open-drain pins.
pub mod i2c {
    pub use syd881x_hal::drivers::i2c::{BitBangI2c, I2cError, Line, OpenDrain};
}
