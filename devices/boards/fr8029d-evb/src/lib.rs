//! Wiring and analog configuration for the FR8029D DEV0.2 evaluation board.
#![no_std]

#[cfg(target_arch = "arm")]
pub use fr8029d_chip::gpio;
pub use fr8029d_chip::{saradc, touchkey};

/// MIC1 is AC-coupled through C45/C46 to `MIC_INP`/`MIC_INN` on pins 46/47.
/// R71 supplies `MIC_BIAS` from pin 60. PC14/PC15 are the separate ASP input.
pub const MIC1: saradc::MicrophoneConfig = saradc::MicrophoneConfig {
    input: saradc::Input::DedicatedDifferential,
    bias: saradc::Bias::Volts0_8,
};

/// DEV0.2 schematic labels TK2 and TK6 wired through R56 and R58.
pub const TOUCH_KEYS: [touchkey::Electrode; 2] =
    [touchkey::Electrode::Pc2, touchkey::Electrode::Pc3];

/// Port A output connected to the red user LED D7 via R52.
pub const LED_PIN: u8 = 5;

/// D7 is wired from PA5 through R52 to ground.
pub const LED_ACTIVE_HIGH: bool = true;
