//! Civil-day configuration only. No prediction,ovulation,disease or pregnancy-health algorithm.
use crate::{Error, boolean, exact};
/// Local civil day count from1970-01-01 in the configured timezone,not UTC milliseconds.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct DayNumber(i32);
redacted!(DayNumber, "DayNumber", 4);
impl DayNumber {
    /// Any i32 civil-day number is representable by the proposed layout.
    #[must_use]
    pub const fn new(value: i32) -> Self {
        Self(value)
    }
    /// Exact signed day number; timezone changes do not rewrite it.
    #[must_use]
    pub const fn get(self) -> i32 {
        self.0
    }
    /// Convert a proleptic Gregorian civil date; leap centuries are validated before conversion.
    /// # Errors
    /// Invalid month/day or day count outside i32 fails; no arbitrary medical date window is added.
    pub fn from_date(year: i32, month: u8, day: u8) -> Result<Self, Error> {
        let leap =
            year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0);
        let days = match month {
            1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
            4 | 6 | 9 | 11 => 30,
            2 => {
                if leap {
                    29
                } else {
                    28
                }
            }
            _ => return Err(Error::InvalidField),
        };
        if day == 0 || day > days {
            return Err(Error::InvalidField);
        }
        let y = i64::from(year) - i64::from(month <= 2);
        let era = y.div_euclid(400);
        let yoe = y - era * 400;
        let m = i64::from(month) + if month > 2 { -3 } else { 9 };
        let doy = (153 * m + 2) / 5 + i64::from(day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let value = era * 146_097 + doe - 719_468;
        Ok(Self(i32::try_from(value).map_err(|_| Error::InvalidField)?))
    }
    /// Recover the exact proleptic Gregorian date,including negative day numbers.
    /// # Errors
    /// Checked conversion protects the representation boundary.
    pub fn to_date(self) -> Result<(i32, u8, u8), Error> {
        let z = i64::from(self.0) + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
        let mut y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = doy - (153 * mp + 2) / 5 + 1;
        let month = mp + if mp < 10 { 3 } else { -9 };
        y += i64::from(month <= 2);
        Ok((
            i32::try_from(y).map_err(|_| Error::InvalidField)?,
            u8::try_from(month).map_err(|_| Error::InvalidField)?,
            u8::try_from(day).map_err(|_| Error::InvalidField)?,
        ))
    }
}
/// Mutually exclusive configuration mode,not a measured physiological state.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum WomensMode {
    /// Last period start and explicitly supplied configured durations.
    Cycle {
        /// Local civil anchor day.
        anchor: DayNumber,
        ///`1..cycle_days`.
        duration_days: u16,
        /// Configured cycle length,not an algorithmic estimate.
        cycle_days: u16,
    },
    /// Due date only; cycle/duration fields encode canonical0.
    Pregnancy {
        /// Local civil due date.
        due: DayNumber,
    },
}
redacted!(WomensMode, "WomensMode", 9);
/// Proposed2D.P0001 and2D.02 configuration body,exact10B; no LATEST wrapper.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct WomensConfig {
    /// Disabling retains a fully valid selected configuration.
    pub enabled: bool,
    /// Explicit mutually exclusive cycle/pregnancy parameters.
    pub mode: WomensMode,
}
redacted!(WomensConfig, "WomensConfig", 10);
impl WomensConfig {
    /// Exact config body size.
    pub const LENGTH: usize = 10;
    /// Decode strict booleans,modes and mutually exclusive canonical parameters.
    /// # Errors
    /// Invalid length,mode,duration relation or pregnancy nonzero unused fields fails.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        exact(bytes, 10)?;
        let enabled = boolean(bytes[0])?;
        let anchor = DayNumber::new(i32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]));
        let duration = u16::from_le_bytes([bytes[6], bytes[7]]);
        let cycle = u16::from_le_bytes([bytes[8], bytes[9]]);
        let mode = match bytes[1] {
            1 => WomensMode::Cycle {
                anchor,
                duration_days: duration,
                cycle_days: cycle,
            },
            2 => {
                if duration != 0 || cycle != 0 {
                    return Err(Error::InvalidField);
                }
                WomensMode::Pregnancy { due: anchor }
            }
            _ => return Err(Error::InvalidField),
        };
        let value = Self { enabled, mode };
        value.validate()?;
        Ok(value)
    }
    /// Validate configured relation1<=duration<=cycle. No clinical range or predictions are invented.
    /// # Errors
    /// Zero/excessive configured period duration fails even when disabled.
    pub const fn validate(self) -> Result<(), Error> {
        match self.mode {
            WomensMode::Cycle {
                duration_days,
                cycle_days,
                ..
            } if duration_days == 0 || duration_days > cycle_days => Err(Error::InvalidField),
            _ => Ok(()),
        }
    }
    /// Encode exact10B,clearing pregnancy's inapplicable cycle/duration fields.
    /// # Errors
    /// Invalid fields/short output fails before writing.
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
        self.validate()?;
        if out.len() < 10 {
            return Err(Error::Capacity);
        }
        out[0] = u8::from(self.enabled);
        match self.mode {
            WomensMode::Cycle {
                anchor,
                duration_days,
                cycle_days,
            } => {
                out[1] = 1;
                out[2..6].copy_from_slice(&anchor.get().to_le_bytes());
                out[6..8].copy_from_slice(&duration_days.to_le_bytes());
                out[8..10].copy_from_slice(&cycle_days.to_le_bytes());
            }
            WomensMode::Pregnancy { due } => {
                out[1] = 2;
                out[2..6].copy_from_slice(&due.get().to_le_bytes());
                out[6..10].fill(0);
            }
        }
        Ok(10)
    }
}
/// Actual configuration storage port. Atomic failure must preserve the previously committed config.
/// Authorization,profile binding,consent and durable storage implementation belong to the product.
pub trait WomensBackend {
    /// Whether an actual selected storage capability exists.
    fn available(&self) -> bool;
    /// Read actual committed configuration; None is not a fabricated default.
    /// # Errors
    /// Actual storage/unavailability failures remain explicit.
    fn load(&self) -> Result<Option<WomensConfig>, crate::BackendError>;
    /// Atomically replace configuration after full validation; no partial writes may report success.
    /// # Errors
    /// Failure must preserve the previous committed value.
    fn store_atomic(&mut self, value: WomensConfig) -> Result<(), crate::BackendError>;
}
/// Read actual configuration,not a disease/pregnancy state or a LATEST measurement wrapper.
/// # Errors
/// Missing backend is Unavailable,missing config is NotFound,malformed stored data is rejected.
pub fn read_womens<B: WomensBackend>(backend: &B) -> Result<WomensConfig, Error> {
    if !backend.available() {
        return Err(Error::Unavailable);
    }
    let value = backend.load()?.ok_or(Error::NotFound)?;
    value.validate()?;
    Ok(value)
}
/// Validate and atomically commit exactly the supplied configuration through the actual port.
/// # Errors
/// Validation,unavailability and storage faults are explicit; no synthesized successful config.
pub fn write_womens<B: WomensBackend>(backend: &mut B, value: WomensConfig) -> Result<(), Error> {
    value.validate()?;
    if !backend.available() {
        return Err(Error::Unavailable);
    }
    backend.store_atomic(value)?;
    Ok(())
}
