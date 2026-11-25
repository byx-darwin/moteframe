//! Typed register locations recovered from the selected SDK ARMCM0.h.
#![forbid(unsafe_code)]

use crate::drivers::PinId;

/// Byte registers; variants cannot construct arbitrary memory addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Register8 {
    /// Per-pin mux byte (six function bits).
    Mux(PinId),
    /// Pull-ups for pins 24..31.
    PullHigh,
    /// SPI speed/MSB/start/enable byte.
    SpiControl,
    /// SPI transmit byte.
    SpiOutput,
    /// SPI receive byte.
    SpiInput,
    /// SPI CPOL/CPHA byte.
    SpiMode,
    /// GPIO top-level interrupt mask.
    GpioTopMask,
    /// PWM0 denominator.
    PwmPeriod,
    /// PWM0 mode/pause/enable.
    PwmControl,
    /// PWM0 numerator.
    PwmDuty,
}
impl Register8 {
    /// Physical address from the SDK register map.
    #[must_use]
    pub const fn address(self) -> usize {
        match self {
            Self::Mux(pin) => 0x5000_1060 + pin.number() as usize,
            Self::PullHigh => 0x5000_1084,
            Self::SpiControl => 0x5001_0078,
            Self::SpiOutput => 0x5001_0079,
            Self::SpiInput => 0x5001_007a,
            Self::SpiMode => 0x5001_007b,
            Self::GpioTopMask => 0x4000_0030,
            Self::PwmPeriod => 0x5001_005d,
            Self::PwmControl => 0x5001_005e,
            Self::PwmDuty => 0x5001_005f,
        }
    }
}

/// Aligned word registers; lower ROM RAM is intentionally inaccessible here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Register32 {
    /// GPIO output latch.
    Output,
    /// GPIO input enable bits.
    InputEnable,
    /// GPIO input sample.
    Input,
    /// GPIO input polarity.
    Invert,
    /// GPIO direction: zero means output.
    Direction,
    /// Package pull-up bitmap for pins 0..23.
    PullLow,
    /// GPIO write-one interrupt acknowledgement.
    GpioClear,
    /// Per-pin GPIO mask (one means masked).
    GpioMask,
    /// GPIO pending status.
    GpioPending,
    /// NVIC interrupt enable register.
    NvicEnable,
    /// NVIC interrupt disable register.
    NvicDisable,
    /// NVIC clear-pending register.
    NvicClear,
}
impl Register32 {
    /// Physical address from the SDK/CMSIS register maps.
    #[must_use]
    pub const fn address(self) -> usize {
        match self {
            Self::Output => 0x5000_1014,
            Self::InputEnable => 0x5000_1018,
            Self::Input => 0x5000_101c,
            Self::Invert => 0x5000_1020,
            Self::Direction => 0x5000_1024,
            Self::PullLow => 0x5000_1080,
            Self::GpioClear => 0x4000_0010,
            Self::GpioMask => 0x4000_0020,
            Self::GpioPending => 0x4000_0028,
            Self::NvicEnable => 0xe000_e100,
            Self::NvicDisable => 0xe000_e180,
            Self::NvicClear => 0xe000_e280,
        }
    }
}

/// Register transport. Hardware implementations must serialize RMW against IRQs.
/// Host fixtures implement the same operations to exercise driver algorithms.
pub trait Registers {
    /// Reads one byte register.
    fn read8(&self, register: Register8) -> u8;
    /// Writes one byte register.
    fn write8(&self, register: Register8, value: u8);
    /// Reads one aligned word register.
    fn read32(&self, register: Register32) -> u32;
    /// Writes one aligned word register.
    fn write32(&self, register: Register32, value: u32);
    /// Atomically replaces only the selected byte bits relative to IRQ access.
    fn update8(&self, register: Register8, clear: u8, set: u8);
    /// Atomically replaces only the selected word bits relative to IRQ access.
    fn update32(&self, register: Register32, clear: u32, set: u32);
}
