//! Wattch5 wiring and compatibility facade for the existing SYD8810 revision.
//!
//! GPIO/SPI/PWM algorithms use typed registers; ARM MMIO is isolated in hardware.
//! GPIO numbering comes from the selected `MCU_SYD8810` block in `board.h`.
#![no_std]

use core::ops::Range;
use device_core::Dimensions;

/// Current display wiring; electrical polarity remains a board-driver concern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayPins {
    /// SPI MOSI.
    pub mosi: u8,
    /// SPI clock.
    pub clock: u8,
    /// Panel chip select.
    pub chip_select: u8,
    /// Command/data select.
    pub data_command: u8,
    /// Reset (new board revision).
    pub reset: u8,
    /// Backlight PWM (new board revision).
    pub backlight: u8,
}

/// Wiring from `FullResources_改复位引脚/Sources/configure/board.h`.
pub const DISPLAY_PINS: DisplayPins = DisplayPins {
    mosi: 14,
    clock: 19,
    chip_select: 10,
    data_command: 9,
    reset: 1,
    backlight: 6,
};
/// Active direct-display geometry from fb.h; the unrelated GUI config says 240.
pub const DISPLAY_DIMENSIONS: Dimensions = Dimensions::new(128, 128);
/// Flash range used by the original scatter file, end exclusive.
pub const APPLICATION_FLASH: Range<u32> = 0x1000_0000..0x1003_e000;
/// Application RAM range. Lower RAM belongs to ROM/shared BLE state.
pub const APPLICATION_RAM: Range<u32> = 0x2000_29a0..0x2000_8000;

pub mod drivers;
#[cfg(all(feature = "hardware", target_arch = "arm"))]
pub mod hardware;
pub mod registers;

/// Polling monotonic clock arithmetic.
pub mod clock;

/// Active original 128x128 panel initialization candidate.
pub mod panel;
/// Fail-closed writable-storage availability and offline allocation proposal.
pub mod storage;

/// Shared SPI receive wire from the selected board layout.
pub const SPI_MISO: u8 = 12;
/// Other shared-bus slaves deselected before display traffic.
pub const SHARED_SLAVE_SELECTS: [u8; 2] = [15, 16];
/// External resource-flash chip select.
pub const RESOURCE_FLASH_SELECT: u8 = 15;
/// Board-specific touch wiring; protocol parsing belongs to the touch driver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TouchPins {
    /// Reset output.
    pub reset: u8,
    /// Touch interrupt input.
    pub interrupt: u8,
    /// Open-drain clock.
    pub clock: u8,
    /// Open-drain data.
    pub data: u8,
    /// Side key input.
    pub key: u8,
}
/// Existing verified touch and key connections.
pub const TOUCH_PINS: TouchPins = TouchPins {
    reset: 23,
    interrupt: 22,
    clock: 20,
    data: 21,
    key: 25,
};
/// Existing controller address; does not identify its silicon model.
pub const TOUCH_ADDRESS: u8 = 0x15;
/// Four independent readback registers in their original order.
pub const TOUCH_REGISTERS: [u8; 4] = [1, 4, 6, 2];
