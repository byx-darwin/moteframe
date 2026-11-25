//! Finite platform-independent state and requests.
use crate::Screen;
/// Invalid presentation input; no state is committed on error.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiError {
    /// Invalid snapshot value or slice length.
    Data,
    /// Tap outside128x128.
    Coordinate,
    /// Six-frame navigation is full.
    NavigationOverflow,
    /// Scene identifier is not part of the contract.
    UnknownScreen,
    /// Caller text uses a character absent from the local font.
    UnsupportedGlyph,
    /// Message exceeds the current bounded text layout.
    TextOverflow,
}
macro_rules! choices {
 ($name:ident,$doc:literal,[$($v:ident),+])=>{
 #[doc=$doc]#[derive(Clone,Copy,Debug,PartialEq,Eq)]#[repr(u8)]pub enum $name {$(#[doc=stringify!($v)]$v),+}
 };
}
choices!(
    Locale,
    "Implemented locales; other languages emit requests.",
    [Zh, En]
);
choices!(Face, "Built-in clock layouts.", [Stack, Line, Segment]);
choices!(
    Theme,
    "Indexed four-color palettes.",
    [Phosphor, Amber, Ice]
);
choices!(TimeFormat, "Clock format.", [Hour24, Hour12]);
choices!(
    Field,
    "RAM appearance fields.",
    [Brightness, Face, Theme, Format, Textsize, Language]
);
choices!(
    MeasureKind,
    "Independent externally measured quantities.",
    [Heart, Pressure, Oxygen, Temperature, Glucose, Hba1c]
);
choices!(
    MeasureState,
    "Externally supplied measurement state.",
    [Idle, Pending, Measuring, Result, Error, Unavailable]
);
choices!(
    TimerState,
    "Caller-owned countdown state.",
    [Idle, Running, Paused, Done, Unavailable]
);
choices!(
    StopwatchState,
    "Caller-owned stopwatch state.",
    [Idle, Running, Paused, Unavailable]
);
choices!(
    SportPhase,
    "Caller-owned workout state.",
    [Countdown, Running, Paused, Done]
);
choices!(
    SportKind,
    "Enabled source workout types; swimming disabled.",
    [Cycling, Mountain, Badminton, Run]
);
choices!(
    Language,
    "Original locale choices plus implemented Chinese.",
    [Zh, En, Fr, De, It, Pt, Ru, Es, Nl]
);
choices!(
    Weekday,
    "Caller-provided weekday; no calendar service.",
    [Mon, Tue, Wed, Thu, Fri, Sat, Sun]
);
choices!(
    WeatherCondition,
    "Original weather conditions.",
    [Fine, Cloudy, Rain, Snow, Smog, Sand, Unknown]
);
/// Normalized physical or host input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    /// Swipe up.
    Up,
    /// Swipe down.
    Down,
    /// Swipe left.
    Left,
    /// Swipe right.
    Right,
    /// Validated logical touch position.
    Tap {
        /// X coordinate.
        x: u16,
        /// Y coordinate.
        y: u16,
    },
    /// Long touch.
    LongPress,
    /// Side-key release.
    KeyRelease,
    /// Side-key hold.
    KeyLongPress,
    /// Host/keyboard selection.
    Select,
}
/// At most one caller request for a normalized input.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// No external request.
    None,
    /// Request only; never an acknowledgement of service completion.
    Request(UiRequest),
}
/// Requests are never executed by the UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiRequest {
    /// Preview backlight0..4.
    BrightnessPreview(u8),
    /// Commit RAM appearance; caller decides persistence.
    AppearanceCommit {
        /// Changed field.
        field: Field,
        /// Validated field index.
        value: u8,
    },
    /// Start a measurement, without manufacturing Pending or a reading.
    MeasureStart(MeasureKind),
    /// Stop the external measurement.
    MeasureStop(MeasureKind),
    /// Request external preparation; does not start time.
    TimerPrepare {
        /// Duration0..359999 seconds.
        seconds: u32,
    },
    /// Countdown service start.
    TimerStart,
    /// Countdown service pause.
    TimerPause,
    /// Countdown service resume.
    TimerResume,
    /// Countdown service stop.
    TimerStop,
    /// Acknowledge externally signalled completion.
    TimerAcknowledge,
    /// Stopwatch start.
    StopwatchStart,
    /// Stopwatch pause.
    StopwatchPause,
    /// Stopwatch resume.
    StopwatchResume,
    /// Request a lap; does not add one locally.
    StopwatchLap,
    /// Request clearing external laps/time.
    StopwatchReset,
    /// External messages read-all notification.
    MessagesReadAllRequested,
    /// Prepare external workout.
    SportPrepare(SportKind),
    /// Start external workout.
    SportStart(SportKind),
    /// Pause workout.
    SportPause,
    /// Resume workout.
    SportResume,
    /// Stop workout.
    SportStop,
    /// Change external workout lock.
    SportLockToggle(bool),
    /// Unlock request from locked workout.
    SportUnlockRequested,
    /// Unlock request from lock window.
    UnlockRequested,
    /// Optional installed-face selection; no execution/loading.
    InstalledFaceSelected,
    /// Unsupported locale requested; implemented locale stays unchanged.
    LanguageRequested(Language),
    /// Display power request only.
    DisplayToggle,
    /// Clear data request only.
    ClearData,
    /// Restart request only.
    Restart,
    /// Power-off request only.
    PowerOff,
    /// Lock request only.
    Lock,
    /// Previous external music item.
    MusicPrevious,
    /// Next external music item.
    MusicNext,
    /// Play/pause external music.
    MusicPlayPause,
    /// Volume down.
    VolumeDown,
    /// Volume up.
    VolumeUp,
    /// Reject incoming call.
    CallReject,
    /// Answer incoming call.
    CallAnswer,
    /// External camera shutter.
    CameraShutter,
    /// Find-phone toggle.
    FindPhoneToggle,
    /// Dismiss a notice and return to its source.
    NoticeDismiss(Screen),
}
/// Demo appearance, never persisted by this crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Appearance {
    /// Backlight level0..4.
    pub brightness: u8,
    /// Built-in face.
    pub face: Face,
    /// Palette.
    pub theme: Theme,
    /// Clock format.
    pub format: TimeFormat,
    /// Notification text scale0..1.
    pub textsize: u8,
    /// Implemented locale.
    pub locale: Locale,
}
impl Default for Appearance {
    fn default() -> Self {
        Self {
            brightness: 2,
            face: Face::Stack,
            theme: Theme::Phosphor,
            format: TimeFormat::Hour24,
            textsize: 0,
            locale: Locale::Zh,
        }
    }
}
choices!(
    NoticeKind,
    "Original ten notification kinds, including the disconnected source stub.",
    [
        Call, App, Alarm, Find, Disconnect, Move, Battery, Camera, Timer, Heart
    ]
);
impl NoticeKind {
    /// Original notification order.
    pub const ALL: [Self; 10] = [
        Self::Call,
        Self::App,
        Self::Alarm,
        Self::Find,
        Self::Disconnect,
        Self::Move,
        Self::Battery,
        Self::Camera,
        Self::Timer,
        Self::Heart,
    ];
    /// Presentation page; data stays in the caller snapshot.
    #[must_use]
    pub const fn screen(self) -> Screen {
        match self {
            Self::Call => Screen::Call,
            Self::App => Screen::NoticeApp,
            Self::Alarm => Screen::NoticeAlarm,
            Self::Find => Screen::NoticeFind,
            Self::Disconnect => Screen::NoticeDisconnect,
            Self::Move => Screen::NoticeMove,
            Self::Battery => Screen::NoticeBattery,
            Self::Camera => Screen::Camera,
            Self::Timer => Screen::NoticeTimer,
            Self::Heart => Screen::NoticeHeart,
        }
    }
}
/// External notification trigger; content is borrowed through the general snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoticeSnapshot {
    /// Explicit notification kind.
    pub kind: NoticeKind,
}
