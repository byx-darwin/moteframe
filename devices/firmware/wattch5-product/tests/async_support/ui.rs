//! Explicit offline UI input/calendar/display fixtures, no production fallback.
use device_runtime::PortError;
use device_services::UtcMapping;
use wattch5_parity_ui_demo::{Event, Snapshot, Weekday};
use wattch5_product::runtime::{
    Calendar, CalendarDate, NoActions, Observation, ObservationSource, TouchSource, UiPorts,
};
#[derive(Debug)]
pub struct Touch(pub std::collections::VecDeque<Event>);
impl TouchSource for Touch {
    fn next_event(&mut self) -> Result<Option<Event>, PortError> {
        Ok(self.0.pop_front())
    }
}
#[derive(Debug)]
pub struct Observations {
    pub unavailable: bool,
    pub generation: u64,
}
impl ObservationSource for Observations {
    fn capture(&mut self) -> Result<Observation<'_>, PortError> {
        if self.unavailable {
            return Err(PortError::Unavailable);
        }
        // Full fixture, independent literal observations; no demo Snapshot::default.
        Ok(Observation {
            generation: self.generation,
            snapshot: Snapshot {
                hour: 0,
                minute: 0,
                month: 1,
                day: 1,
                weekday: Weekday::Mon,
                battery: 37,
                measurements: [wattch5_parity_ui_demo::MeasurementSnapshot {
                    state: wattch5_parity_ui_demo::MeasureState::Unavailable,
                    reading: wattch5_parity_ui_demo::Reading::Missing,
                }; 6],
                heart_high: None,
                heart_low: None,
                activity_heart: None,
                steps: 23,
                distance: 1,
                calories: 2,
                sleep_total: None,
                sleep_deep: None,
                sleep_light: None,
                messages: &[],
                timer: wattch5_parity_ui_demo::TimerSnapshot {
                    seconds: 0,
                    state: wattch5_parity_ui_demo::TimerState::Unavailable,
                },
                stopwatch: wattch5_parity_ui_demo::StopwatchSnapshot {
                    centis: 0,
                    state: wattch5_parity_ui_demo::StopwatchState::Unavailable,
                    laps: &[],
                },
                sport: wattch5_parity_ui_demo::SportSnapshot {
                    phase: wattch5_parity_ui_demo::SportPhase::Done,
                    countdown: 0,
                    elapsed_seconds: 0,
                    distance: 0,
                    calories: 0,
                    heart: None,
                    locked: false,
                },
                weather: None,
                info: wattch5_parity_ui_demo::DeviceInfo {
                    model: None,
                    version: None,
                    address: None,
                },
                qr: None,
                applet_available: false,
                charging: 37,
                progress: 0,
            },
        })
    }
}
#[derive(Debug)]
pub struct Dates;
impl Calendar for Dates {
    fn convert(&mut self, m: UtcMapping) -> Result<CalendarDate, PortError> {
        // This test adapter accepts only its actual synchronized UTC fixture.
        if !m.valid || m.utc_ms != 1_761_696_000_000 {
            return Err(PortError::Unavailable);
        }
        Ok(CalendarDate {
            hour: 8,
            minute: 0,
            month: 10,
            day: 29,
            weekday: Weekday::Wed,
        })
    }
}
#[derive(Debug, Default)]
pub struct Display {
    pub rows: Vec<device_core::Rect>,
    pub fail: bool,
    pub pixels: Vec<device_core::Rgb565>,
}
impl device_core::Display for Display {
    type Error = ();
    fn dimensions(&self) -> device_core::Dimensions {
        device_core::Dimensions::new(128, 128)
    }
    fn write(&mut self, r: device_core::Rect, p: &[device_core::Rgb565]) -> Result<(), ()> {
        assert_eq!(r.height, 1);
        assert_eq!(p.len(), usize::from(r.width));
        if self.fail {
            return Err(());
        }
        if self.pixels.is_empty() {
            self.pixels.resize(128 * 128, device_core::Rgb565(0));
        }
        let start = usize::from(r.y) * 128 + usize::from(r.x);
        self.pixels[start..start + p.len()].copy_from_slice(p);
        self.rows.push(r);
        Ok(())
    }
}
pub fn ports() -> UiPorts<Touch, Observations, Dates, Display, NoActions> {
    UiPorts {
        touch: Touch(std::collections::VecDeque::new()),
        observations: Observations {
            unavailable: false,
            generation: 1,
        },
        calendar: Dates,
        display: Display::default(),
        actions: NoActions,
    }
}
