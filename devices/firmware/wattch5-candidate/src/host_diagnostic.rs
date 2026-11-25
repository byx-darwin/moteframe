//! Versioned numeric fatal diagnostics for the isolated experiment, without formatting.
use ble_controller::{ControllerError, HciError};
use ble_controller_syd881x::SydError;
use ble_host_trouble::host::{BleHostError, Error};
use core::convert::Infallible;
use syd881x_radio::{InitializationError, InitializationFailure, InitializationStage, RadioError};

/// Concrete error chain for this board composition. New boards map their own access errors.
pub type HostExitError<E = SydError<Infallible>> = BleHostError<HciError<E>>;
/// Resolves the native backend cause without discarding composition reset failures.
pub trait BackendDiagnostic {
    /// Returns the original SYD error or a numeric reset-gate reason.
    /// # Errors
    /// Returns the composition gate's stable rejection code.
    fn syd_error(&self) -> Result<&SydError<Infallible>, u8>;
}
impl BackendDiagnostic for SydError<Infallible> {
    fn syd_error(&self) -> Result<&SydError<Infallible>, u8> {
        Ok(self)
    }
}
/// Published mailbox tag; the writer commits it only after the complete payload.
pub const MAGIC: u32 = 0x4845_5832;

/// Exclusive mailbox publication boundary. The commit word is written atomically last.
pub trait ExitMailbox {
    /// Invalidate an older record before writing payload bytes.
    fn invalidate(&mut self);
    /// Write one byte at an offset in4..128 while the record is uncommitted.
    fn write_payload(&mut self, offset: u8, value: u8);
    /// Atomically publish the complete payload's tag.
    fn commit(&mut self, magic: u32);
}

#[derive(Default)]
struct Cause {
    path: [u8; 8],
    data: [u8; 16],
}
impl Cause {
    fn words(&mut self, words: &[u32]) {
        for (slot, word) in self.data.chunks_exact_mut(4).zip(words) {
            slot.copy_from_slice(&word.to_le_bytes());
        }
    }
    fn lengths(&mut self, a: usize, b: usize) {
        if u32::try_from(a).is_err() || u32::try_from(b).is_err() {
            self.path[7] |= 1;
        }
        self.words(&[
            u32::try_from(a).unwrap_or(u32::MAX),
            u32::try_from(b).unwrap_or(u32::MAX),
        ]);
    }
    fn radio(&mut self, error: RadioError<Infallible>) {
        self.path[6] = match error {
            RadioError::Access(value) => match value {},
            RadioError::RfTimeout => 2,
            RadioError::StatusTimeout => 3,
            RadioError::UpdateSetupTimeout { counter, instant } => {
                self.words(&[u32::from(counter), u32::from(instant)]);
                4
            }
            RadioError::UpdateSetupCrossed => 5,
            RadioError::UpdateCompletionTimeout => 6,
            RadioError::FifoFull => 7,
            RadioError::UpdateBusy => 8,
            RadioError::InvalidParameters => 9,
            RadioError::InvalidReceiveLength { length } => {
                self.words(&[u32::from(length)]);
                10
            }
        };
    }
    fn initialization(&mut self, error: &InitializationError<Infallible>) {
        self.path[4] = match error.stage {
            InitializationStage::Validate => 1,
            InitializationStage::Reset => 2,
            InitializationStage::PreCalibration => 3,
            InitializationStage::McuClock => 4,
            InitializationStage::Configuration => 5,
            InitializationStage::PostCalibration => 6,
            InitializationStage::LowFrequencyClock => 7,
            InitializationStage::PollingMode => 8,
        };
        self.path[5] = match &error.cause {
            InitializationFailure::InvalidHeader { mcu, lpo } => {
                self.words(&[u32::from(*mcu), u32::from(*lpo)]);
                1
            }
            InitializationFailure::Radio(value) => {
                self.radio(*value);
                2
            }
            InitializationFailure::ResetTimeout { last_status, polls } => {
                self.words(&[u32::from(*last_status), u32::from(*polls)]);
                3
            }
            InitializationFailure::PreCalibrationTimeout { last_status, polls } => {
                self.words(&[u32::from(*last_status), u32::from(*polls)]);
                4
            }
            InitializationFailure::PostCalibrationTimeout { last_status, polls } => {
                self.words(&[u32::from(*last_status), u32::from(*polls)]);
                5
            }
        };
    }
    fn host(&mut self, error: &Error) {
        self.path[0] = 2;
        self.path[1] = match error {
            Error::Hci(value) => {
                self.words(&[u32::from(u8::from(*value))]);
                1
            }
            Error::HciDecode(value) => {
                self.words(&[match value {
                    bt_hci::FromHciBytesError::InvalidSize => 1,
                    bt_hci::FromHciBytesError::InvalidValue => 2,
                }]);
                2
            }
            Error::Att(value) => {
                self.words(&[u32::from(value.to_u8())]);
                3
            }
            Error::InsufficientSpace => 4,
            Error::InvalidValue => 5,
            Error::UnexpectedDataLength { expected, actual } => {
                self.lengths(*expected, *actual);
                6
            }
            Error::CannotConstructGattValue(value) => {
                self.data = *value;
                7
            }
            Error::ConfigFilterAcceptListIsEmpty => 8,
            Error::UnexpectedGattResponse => 9,
            Error::MalformedCharacteristicDeclaration { expected, actual } => {
                self.lengths(*expected, *actual);
                10
            }
            Error::InvalidCharacteristicDeclarationData => 11,
            Error::FailedToFinalize { expected, actual } => {
                self.lengths(*expected, *actual);
                12
            }
            // The upstream codec module is private. Its public conversion maps
            // InsufficientSpace to that Host variant, and InvalidValue to CodecError.
            Error::CodecError(value) => {
                self.words(
                    &[if matches!(Error::from(*value), Error::InsufficientSpace) {
                        1
                    } else {
                        2
                    }],
                );
                13
            }
            Error::ExtendedAdvertisingNotSupported => 14,
            Error::InvalidUuidLength(value) => {
                self.lengths(*value, 0);
                15
            }
            Error::Advertisement(value) => {
                self.words(&[match value {
                    ble_host_trouble::host::advertise::AdvertisementDataError::TooLong => 1,
                }]);
                16
            }
            Error::L2capConnectError(value) => {
                self.words(&[u32::from(*value as u16)]);
                17
            }
            Error::InvalidChannelId => 18,
            Error::NoChannelAvailable => 19,
            Error::NotFound => 20,
            Error::InvalidState => 21,
            Error::OutOfMemory => 22,
            Error::NotSupported => 23,
            Error::ChannelClosed => 24,
            Error::Timeout => 25,
            Error::Busy => 26,
            Error::NoPermits => 27,
            Error::Disconnected => 28,
            Error::ConnectionLimitReached => 29,
            Error::GattSubscriberLimitReached => 30,
            Error::AlreadyInUse => 31,
            Error::Other => 32,
        };
    }
}

/// Fixed 128-byte little-endian record; no text, allocator, restart or error suppression.
pub struct ExitDiagnostic {
    bytes: [u8; 128],
}
impl core::fmt::Debug for ExitDiagnostic {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ExitDiagnostic").finish_non_exhaustive()
    }
}
impl ExitDiagnostic {
    /// Publishes a complete numeric record without moving or suppressing the result.
    pub fn publish<E: BackendDiagnostic>(
        result: &Result<(), HostExitError<E>>,
        mailbox: &mut impl ExitMailbox,
    ) {
        let diagnostic = Self::capture(result);
        mailbox.invalidate();
        for (offset, byte) in (4_u8..128).zip(&diagnostic.as_bytes()[4..]) {
            mailbox.write_payload(offset, *byte);
        }
        core::sync::atomic::compiler_fence(core::sync::atomic::Ordering::Release);
        mailbox.commit(MAGIC);
    }
    /// Captures every variant and payload in this composition's complete error chain.
    /// Initialization cleanup failures occupy separate cause slots.
    #[must_use]
    pub fn capture<E: BackendDiagnostic>(result: &Result<(), HostExitError<E>>) -> Self {
        let mut causes: [Cause; 3] = core::array::from_fn(|_| Cause::default());
        match result {
            Ok(()) => causes[0].path[0] = 3,
            Err(BleHostError::BleHost(error)) => causes[0].host(error),
            Err(BleHostError::Controller(error)) => {
                let primary = &mut causes[0];
                primary.path[0] = 1;
                primary.path[1] = match error {
                    HciError::InvalidPacket => 2,
                    HciError::Busy => 3,
                    HciError::Unsupported => 4,
                    HciError::Controller(error) => {
                        primary.path[2] = match error {
                            ControllerError::Faulted => 2,
                            ControllerError::State => 3,
                            ControllerError::Capacity => 4,
                            ControllerError::Protocol => 5,
                            ControllerError::Backend(error) => {
                                primary.path[3] = match error.syd_error() {
                                    Err(code) => {
                                        primary.words(&[u32::from(code)]);
                                        5
                                    }
                                    Ok(error) => match error {
                                        SydError::Protocol => 3,
                                        SydError::ProtocolAt { site } => {
                                            primary.words(&[u32::from(*site)]);
                                            3
                                        }
                                        SydError::Capacity => 4,
                                        SydError::Radio(error) => {
                                            primary.radio(*error);
                                            1
                                        }
                                        SydError::Initialization(error) => {
                                            primary.initialization(error);
                                            2
                                        }
                                    },
                                };
                                1
                            }
                        };
                        1
                    }
                };
                if let HciError::Controller(ControllerError::Backend(backend)) = error
                    && let Ok(SydError::Initialization(error)) = backend.syd_error()
                {
                    for (cause, error) in causes[1..]
                        .iter_mut()
                        .zip([&error.calibration_cleanup, &error.advertising_cleanup])
                    {
                        if let Some(error) = error {
                            cause.path[..4].copy_from_slice(&[1, 1, 1, 1]);
                            cause.radio(*error);
                        }
                    }
                }
            }
        }
        let mut bytes = [0; 128];
        bytes[..4].copy_from_slice(&MAGIC.to_le_bytes());
        bytes[4..8].copy_from_slice(&[1, 0, 128, 0]);
        bytes[8..12].copy_from_slice(&18_u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&u32::from(result.is_err()).to_le_bytes());
        for (slot, cause) in bytes[16..88].chunks_exact_mut(24).zip(causes) {
            slot[..8].copy_from_slice(&cause.path);
            slot[8..].copy_from_slice(&cause.data);
        }
        let checksum = bytes[4..124].iter().fold(2_166_136_261_u32, |hash, byte| {
            (hash ^ u32::from(*byte)).wrapping_mul(16_777_619)
        });
        bytes[124..].copy_from_slice(&checksum.to_le_bytes());
        Self { bytes }
    }
    /// Wire bytes, including commit tag; the MMIO writer must publish the tag last.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 128] {
        &self.bytes
    }
}
