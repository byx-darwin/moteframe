//! Panel profile recovered from this board's pinned original Flash.
//! Initialization at 0x10012624 and wake at 0x1001288a differ from the active
//! SDK source branch. This profile is not a panel silicon ID reading.
//! Reset is handled before this sequence; backlight follows the first image.
use crate::InitStep;
use device_core::{DelayMs, Dimensions};
/// Visible framebuffer geometry.
pub const DIMENSIONS: Dimensions = Dimensions::new(128, 128);
/// GRAM origin for the original board's alternate panel profile.
pub const OFFSET: (u16, u16) = (2, 1);
/// SPI idle polarity observed in the pinned original runtime.
pub const SPI_POLARITY: bool = false;
/// SPI phase observed in the pinned original runtime.
pub const SPI_PHASE: bool = false;
/// Original Flash's post-reset commands followed by its display-on wake path.
/// Unlike the SDK source, the original machine code has no40ms delay before28.
pub const INITIALIZATION: &[InitStep<'static>] = &[
    InitStep::Command(0xfe, &[]),
    InitStep::Command(0xfe, &[]),
    InitStep::Command(0xef, &[]),
    InitStep::Command(0xb1, &[0xc0]),
    InitStep::Command(0xb3, &[0x03]),
    InitStep::Command(0xb6, &[0x01]),
    InitStep::Command(0xa3, &[0x11]),
    InitStep::Command(0x36, &[0xd8]),
    InitStep::Command(0x3a, &[0x05]),
    InitStep::Command(0xb4, &[0x21]),
    InitStep::Command(0xe6, &[0x38, 0x00]),
    InitStep::Command(0xe7, &[0x60, 0x00]),
    InitStep::Command(
        0xf0,
        &[
            0x1a, 0x3d, 0x13, 0x4e, 0x77, 0x2d, 0x30, 0x00, 0x28, 0x0c, 0x0b, 0x15, 0x15, 0x0f,
        ],
    ),
    InitStep::Command(
        0xf1,
        &[
            0x1a, 0x2a, 0x24, 0x3e, 0x69, 0x2d, 0x30, 0x00, 0x0a, 0x0d, 0x0d, 0x14, 0x13, 0x0f,
        ],
    ),
    InitStep::Command(0xfe, &[]),
    InitStep::Command(0xff, &[]),
    InitStep::Command(0x35, &[0x00]),
    InitStep::Command(0x44, &[0x00]),
    InitStep::Command(0xb6, &[0x01]),
    InitStep::Command(0xa3, &[0x0b]),
    InitStep::Command(0x21, &[]),
    InitStep::Command(0x28, &[]),
    InitStep::Command(0x11, &[]),
    InitStep::DelayMs(150),
    InitStep::Command(0x29, &[]),
    InitStep::DelayMs(120),
];

/// Drives the existing reset pulse sequence high50ms / low50ms / high120ms.
/// # Errors
/// Propagates the board GPIO failure.
pub fn reset<P: device_core::OutputPin, D: DelayMs>(
    pin: &mut P,
    delay: &mut D,
) -> Result<(), P::Error> {
    pin.set_level(true)?;
    delay.delay_ms(50);
    pin.set_level(false)?;
    delay.delay_ms(50);
    pin.set_level(true)?;
    delay.delay_ms(120);
    Ok(())
}
