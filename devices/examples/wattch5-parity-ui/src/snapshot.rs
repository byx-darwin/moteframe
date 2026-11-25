//! Caller-owned borrowed data. The UI never advances or derives service values.
use crate::{
    MeasureKind, MeasureState, SportPhase, StopwatchState, TimerState, UiError, WeatherCondition,
    Weekday, glyph,
};
/// Bounded reading representation; a tenth is fixed-point, not a health algorithm.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reading {
    /// Missing externally supplied value.
    Missing,
    /// Whole value.
    Number(u16),
    /// Tenths (365 means36.5).
    Tenth(u16),
    /// Blood-pressure pair.
    Pressure {
        /// Upper reading.
        upper: u16,
        /// Lower reading.
        lower: u16,
    },
}
/// A separate snapshot for each measurement kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeasurementSnapshot {
    /// External phase.
    pub state: MeasureState,
    /// External reading.
    pub reading: Reading,
}
impl Default for MeasurementSnapshot {
    fn default() -> Self {
        Self {
            state: MeasureState::Idle,
            reading: Reading::Missing,
        }
    }
}
/// Countdown values supplied by the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimerSnapshot {
    /// `Seconds0..359_999`.
    pub seconds: u32,
    /// External phase.
    pub state: TimerState,
}
/// Stopwatch values; at most20 caller-owned laps.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StopwatchSnapshot<'a> {
    /// `Hundredths0..599_999`.
    pub centis: u32,
    /// External phase.
    pub state: StopwatchState,
    /// Hundredths per lap; no local lap service.
    pub laps: &'a [u32],
}
/// Workout values supplied by the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SportSnapshot {
    /// External phase.
    pub phase: SportPhase,
    /// Preparation countdown0..3.
    pub countdown: u8,
    /// `Seconds0..359_999`.
    pub elapsed_seconds: u32,
    /// Distance hundredths0..99999.
    pub distance: u32,
    /// Calories0..99999.
    pub calories: u32,
    /// Optional heart reading0..999.
    pub heart: Option<u16>,
    /// Caller-owned lock.
    pub locked: bool,
}
/// Caller-supplied optional low/high temperature and weather.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeatherSnapshot {
    /// Low temperature-99..99.
    pub low: i16,
    /// High temperature-99..99.
    pub high: i16,
    /// Original condition.
    pub condition: WeatherCondition,
}
/// Localized caller-owned text. Real text is rendered, not replaced with a fixed sample.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Message<'a> {
    /// Chinese content, bounded120 UTF-8 bytes.
    pub zh: &'a str,
    /// English content, bounded120 UTF-8 bytes.
    pub en: &'a str,
}
impl core::fmt::Debug for Message<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Message")
            .field("zh_bytes", &self.zh.len())
            .field("en_bytes", &self.en.len())
            .finish()
    }
}
/// Device information; absent values display missing markers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeviceInfo<'a> {
    /// Caller model string.
    pub model: Option<&'a str>,
    /// Caller version string.
    pub version: Option<&'a str>,
    /// Caller formatted address.
    pub address: Option<&'a str>,
}
/// Borrowed display snapshot; contains no platform or BLE types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot<'a> {
    /// Hour0..23.
    pub hour: u8,
    /// Minute0..59.
    pub minute: u8,
    /// Month1..12.
    pub month: u8,
    /// Day1..31.
    pub day: u8,
    /// Caller weekday.
    pub weekday: Weekday,
    /// Battery0..100.
    pub battery: u8,
    /// Independent snapshots in `MeasureKind` order.
    pub measurements: [MeasurementSnapshot; 6],
    /// Optional history/activity heart values0..999.
    pub heart_high: Option<u16>,
    /// Optional heart minimum.
    pub heart_low: Option<u16>,
    /// Optional activity heart value.
    pub activity_heart: Option<u16>,
    /// Step count0..99999.
    pub steps: u32,
    /// Distance hundredths0..99999.
    pub distance: u32,
    /// Calories0..99999.
    pub calories: u32,
    /// Optional total sleep minutes0..5999.
    pub sleep_total: Option<u16>,
    /// Optional deep sleep minutes.
    pub sleep_deep: Option<u16>,
    /// Optional light sleep minutes.
    pub sleep_light: Option<u16>,
    /// At most20 bounded localized message bodies.
    pub messages: &'a [Message<'a>],
    /// External countdown.
    pub timer: TimerSnapshot,
    /// External stopwatch and borrowed laps.
    pub stopwatch: StopwatchSnapshot<'a>,
    /// External workout.
    pub sport: SportSnapshot,
    /// Optional weather.
    pub weather: Option<WeatherSnapshot>,
    /// Device identity.
    pub info: DeviceInfo<'a>,
    /// Caller-provided21x21 modules; not a QR encoder or scanner certificate.
    pub qr: Option<&'a [[bool; 21]; 21]>,
    /// Installed-face capability; never loads executable content.
    pub applet_available: bool,
    /// Charge percentage0..100.
    pub charging: u8,
    /// External update/download progress0..100.
    pub progress: u8,
}
const DEMO_MESSAGES: [Message<'static>; 2] = [
    Message {
        zh: "演示消息内容",
        en: "DEMO MESSAGE",
    },
    Message {
        zh: "演示消息内容",
        en: "SECOND DEMO",
    },
];
impl Default for Snapshot<'_> {
    fn default() -> Self {
        Self {
            hour: 10,
            minute: 50,
            month: 10,
            day: 27,
            weekday: Weekday::Tue,
            battery: 82,
            measurements: [MeasurementSnapshot::default(); 6],
            heart_high: None,
            heart_low: None,
            activity_heart: None,
            steps: 0,
            distance: 0,
            calories: 0,
            sleep_total: None,
            sleep_deep: None,
            sleep_light: None,
            messages: &DEMO_MESSAGES,
            timer: TimerSnapshot {
                seconds: 300,
                state: TimerState::Idle,
            },
            stopwatch: StopwatchSnapshot {
                centis: 1284,
                state: StopwatchState::Paused,
                laps: &[1040, 1284],
            },
            sport: SportSnapshot {
                phase: SportPhase::Countdown,
                countdown: 3,
                elapsed_seconds: 0,
                distance: 0,
                calories: 0,
                heart: None,
                locked: false,
            },
            weather: None,
            info: DeviceInfo {
                model: Some("WATTCH 5"),
                version: None,
                address: None,
            },
            qr: None,
            applet_available: false,
            charging: 82,
            progress: 42,
        }
    }
}
fn text_width(s: &str) -> Result<usize, UiError> {
    s.chars().try_fold(0_usize, |n, ch| {
        let g = glyph(ch)?;
        n.checked_add(if g.width == 5 { 6 } else { 12 })
            .ok_or(UiError::Data)
    })
}
impl Snapshot<'_> {
    /// Missing product observations, without demo messages, laps or readings.
    ///
    /// Mandatory clock/date/battery/activity scalars are validation placeholders;
    /// render this value through `Model::product_scene` until providers exist.
    #[must_use]
    pub const fn unavailable() -> Self {
        Self {
            hour: 0,
            minute: 0,
            month: 1,
            day: 1,
            weekday: Weekday::Mon,
            battery: 0,
            measurements: [MeasurementSnapshot {
                state: MeasureState::Unavailable,
                reading: Reading::Missing,
            }; 6],
            heart_high: None,
            heart_low: None,
            activity_heart: None,
            steps: 0,
            distance: 0,
            calories: 0,
            sleep_total: None,
            sleep_deep: None,
            sleep_light: None,
            messages: &[],
            timer: TimerSnapshot {
                seconds: 0,
                state: TimerState::Unavailable,
            },
            stopwatch: StopwatchSnapshot {
                centis: 0,
                state: StopwatchState::Unavailable,
                laps: &[],
            },
            sport: SportSnapshot {
                phase: SportPhase::Done,
                countdown: 0,
                elapsed_seconds: 0,
                distance: 0,
                calories: 0,
                heart: None,
                locked: false,
            },
            weather: None,
            info: DeviceInfo {
                model: Some("WATTCH 5"),
                version: None,
                address: None,
            },
            qr: None,
            applet_available: false,
            charging: 0,
            progress: 0,
        }
    }
    /// Validates every field before a model update or prepared render can commit.
    /// # Errors
    /// Invalid ranges/slices and unsupported glyphs reject the whole snapshot.
    pub fn validate(&self) -> Result<(), UiError> {
        if self.hour > 23
            || self.minute > 59
            || !(1..=12).contains(&self.month)
            || !(1..=31).contains(&self.day)
            || self.battery > 100
            || self.charging > 100
            || self.progress > 100
            || self.timer.seconds > 359_999
            || self.stopwatch.centis > 599_999
            || self.stopwatch.laps.len() > 20
            || self.stopwatch.laps.iter().any(|x| *x > 599_999)
            || self.messages.len() > 20
            || self.steps > 99999
            || self.distance > 99999
            || self.calories > 99999
            || self.sport.countdown > 3
            || self.sport.elapsed_seconds > 359_999
            || self.sport.distance > 99999
            || self.sport.calories > 99999
        {
            return Err(UiError::Data);
        }
        for v in [
            self.heart_high,
            self.heart_low,
            self.activity_heart,
            self.sport.heart,
        ] {
            if v.is_some_and(|v| v > 999) {
                return Err(UiError::Data);
            }
        }
        for v in [self.sleep_total, self.sleep_deep, self.sleep_light] {
            if v.is_some_and(|v| v > 5999) {
                return Err(UiError::Data);
            }
        }
        for (i, m) in self.measurements.iter().enumerate() {
            let valid = match m.reading {
                Reading::Missing => true,
                Reading::Number(n) => i != 1 && n <= 999,
                Reading::Tenth(n) => (3..=5).contains(&i) && n <= 9999,
                Reading::Pressure { upper, lower } => i == 1 && upper <= 999 && lower <= 999,
            };
            if !valid {
                return Err(UiError::Data);
            }
        }
        if self
            .weather
            .is_some_and(|w| w.low < -99 || w.high > 99 || w.low > w.high)
        {
            return Err(UiError::Data);
        }
        for m in self.messages {
            for s in [m.zh, m.en] {
                if s.len() > 120 {
                    return Err(UiError::Data);
                }
                text_width(s)?;
            }
        }
        for (s, max) in [
            (self.info.model, 72),
            (self.info.version, 64),
            (self.info.address, 88),
        ] {
            if let Some(s) = s
                && (s.len() > 120 || text_width(s)?.saturating_sub(1) > max)
            {
                return Err(UiError::Data);
            }
        }
        Ok(())
    }
    /// Returns the specific caller-owned measurement.
    #[must_use]
    pub const fn measurement(&self, kind: MeasureKind) -> MeasurementSnapshot {
        self.measurements[kind as usize]
    }
}
