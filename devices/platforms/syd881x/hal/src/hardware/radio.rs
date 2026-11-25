//! Exclusive native-radio MMIO; no vendor BLE or ROM entry is called.
use super::{AlreadyTaken, Mmio, critical};
use core::{
    convert::Infallible,
    ptr::{read_volatile, write_volatile},
};
use syd881x_radio::{Bank, ByteRegisters, Register};
static mut RADIO_CLAIMED: bool = false;
/// Native radio register token; radio clock/configuration changes remain serialized.
#[derive(Debug)]
pub struct RadioMmio {
    _private: (),
}
impl Mmio {
    /// Claims the radio register banks once; keep this token inside one Controller.
    /// Initialize radio clocks/configuration before display pins and buses. Callers
    /// serialize later resets with display activity; shared clock banks are not
    /// independently owned or safe to reconfigure from an ISR.
    /// # Errors
    /// Rejects duplicate ownership, including after the first radio token is dropped.
    pub fn radio(&mut self) -> Result<RadioMmio, AlreadyTaken> {
        critical(|| {
            let claimed = &raw mut RADIO_CLAIMED;
            // SAFETY: The singleton flag is read/written only with PRIMASK held.
            unsafe {
                if read_volatile(claimed) {
                    return Err(AlreadyTaken);
                }
                write_volatile(claimed, true);
            }
            Ok(RadioMmio { _private: () })
        })
    }
}
fn address(r: Register) -> usize {
    let base = match r.bank() {
        Bank::Ll => 0x4002_0000,
        Bank::Modem => 0x4003_0000,
        Bank::Misc => 0x4000_0000,
        Bank::PowerOn => 0x5000_1000,
        Bank::Interface => 0x5001_0000,
    };
    base + usize::from(r.offset())
}
impl ByteRegisters for RadioMmio {
    type Error = Infallible;
    fn read8(&mut self, r: Register) -> Result<u8, Infallible> {
        // SAFETY: Exclusive radio token; restricted bank/byte-offset addresses.
        Ok(unsafe { read_volatile(address(r) as *const u8) })
    }
    fn write8(&mut self, r: Register, value: u8) -> Result<(), Infallible> {
        // SAFETY: Same exclusive token; each access occurs once at byte width.
        unsafe {
            write_volatile(address(r) as *mut u8, value);
        }
        Ok(())
    }
}

/// Stateless register transport for an exclusive reset/debug experiment.
/// Unlike the singleton runtime token it does not depend on initialized claim flags.
#[derive(Debug)]
pub struct RawRadioMmio(RadioMmio);
impl RawRadioMmio {
    /// Creates the transport without touching memory or peripheral registers.
    /// # Safety
    /// Caller exclusively controls the chip experiment, masks competing IRQs and
    /// serializes every access with all other register owners for its lifetime.
    /// This does not initialize clocks, calibrate RF or establish a boot contract.
    #[must_use]
    pub unsafe fn new_unchecked() -> Self {
        Self(RadioMmio { _private: () })
    }
}
impl ByteRegisters for RawRadioMmio {
    type Error = Infallible;
    fn read8(&mut self, register: Register) -> Result<u8, Infallible> {
        self.0.read8(register)
    }
    fn write8(&mut self, register: Register, value: u8) -> Result<(), Infallible> {
        self.0.write8(register, value)
    }
}
