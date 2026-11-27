//! Wiring and analog configuration for the FR8029D DEV0.2 evaluation board.
#![no_std]

pub use fr8029d_chip::saradc;

/// MIC1 is AC-coupled through C45/C46 to `MIC_INP`/`MIC_INN` on pins 46/47.
/// R71 supplies `MIC_BIAS` from pin 60. PC14/PC15 are the separate ASP input.
pub const MIC1: saradc::MicrophoneConfig = saradc::MicrophoneConfig {
    input: saradc::Input::DedicatedDifferential,
    bias: saradc::Bias::Volts0_8,
};
