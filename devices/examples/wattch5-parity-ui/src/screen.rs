//! Generated from the approved V2 contract; preserve registry order.
use crate::UiError;
/// Finite presentation scenes, including registered source stubs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Screen {
    /// boot scene.
    Boot,
    /// dial scene.
    Dial,
    /// activity scene.
    Activity,
    /// sport scene.
    Sport,
    /// heart scene.
    Heart,
    /// pressure scene.
    Pressure,
    /// oxygen scene.
    Oxygen,
    /// sleep scene.
    Sleep,
    /// alarm scene.
    Alarm,
    /// stopwatch scene.
    Stopwatch,
    /// findphone scene.
    Findphone,
    /// about scene.
    About,
    /// power scene.
    Power,
    /// messages scene.
    Messages,
    /// weather scene.
    Weather,
    /// settings scene.
    Settings,
    /// ota scene.
    Ota,
    /// face scene.
    Face,
    /// notification scene.
    Notification,
    /// tools scene.
    Tools,
    /// quick scene.
    Quick,
    /// brightness scene.
    Brightness,
    /// clear scene.
    Clear,
    /// timer scene.
    Timer,
    /// reset scene.
    Reset,
    /// music scene.
    Music,
    /// language scene.
    Language,
    /// temperature scene.
    Temperature,
    /// shutdown scene.
    Shutdown,
    /// restart scene.
    Restart,
    /// textsize scene.
    Textsize,
    /// charge scene.
    Charge,
    /// lock scene.
    Lock,
    /// unlock scene.
    Unlock,
    /// applet scene.
    Applet,
    /// glucose scene.
    Glucose,
    /// hba1c scene.
    Hba1c,
    /// call scene.
    Call,
    /// noticeApp scene.
    NoticeApp,
    /// noticeAlarm scene.
    NoticeAlarm,
    /// noticeFind scene.
    NoticeFind,
    /// noticeDisconnect scene.
    NoticeDisconnect,
    /// noticeMove scene.
    NoticeMove,
    /// noticeBattery scene.
    NoticeBattery,
    /// camera scene.
    Camera,
    /// noticeTimer scene.
    NoticeTimer,
    /// noticeHeart scene.
    NoticeHeart,
    /// timerCustom scene.
    TimerCustom,
    /// timerRun scene.
    TimerRun,
    /// timerDone scene.
    TimerDone,
    /// sportList scene.
    SportList,
    /// sportCountdown scene.
    SportCountdown,
    /// sportRun scene.
    SportRun,
    /// sportControls scene.
    SportControls,
    /// sportEnd scene.
    SportEnd,
    /// messageDetail scene.
    MessageDetail,
    /// deviceQr scene.
    DeviceQr,
    /// measureTips scene.
    MeasureTips,
    /// measureConfirm scene.
    MeasureConfirm,
    /// theme scene.
    Theme,
    /// format scene.
    Format,
    /// request scene.
    Request,
}
impl Screen {
    /// All62 scenes in source registry/notice/extra order.
    pub const ALL: [Self; 62] = [
        Self::Boot,
        Self::Dial,
        Self::Activity,
        Self::Sport,
        Self::Heart,
        Self::Pressure,
        Self::Oxygen,
        Self::Sleep,
        Self::Alarm,
        Self::Stopwatch,
        Self::Findphone,
        Self::About,
        Self::Power,
        Self::Messages,
        Self::Weather,
        Self::Settings,
        Self::Ota,
        Self::Face,
        Self::Notification,
        Self::Tools,
        Self::Quick,
        Self::Brightness,
        Self::Clear,
        Self::Timer,
        Self::Reset,
        Self::Music,
        Self::Language,
        Self::Temperature,
        Self::Shutdown,
        Self::Restart,
        Self::Textsize,
        Self::Charge,
        Self::Lock,
        Self::Unlock,
        Self::Applet,
        Self::Glucose,
        Self::Hba1c,
        Self::Call,
        Self::NoticeApp,
        Self::NoticeAlarm,
        Self::NoticeFind,
        Self::NoticeDisconnect,
        Self::NoticeMove,
        Self::NoticeBattery,
        Self::Camera,
        Self::NoticeTimer,
        Self::NoticeHeart,
        Self::TimerCustom,
        Self::TimerRun,
        Self::TimerDone,
        Self::SportList,
        Self::SportCountdown,
        Self::SportRun,
        Self::SportControls,
        Self::SportEnd,
        Self::MessageDetail,
        Self::DeviceQr,
        Self::MeasureTips,
        Self::MeasureConfirm,
        Self::Theme,
        Self::Format,
        Self::Request,
    ];
    /// Stable host/reference identifier.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Boot => "boot",
            Self::Dial => "dial",
            Self::Activity => "activity",
            Self::Sport => "sport",
            Self::Heart => "heart",
            Self::Pressure => "pressure",
            Self::Oxygen => "oxygen",
            Self::Sleep => "sleep",
            Self::Alarm => "alarm",
            Self::Stopwatch => "stopwatch",
            Self::Findphone => "findphone",
            Self::About => "about",
            Self::Power => "power",
            Self::Messages => "messages",
            Self::Weather => "weather",
            Self::Settings => "settings",
            Self::Ota => "ota",
            Self::Face => "face",
            Self::Notification => "notification",
            Self::Tools => "tools",
            Self::Quick => "quick",
            Self::Brightness => "brightness",
            Self::Clear => "clear",
            Self::Timer => "timer",
            Self::Reset => "reset",
            Self::Music => "music",
            Self::Language => "language",
            Self::Temperature => "temperature",
            Self::Shutdown => "shutdown",
            Self::Restart => "restart",
            Self::Textsize => "textsize",
            Self::Charge => "charge",
            Self::Lock => "lock",
            Self::Unlock => "unlock",
            Self::Applet => "applet",
            Self::Glucose => "glucose",
            Self::Hba1c => "hba1c",
            Self::Call => "call",
            Self::NoticeApp => "noticeApp",
            Self::NoticeAlarm => "noticeAlarm",
            Self::NoticeFind => "noticeFind",
            Self::NoticeDisconnect => "noticeDisconnect",
            Self::NoticeMove => "noticeMove",
            Self::NoticeBattery => "noticeBattery",
            Self::Camera => "camera",
            Self::NoticeTimer => "noticeTimer",
            Self::NoticeHeart => "noticeHeart",
            Self::TimerCustom => "timerCustom",
            Self::TimerRun => "timerRun",
            Self::TimerDone => "timerDone",
            Self::SportList => "sportList",
            Self::SportCountdown => "sportCountdown",
            Self::SportRun => "sportRun",
            Self::SportControls => "sportControls",
            Self::SportEnd => "sportEnd",
            Self::MessageDetail => "messageDetail",
            Self::DeviceQr => "deviceQr",
            Self::MeasureTips => "measureTips",
            Self::MeasureConfirm => "measureConfirm",
            Self::Theme => "theme",
            Self::Format => "format",
            Self::Request => "request",
        }
    }
    /// Parses a bounded scene identifier.
    /// # Errors
    /// Unknown identifiers are rejected.
    pub fn from_id(s: &str) -> Result<Self, UiError> {
        Self::ALL
            .into_iter()
            .find(|p| p.id() == s)
            .ok_or(UiError::UnknownScreen)
    }
}
