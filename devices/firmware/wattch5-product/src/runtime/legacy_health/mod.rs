//! Original product value paths over injected inputs, source-aware storage and UI views.
mod backend;
mod history;
mod ui;
pub use backend::{LegacyBackend, LegacyInputSource, LegacyProduct};
use device_health::{BloodPressure, Error, Glucose, Oxygen, ResultOrigin, SourcedResult};
pub use history::{HealthHistory, HealthLog, HealthSchema, LatestView};
pub use ui::{LegacyActions, LegacyObservations};
use wattch5_parity_ui_demo::{MeasureState, MeasurementSnapshot, Reading};
/// Actual inputs supplied by product owners. Missing values never select demo defaults.
#[derive(Debug, Clone, Copy)]
pub struct LegacyInputs {
    /// Actual current heart-rate result; the original task gated all three paths on positive HR.
    pub heart_rate: Option<u16>,
    /// Actual configured age.
    pub age: u8,
    /// Original sex==1 table branch.
    pub male: bool,
    /// Actual explicitly enabled reference; absent selects the original age/sex table.
    pub reference_bp: Option<(u8, u8)>,
    /// Actual local hour required by the retained time-generated glucose path.
    pub hour: Option<u8>,
    /// Actual raw legacy-compatible tick value; this is not UTC.
    pub clock_ticks: Option<u32>,
}
/// Execute the retained original formulas and append one canonical origin byte.
/// # Errors
/// Missing HR/clock, unsupported module, unrepresentable values or short output fail.
pub fn calculate(module: u8, inputs: LegacyInputs, out: &mut [u8]) -> Result<usize, Error> {
    let hr = inputs
        .heart_rate
        .filter(|v| *v > 0)
        .ok_or(Error::Unavailable)?;
    let mut value = [0; 8];
    let (length, origin) = match module {
        0x25 => {
            let (upper, lower) = inputs
                .reference_bp
                .unwrap_or_else(|| base(inputs.age, inputs.male));
            let delta = f64::from(hr.max(50)) - 75.0;
            let sys = f64::from(upper) + 0.42 * delta;
            let dia = f64::from(lower) + 0.28 * delta;
            if !(0.0..256.0).contains(&sys) || !(0.0..256.0).contains(&dia) {
                return Err(Error::InvalidField);
            }
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            // Original C truncates nonnegative, checked u8-representable formulas.
            let body = BloodPressure {
                systolic: Some(sys as u16),
                diastolic: Some(dia as u16),
                pulse: Some(hr),
            };
            (body.encode(&mut value)?, ResultOrigin::Estimate)
        }
        0x26 => {
            let delta = if hr > 85 {
                ((85.0 - f64::from(hr)) / 30.0).max(-1.5)
            } else {
                ((85.0 - f64::from(hr)) / 17.0).min(1.2)
            };
            #[allow(clippy::cast_possible_truncation)]
            // Match original float narrowing before float addition.
            let raw = 98.0_f32 + delta as f32 + 0.05_f32;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            // raw is bounded96.55..99.25; preserve original integer/tenth truncation.
            let percent = (raw as u16) * 100 + ((raw * 10.0) as u16 % 10) * 10;
            (
                Oxygen {
                    spo2_x100: Some(percent),
                    pi_x100: None,
                }
                .encode(&mut value)?,
                ResultOrigin::Estimate,
            )
        }
        0x27 => {
            let hour = inputs.hour.filter(|h| *h < 24).ok_or(Error::Unavailable)?;
            let ticks = inputs.clock_ticks.ok_or(Error::Unavailable)?;
            let (low, high): (f32, f32) = match hour {
                6 | 21 => (4.5, 5.5),
                7 | 12 | 20 => (5.5, 6.5),
                8 | 13 => (6.5, 8.5),
                9 | 14 => (7.0, 8.0),
                10 | 15 => (6.0, 7.0),
                17 | 22 | 23 => (4.0, 5.0),
                18 => (6.0, 8.0),
                19 => (6.5, 7.5),
                _ => (5.0, 6.0),
            };
            let digit = u8::try_from(ticks % 10).map_err(|_| Error::InvalidField)?;
            #[allow(clippy::cast_possible_truncation)]
            // Original fabs promotes the interpolation to double before float return.
            let raw = (f64::from(low) + (f64::from(high - low) / 9.0) * f64::from(digit)) as f32;
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            // Original C uses integer part and separately rounded tenths; raw stays4..8.5.
            let glucose = (raw as u16) * 100 + (((raw + 0.05) * 10.0) as u16 % 10) * 10;
            (
                Glucose {
                    mmol_l_x100: Some(glucose),
                }
                .encode(&mut value)?,
                ResultOrigin::LegacyGenerated,
            )
        }
        _ => return Err(Error::Unsupported),
    };
    SourcedResult::new(module, &value[..length], origin)?.encode(out)
}
fn base(age: u8, male: bool) -> (u8, u8) {
    match (age, male) {
        (0..25, true) => (115, 73),
        (0..20, false) => (110, 70),
        (20..25, false) => (110, 71),
        (25..30, true) => (115, 75),
        (25..30, false) => (112, 73),
        (30..35, true) => (117, 76),
        (30..35, false) => (114, 74),
        (35..40, true) => (120, 80),
        (35..40, false) => (116, 77),
        (40..45, true) => (124, 81),
        (40..45, false) => (122, 78),
        (45..50, true) => (128, 82),
        (45..50, false) => (128, 79),
        (50..55, true) => (134, 84),
        (50..55, false) => (134, 80),
        (55..65, true) => (137, 84),
        (55..65, false) => (139, 82),
        (_, true) => (148, 86),
        (_, false) => (145, 83),
    }
}
/// Convert a validated committed result into the existing page model. Origin is not UI copy.
/// # Errors
/// Bad schema/values are rejected; no default physiological reading is substituted.
pub fn measurement(module: u8, bytes: &[u8]) -> Result<MeasurementSnapshot, Error> {
    let value = SourcedResult::decode(module, bytes)?.value();
    let reading = match module {
        0x25 => {
            let v = BloodPressure::decode(value)?;
            match (v.systolic, v.diastolic) {
                (Some(upper), Some(lower)) => Reading::Pressure { upper, lower },
                _ => Reading::Missing,
            }
        }
        0x26 => Oxygen::decode(value)?
            .spo2_x100
            .map_or(Reading::Missing, |n| Reading::Number(n / 100)),
        0x27 => Glucose::decode(value)?
            .mmol_l_x100
            .map_or(Reading::Missing, |n| {
                Reading::Tenth(n / 10 + u16::from(n % 10 >= 5))
            }),
        _ => return Err(Error::Unsupported),
    };
    // Wire values remain full u16. The existing pressure page accepts three digits;
    // an unrepresentable imported value must not invalidate the entire UI snapshot.
    if matches!(reading, Reading::Pressure { upper, lower } if upper > 999 || lower > 999) {
        Ok(MeasurementSnapshot {
            state: MeasureState::Error,
            reading: Reading::Missing,
        })
    } else {
        Ok(MeasurementSnapshot {
            state: MeasureState::Result,
            reading,
        })
    }
}
