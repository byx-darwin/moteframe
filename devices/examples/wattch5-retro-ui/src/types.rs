//! Fixed presentation types. No clocks, persistence or device-service types.
/// `Screen` choices in the approved presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Screen {
    /// Default clock face.
    Dial,
    /// Three tool entries.
    Tools,
    /// Quick settings.
    Quick,
    /// General settings.
    Settings,
    /// Appearance submenu.
    Display,
    /// Brightness draft.
    Brightness,
    /// Face draft.
    Face,
    /// Theme draft.
    Theme,
    /// Time-format draft.
    Format,
    /// Static skin information.
    About,
    /// Injected timer snapshot.
    Timer,
    /// Injected stopwatch snapshot.
    Stopwatch,
    /// System requests.
    System,
    /// Explicit system confirmation.
    Confirm,
    /// Request-only feedback.
    Request,
}
/// `Face` choices in the approved presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Face {
    /// Stacked pixel digits.
    Stack,
    /// Horizontal clock.
    Line,
    /// Seven-segment clock.
    Segment,
}
/// `Theme` choices in the approved presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Theme {
    /// Green phosphor palette.
    Phosphor,
    /// Amber palette.
    Amber,
    /// Cool blue palette.
    Ice,
}
/// `TimeFormat` choices in the approved presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TimeFormat {
    /// 24-hour clock.
    Hour24,
    /// 12-hour clock.
    Hour12,
}
/// `Field` choices in the approved presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Field {
    /// Brightness level.
    Brightness,
    /// Face layout.
    Face,
    /// Palette.
    Theme,
    /// Clock format.
    Format,
}
/// `Notice` choices in the approved presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Notice {
    /// Request has no device action.
    DemoOnly,
    /// External capability unavailable.
    Unavailable,
}
/// `TimerState` choices in the approved presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum TimerState {
    /// Not running.
    Idle,
    /// Injected running snapshot.
    Running,
    /// Injected paused snapshot.
    Paused,
    /// Injected completed snapshot.
    Done,
    /// No injected data.
    Unavailable,
}
/// `StopwatchState` choices in the approved presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum StopwatchState {
    /// Not running.
    Idle,
    /// Injected running snapshot.
    Running,
    /// Injected paused snapshot.
    Paused,
    /// No injected data.
    Unavailable,
}
/// `Weekday` choices in the approved presentation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Weekday {
    /// Monday.
    Mon,
    /// Tuesday.
    Tue,
    /// Wednesday.
    Wed,
    /// Thursday.
    Thu,
    /// Friday.
    Fri,
    /// Saturday.
    Sat,
    /// Sunday.
    Sun,
}
/// Normalized platform event; Select is also available for diagnostic input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// Swipe up.
    Up,
    /// Swipe down.
    Down,
    /// Swipe left.
    Left,
    /// Swipe right/back.
    Right,
    /// Touch coordinates, rejected atomically when outside128x128.
    Tap {
        /// Horizontal coordinate.
        x: u16,
        /// Vertical coordinate.
        y: u16,
    },
    /// Touch long press.
    LongPress,
    /// Debounced side-key release.
    KeyRelease,
    /// Debounced side-key long press.
    KeyLongPress,
    /// Explicit diagnostic selection; physical tap works independently.
    Select,
}
/// Request emitted for an external caller, never executed by the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiRequest {
    /// Preview a0..4 backlight selection.
    BrightnessPreview(u8),
    /// RAM presentation selection, not a persistent write acknowledgement.
    AppearanceCommit {
        /// Changed appearance field.
        field: Field,
        /// Validated index for that field.
        value: u8,
    },
    /// Caller-owned `TimerStart` request.
    TimerStart,
    /// Caller-owned `TimerPause` request.
    TimerPause,
    /// Caller-owned `TimerResume` request.
    TimerResume,
    /// Caller-owned `TimerDataRequest` request.
    TimerDataRequest,
    /// Caller-owned `StopwatchStart` request.
    StopwatchStart,
    /// Caller-owned `StopwatchPause` request.
    StopwatchPause,
    /// Caller-owned `StopwatchResume` request.
    StopwatchResume,
    /// Caller-owned `StopwatchDataRequest` request.
    StopwatchDataRequest,
    /// Caller-owned `DisplayToggle` request.
    DisplayToggle,
    /// Caller-owned `DisplaySleep` request.
    DisplaySleep,
    /// Caller-owned `Restart` request.
    Restart,
    /// Caller-owned `PowerOff` request.
    PowerOff,
    /// Caller-owned `ResetSettings` request.
    ResetSettings,
}
/// At most one request per event. Navigation is reflected in the updated model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// No external request.
    None,
    /// External action requested, without execution or success claim.
    Request(UiRequest),
}
/// Invalid data, coordinates, or exhausted navigation capacity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiError {
    /// Snapshot has an out-of-range numeric field.
    Data,
    /// Tap is outside logical128x128.
    Coordinate,
    /// Fixed six-frame navigation stack is full.
    NavigationOverflow,
}
/// Saved demo appearance, never read from or written to hardware by this crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Appearance {
    /// Level0..4.
    pub brightness: u8,
    /// Clock layout.
    pub face: Face,
    /// Four-color palette.
    pub theme: Theme,
    /// Clock format.
    pub format: TimeFormat,
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            brightness: 2,
            face: Face::Stack,
            theme: Theme::Phosphor,
            format: TimeFormat::Hour24,
        }
    }
}
/// Injected demo values; validation is atomic when accepted by the model.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DisplayData {
    /// Hour0..23.
    pub hour: u8,
    /// Minute0..59.
    pub minute: u8,
    /// Month1..12, no calendar calculation.
    pub month: u8,
    /// Day1..31, no calendar calculation.
    pub day: u8,
    /// Caller-supplied weekday.
    pub weekday: Weekday,
    /// Battery percentage0..100, no battery driver.
    pub battery: u8,
    /// Timer seconds0..5999, no automatic countdown.
    pub timer_seconds: u16,
    /// Caller-supplied timer state.
    pub timer_state: TimerState,
    /// Stopwatch hundredths0..599999, no automatic timer.
    pub stopwatch_centis: u32,
    /// Caller-supplied stopwatch state.
    pub stopwatch_state: StopwatchState,
}
impl DisplayData {
    pub(crate) fn validate(self) -> Result<(), UiError> {
        if self.hour > 23
            || self.minute > 59
            || !(1..=12).contains(&self.month)
            || !(1..=31).contains(&self.day)
            || self.battery > 100
            || self.timer_seconds > 5999
            || self.stopwatch_centis > 599_999
        {
            return Err(UiError::Data);
        }
        Ok(())
    }
}
impl Default for DisplayData {
    fn default() -> Self {
        Self {
            hour: 10,
            minute: 50,
            month: 10,
            day: 27,
            weekday: Weekday::Tue,
            battery: 82,
            timer_seconds: 300,
            timer_state: TimerState::Idle,
            stopwatch_centis: 1284,
            stopwatch_state: StopwatchState::Paused,
        }
    }
}
