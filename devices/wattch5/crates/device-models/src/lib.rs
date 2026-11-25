//! Optional application models, with no sensor algorithm, UI or physical storage dependency.
#![no_std]
#[cfg(feature = "profile")]
mod profile;
#[cfg(feature = "profile")]
pub use profile::{ActivityGoals, UserProfile};
#[cfg(feature = "device")]
mod device;
#[cfg(feature = "device")]
pub use device::BatteryState;
#[cfg(feature = "settings")]
mod settings;
#[cfg(feature = "settings")]
pub use settings::{HourFormat, TimeWindow, Units};
#[cfg(any(feature = "profile", feature = "device"))]
fn canonical(mask: u64, values: &[u64]) -> Result<(), device_protocol::ProtocolError> {
    for (i, value) in values.iter().enumerate() {
        if mask & (1 << i) == 0 && *value != 0 {
            return Err(device_protocol::ProtocolError::InvalidState);
        }
    }
    Ok(())
}
#[cfg(any(feature = "reminders", feature = "notifications", feature = "weather"))]
mod body;
#[cfg(feature = "reminders")]
mod reminders;
#[cfg(feature = "reminders")]
pub use reminders::Reminder;
#[cfg(feature = "notifications")]
mod notifications;
#[cfg(feature = "notifications")]
pub use notifications::Notification;
#[cfg(feature = "weather")]
mod weather;
#[cfg(feature = "weather")]
pub use weather::{Weather, WeatherDay};
