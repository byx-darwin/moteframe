//! Complete ordered board profile contract recovered from the pinned original Flash.
use board_syd8810::panel::{DIMENSIONS, INITIALIZATION, OFFSET};
use device_display::InitStep;
#[test]
fn test_should_match_the_original_flash_command_data_delay_trace() {
    assert_eq!(
        (DIMENSIONS.width, DIMENSIONS.height, OFFSET),
        (128, 128, (2, 1))
    );
    let mut trace = Vec::<u32>::new();
    for step in INITIALIZATION {
        match step {
            InitStep::Command(command, data) => {
                trace.push(0x100 | u32::from(*command));
                trace.extend(data.iter().copied().map(u32::from));
            }
            InitStep::DelayMs(ms) => trace.push(0x10000 | u32::from(*ms)),
        }
    }
    assert_eq!(trace, REQUIRED);
}
const REQUIRED: &[u32] = &[
    510, 510, 495, 433, 192, 435, 3, 438, 1, 419, 17, 310, 216, 314, 5, 436, 33, 486, 56, 0, 487,
    96, 0, 496, 26, 61, 19, 78, 119, 45, 48, 0, 40, 12, 11, 21, 21, 15, 497, 26, 42, 36, 62, 105,
    45, 48, 0, 10, 13, 13, 20, 19, 15, 510, 511, 309, 0, 324, 0, 438, 1, 419, 11, 289, 296, 273,
    65686, 297, 65656,
];

#[test]
fn test_should_use_the_original_runtime_spi_mode_zero() {
    assert_eq!(
        (
            board_syd8810::panel::SPI_POLARITY,
            board_syd8810::panel::SPI_PHASE
        ),
        (false, false)
    );
}
