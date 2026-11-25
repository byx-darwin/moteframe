//! Bounded hardware initialization subset recovered from the pinned ROM.
use crate::{Bank, ByteRegisters, ConfigurationTable, Radio, RadioError, Register};
use core::num::NonZeroU16;

/// Hardware phase containing an initialization failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitializationStage {
    /// Header validation before all hardware accesses.
    Validate,
    /// LL reset and the probe's self-clear gate.
    Reset,
    /// Bank-zero RF calibration and bit-six cleanup.
    PreCalibration,
    /// Deterministic MCU oscillator selection.
    McuClock,
    /// Ordered physical-flash register records.
    Configuration,
    /// Bank-one RF calibration.
    PostCalibration,
    /// Low-frequency clock selection.
    LowFrequencyClock,
    /// Disable automatic sleep and manual RF mode.
    PollingMode,
}

/// The primary reason initialization could not meet a finite gate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitializationFailure<E> {
    /// Unsupported header clock fields, rejected before hardware access.
    InvalidHeader {
        /// MCU clock field.
        mcu: u8,
        /// Low-frequency clock field.
        lpo: u8,
    },
    /// Register transport or finite bridge transaction failure.
    Radio(RadioError<E>),
    /// LL reset bit stayed set across all 4096 probe reads.
    ResetTimeout {
        /// Final observed register byte.
        last_status: u8,
        /// Number of completion reads.
        polls: u16,
    },
    /// RF bank-zero calibration bit one stayed clear across 256 reads.
    PreCalibrationTimeout {
        /// Final completed RF sample.
        last_status: u8,
        /// Number of complete RF read transactions.
        polls: u16,
    },
    /// RF bank-one calibration bits two/three never equalled one across 256 reads.
    PostCalibrationTimeout {
        /// Final completed RF sample.
        last_status: u8,
        /// Number of complete RF read transactions.
        polls: u16,
    },
}

/// Primary initialization failure and separately retained bounded cleanup failures.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InitializationError<E> {
    /// Phase in which the primary failure happened.
    pub stage: InitializationStage,
    /// Primary failure; cleanup never overwrites it.
    pub cause: InitializationFailure<E>,
    /// Failure clearing bank-zero `RF[0x3d]` bit six, if cleanup was attempted.
    pub calibration_cleanup: Option<RadioError<E>>,
    /// Failure clearing `ADV_EN` during failure cleanup, if attempted.
    pub advertising_cleanup: Option<RadioError<E>>,
}

/// Completed software gates, without a cold-start or air-transmission claim.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InitializationReport {
    /// Reads until the probe's LL reset self-clear observation.
    pub reset_polls: u16,
    /// Completed RF reads until bank-zero bit-one calibration readiness.
    pub pre_calibration_polls: u16,
    /// Completed RF reads until bank-one calibration readiness.
    pub post_calibration_polls: u16,
}

impl<R: ByteRegisters> Radio<R> {
    /// Leaves inherited sleep before any RF bridge access.
    ///
    /// Disables automatic sleep, then follows ROM `0x4712` / `0x6a46`:
    /// request standby reset, settle 100us, reset LL, settle 500us, select
    /// manual standby. `settle_us` must provide at least the requested delay;
    /// the caller masks IRQs. This does not establish an analog cold boundary.
    /// [`Self::initialize`] subsequently releases manual mode for polling TX.
    /// # Errors
    /// Returns register transport failures; no RF transaction is attempted.
    pub fn prepare_standby(
        &mut self,
        mut settle_us: impl FnMut(u32),
    ) -> Result<(), RadioError<R::Error>> {
        for (register, mask) in [
            (Register::new(Bank::Ll, 0x1a), 7),
            (Register::new(Bank::PowerOn, 0x0a), 1),
        ] {
            let previous = self.read(register)?;
            self.write(register, previous & !mask)?;
        }
        let power = Register::new(Bank::PowerOn, 8);
        let previous = self.read(power)?;
        self.write(power, previous | 0x80)?;
        settle_us(100);
        let reset = Register::new(Bank::Ll, 0);
        let previous = self.read(reset)?;
        self.write(reset, previous | 1)?;
        settle_us(500);
        let previous = self.read(power)?;
        self.write(power, (previous & !0x1f) | 4)?;
        let previous = self.read(power)?;
        self.write(power, previous | 0x20)
    }

    /// Initializes the documented ROM hardware subset using this board's table.
    ///
    /// Includes LL reset, both bounded RF calibrations, deterministic clock
    /// selection, ordered records, and polling mode. The caller must mask IRQs
    /// and provide stable MMIO access. Analog RF state is not reset by LL `SR_A`;
    /// a probe injected into a running application remains a warm-state probe.
    /// # Errors
    /// Rejects unsupported header clocks before MMIO. Reports phase-specific
    /// reset/calibration/bridge failures, with secondary cleanup errors separate.
    pub fn initialize(
        &mut self,
        table: &ConfigurationTable<'_>,
    ) -> Result<InitializationReport, InitializationError<R::Error>> {
        self.initialize_with_calibration_budget(
            table,
            NonZeroU16::new(256).unwrap_or(NonZeroU16::MIN),
        )
    }

    /// Uses a caller-selected finite RF calibration-read budget per phase.
    ///
    /// This counts completed bridge transactions, not elapsed microseconds. A
    /// native Flash start may need more reads than a debugger-delayed warm start.
    /// Increasing it does not repair a missing clock or power configuration.
    /// # Errors
    /// Same initialization/cleanup failures as [`Self::initialize`], retaining
    /// the last RF sample and the actual supplied poll bound on timeout.
    pub fn initialize_with_calibration_budget(
        &mut self,
        table: &ConfigurationTable<'_>,
        calibration_budget: NonZeroU16,
    ) -> Result<InitializationReport, InitializationError<R::Error>> {
        let clock = table.header()[1];
        let mcu = clock & 0x0f;
        let lpo = (clock >> 4) & 7;
        if mcu > 8 || lpo > 2 {
            return Err(InitializationError::new(
                InitializationStage::Validate,
                InitializationFailure::InvalidHeader { mcu, lpo },
            ));
        }
        match self.run_initialization(table, mcu, lpo, calibration_budget.get()) {
            Ok(report) => Ok(report),
            Err(mut error) => {
                if error.stage != InitializationStage::Reset {
                    error.calibration_cleanup = self.clear_pre_calibration().err();
                }
                error.advertising_cleanup = self.disable_advertising().err();
                Err(error)
            }
        }
    }

    fn run_initialization(
        &mut self,
        table: &ConfigurationTable<'_>,
        mcu: u8,
        lpo: u8,
        calibration_budget: u16,
    ) -> Result<InitializationReport, InitializationError<R::Error>> {
        use InitializationStage as Stage;
        let reset_polls = self
            .reset_link_layer()
            .map_err(|cause| InitializationError::new(Stage::Reset, cause))?;
        let pre_calibration_polls = self
            .pre_calibrate(calibration_budget)
            .map_err(|cause| InitializationError::new(Stage::PreCalibration, cause))?;
        self.clear_pre_calibration()
            .map_err(|error| InitializationError::radio(Stage::PreCalibration, error))?;
        self.select_mcu_clock(mcu)
            .map_err(|error| InitializationError::radio(Stage::McuClock, error))?;
        self.apply_configuration(table)
            .map_err(|error| InitializationError::radio(Stage::Configuration, error))?;
        let post_calibration_polls = self
            .post_calibrate(calibration_budget)
            .map_err(|cause| InitializationError::new(Stage::PostCalibration, cause))?;
        self.select_low_frequency_clock(lpo)
            .map_err(|error| InitializationError::radio(Stage::LowFrequencyClock, error))?;
        self.configure_polling_mode()
            .map_err(|error| InitializationError::radio(Stage::PollingMode, error))?;
        Ok(InitializationReport {
            reset_polls,
            pre_calibration_polls,
            post_calibration_polls,
        })
    }

    fn reset_link_layer(&mut self) -> Result<u16, InitializationFailure<R::Error>> {
        let control = Register::new(Bank::Ll, 0);
        let previous = self.read(control)?;
        self.write(control, previous | 1)?;
        let mut last_status = previous | 1;
        for polls in 1..=4096 {
            last_status = self.read(control)?;
            if last_status & 1 == 0 {
                return Ok(polls);
            }
        }
        Err(InitializationFailure::ResetTimeout {
            last_status,
            polls: 4096,
        })
    }

    fn pre_calibrate(&mut self, budget: u16) -> Result<u16, InitializationFailure<R::Error>> {
        self.rf_write(0x7f, 0)?;
        let control = self.rf_read(0x3d)?;
        self.rf_write(0x3d, control | 0x40)?;
        self.rf_write(0x7f, 0)?;
        self.rf_write(0x3b, 1)?;
        let mut last_status = 0;
        for polls in 1..=budget {
            last_status = self.rf_read(0x3b)?;
            if last_status & 2 != 0 {
                return Ok(polls);
            }
        }
        Err(InitializationFailure::PreCalibrationTimeout {
            last_status,
            polls: budget,
        })
    }

    fn clear_pre_calibration(&mut self) -> Result<(), RadioError<R::Error>> {
        self.rf_write(0x7f, 0)?;
        let control = self.rf_read(0x3d)?;
        self.rf_write(0x3d, control & !0x40)
    }

    fn select_mcu_clock(&mut self, desired: u8) -> Result<(), RadioError<R::Error>> {
        self.rf_write(0x7f, 0)?;
        if desired == 8 {
            self.rf_write(0x40, 1)
        } else {
            self.rf_write(0x40, 0)?;
            let clock = Register::new(Bank::PowerOn, 0);
            let previous = self.read(clock)?;
            self.write(clock, (previous & !7) | desired)
        }
    }

    fn post_calibrate(&mut self, budget: u16) -> Result<u16, InitializationFailure<R::Error>> {
        self.rf_write(0x7f, 1)?;
        self.rf_write(0x0a, 2)?;
        let mut last_status = 0;
        for polls in 1..=budget {
            last_status = self.rf_read(0x0a)?;
            if (last_status >> 2) & 3 == 1 {
                return Ok(polls);
            }
        }
        Err(InitializationFailure::PostCalibrationTimeout {
            last_status,
            polls: budget,
        })
    }

    fn select_low_frequency_clock(&mut self, desired: u8) -> Result<(), RadioError<R::Error>> {
        self.rf_write(0x7f, 2)?;
        let previous = self.rf_read(0)?;
        self.rf_write(0, (previous & !3) | desired)?;
        self.rf_write(0x7f, 0)?;
        let (rf3, rf4) = match desired {
            0 => (0, 0),
            1 => (15, 0),
            _ => (0, 15),
        };
        self.rf_write(3, rf3)?;
        self.rf_write(4, rf4)
    }

    fn configure_polling_mode(&mut self) -> Result<(), RadioError<R::Error>> {
        for (register, mask) in [
            (Register::new(Bank::Ll, 0x1a), 7),
            (Register::new(Bank::PowerOn, 0x0a), 1),
            (Register::new(Bank::PowerOn, 8), 0x20),
        ] {
            let previous = self.read(register)?;
            self.write(register, previous & !mask)?;
        }
        Ok(())
    }

    fn disable_advertising(&mut self) -> Result<(), RadioError<R::Error>> {
        let control = Register::new(Bank::Ll, 0x10);
        let previous = self.read(control)?;
        self.write(control, previous & !1)
    }
}

impl<E> InitializationError<E> {
    fn new(stage: InitializationStage, cause: InitializationFailure<E>) -> Self {
        Self {
            stage,
            cause,
            calibration_cleanup: None,
            advertising_cleanup: None,
        }
    }
    fn radio(stage: InitializationStage, error: RadioError<E>) -> Self {
        Self::new(stage, InitializationFailure::Radio(error))
    }
}
impl<E> From<RadioError<E>> for InitializationFailure<E> {
    fn from(error: RadioError<E>) -> Self {
        Self::Radio(error)
    }
}
