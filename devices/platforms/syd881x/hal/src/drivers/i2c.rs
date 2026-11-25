//! Bounded open-drain pointer reads. No controller payload writes or updater API.
use super::{Board, Error, Input, PinId, Pull};
use crate::registers::{Register32, Registers};
pub use device_bitbang_i2c::{BitBangI2c, I2cError, Line};
/// Exclusively owned GPIO input/output-low wire.
#[derive(Debug)]
pub struct OpenDrain<'a, R: Registers> {
    input: Input<'a, R>,
}
impl<R: Registers> Board<R> {
    /// Reserves a pulled-up, initially released GPIO for an open-drain protocol.
    /// # Errors
    /// Rejects a pin already owned by another driver.
    pub fn open_drain(&self, pin: PinId) -> Result<OpenDrain<'_, R>, Error> {
        let input = self.input(pin, Pull::Up)?;
        self.registers.update32(Register32::Output, pin.mask(), 0);
        Ok(OpenDrain { input })
    }
}
impl<R: Registers> Line for OpenDrain<'_, R> {
    fn low(&mut self) {
        self.input
            .board
            .registers
            .update32(Register32::Direction, self.input.pin.mask(), 0);
    }
    fn release(&mut self) {
        self.input
            .board
            .registers
            .update32(Register32::Direction, 0, self.input.pin.mask());
    }
    fn high(&self) -> bool {
        self.input.is_high()
    }
}
impl<R: Registers> Drop for OpenDrain<'_, R> {
    fn drop(&mut self) {
        self.release();
    }
}
