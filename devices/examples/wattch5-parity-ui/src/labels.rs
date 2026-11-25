//! Fixed bilingual copy from contracts.js.
use crate::{Locale, Screen};
/// Finite identifiers for approved bilingual copy.
#[derive(Clone, Copy, Debug)]
#[repr(u8)]
pub enum Label {
    /// tools.
    Tools,
    /// quick.
    Quick,
    /// settings.
    Settings,
    /// timer.
    Timer,
    /// stopwatch.
    Stopwatch,
    /// weather.
    Weather,
    /// about.
    About,
    /// face.
    Face,
    /// brightness.
    Brightness,
    /// textsize.
    Textsize,
    /// language.
    Language,
    /// clear.
    Clear,
    /// restart.
    Restart,
    /// power.
    Power,
    /// heart.
    Heart,
    /// pressure.
    Pressure,
    /// oxygen.
    Oxygen,
    /// temperature.
    Temperature,
    /// glucose.
    Glucose,
    /// hba1c.
    Hba1c,
    /// activity.
    Activity,
    /// sleep.
    Sleep,
    /// messages.
    Messages,
    /// sport.
    Sport,
    /// music.
    Music,
    /// findphone.
    Findphone,
    /// lock.
    Lock,
    /// unlock.
    Unlock,
    /// alarm.
    Alarm,
    /// reset.
    Reset,
    /// boot.
    Boot,
    /// shutdown.
    Shutdown,
    /// charge.
    Charge,
    /// ota.
    Ota,
    /// applet.
    Applet,
    /// notification.
    Notification,
    /// call.
    Call,
    /// noticeApp.
    NoticeApp,
    /// noticeAlarm.
    NoticeAlarm,
    /// noticeFind.
    NoticeFind,
    /// noticeDisconnect.
    NoticeDisconnect,
    /// noticeMove.
    NoticeMove,
    /// noticeBattery.
    NoticeBattery,
    /// camera.
    Camera,
    /// noticeTimer.
    NoticeTimer,
    /// noticeHeart.
    NoticeHeart,
    /// timerCustom.
    TimerCustom,
    /// timerRun.
    TimerRun,
    /// timerDone.
    TimerDone,
    /// sportList.
    SportList,
    /// sportCountdown.
    SportCountdown,
    /// sportRun.
    SportRun,
    /// sportControls.
    SportControls,
    /// sportEnd.
    SportEnd,
    /// messageDetail.
    MessageDetail,
    /// deviceQr.
    DeviceQr,
    /// measureTips.
    MeasureTips,
    /// measureConfirm.
    MeasureConfirm,
    /// theme.
    Theme,
    /// format.
    Format,
    /// request.
    Request,
    /// start.
    Start,
    /// stop.
    Stop,
    /// pause.
    Pause,
    /// resume.
    Resume,
    /// apply.
    Apply,
    /// cancel.
    Cancel,
    /// yes.
    Yes,
    /// back.
    Back,
    /// demo.
    Demo,
    /// noData.
    NoData,
    /// measuring.
    Measuring,
    /// result.
    Result,
    /// idle.
    Idle,
    /// unavailable.
    Unavailable,
    /// pending.
    Pending,
    /// error.
    Error,
    /// high.
    High,
    /// low.
    Low,
    /// notRun.
    NotRun,
    /// requestOnly.
    RequestOnly,
    /// clearQuestion.
    ClearQuestion,
    /// powerQuestion.
    PowerQuestion,
    /// restartQuestion.
    RestartQuestion,
    /// lockQuestion.
    LockQuestion,
    /// sourceStub.
    SourceStub,
    /// noService.
    NoService,
    /// custom.
    Custom,
    /// lap.
    Lap,
    /// resetAction.
    ResetAction,
    /// next.
    Next,
    /// prev.
    Prev,
    /// play.
    Play,
    /// vol.
    Vol,
    /// messageSample.
    MessageSample,
    /// small.
    Small,
    /// large.
    Large,
    /// steps.
    Steps,
    /// distance.
    Distance,
    /// calories.
    Calories,
    /// deep.
    Deep,
    /// light.
    Light,
    /// total.
    Total,
    /// running.
    Running,
    /// paused.
    Paused,
    /// stopped.
    Stopped,
    /// duration.
    Duration,
    /// accept.
    Accept,
    /// reject.
    Reject,
    /// dismiss.
    Dismiss,
    /// shoot.
    Shoot,
    /// ready.
    Ready,
    /// info.
    Info,
    /// firmware.
    Firmware,
    /// pair.
    Pair,
    /// installed.
    Installed,
    /// cycling.
    Cycling,
    /// mountain.
    Mountain,
    /// badminton.
    Badminton,
    /// run.
    Run,
    /// end.
    End,
    /// locked.
    Locked,
    /// holdKey.
    HoldKey,
    /// safety.
    Safety,
    /// measureAsk.
    MeasureAsk,
    /// chinese.
    Chinese,
    /// english.
    English,
    /// remaining.
    Remaining,
    /// limits.
    Limits,
    /// languagePending.
    LanguagePending,
    /// noQr.
    NoQr,
    /// weekMon.
    WeekMon,
    /// weekTue.
    WeekTue,
    /// weekWed.
    WeekWed,
    /// weekThu.
    WeekThu,
    /// weekFri.
    WeekFri,
    /// weekSat.
    WeekSat,
    /// weekSun.
    WeekSun,
    /// weatherFine.
    WeatherFine,
    /// weatherCloudy.
    WeatherCloudy,
    /// weatherRain.
    WeatherRain,
    /// weatherSnow.
    WeatherSnow,
    /// weatherSmog.
    WeatherSmog,
    /// weatherSand.
    WeatherSand,
    /// weatherUnknown.
    WeatherUnknown,
    /// rangeLabel.
    RangeLabel,
    /// nextMessage.
    NextMessage,
}
static COPY: [[&str; 2]; 147] = [
    ["TOOLS", "功能菜单"],
    ["QUICK", "快捷菜单"],
    ["SETTINGS", "设置"],
    ["TIMER", "倒计时"],
    ["STOPWATCH", "秒表"],
    ["WEATHER", "天气"],
    ["DEVICE INFO", "设备信息"],
    ["DIAL", "更换表盘"],
    ["BRIGHT", "亮度调节"],
    ["TEXT SIZE", "通知文字"],
    ["LANGUAGE", "语言切换"],
    ["CLEAR DATA", "清除数据"],
    ["RESTART", "重启设备"],
    ["POWER OFF", "关机"],
    ["HEART", "心率"],
    ["PRESSURE", "血压"],
    ["OXYGEN", "血氧"],
    ["TEMP", "体温"],
    ["GLUCOSE", "血糖"],
    ["HBA1C", "糖化血红蛋白"],
    ["STEPS", "计步"],
    ["SLEEP", "睡眠"],
    ["MESSAGES", "消息"],
    ["SPORT", "运动"],
    ["MUSIC", "音乐"],
    ["FIND PHONE", "查找手机"],
    ["LOCK", "锁屏"],
    ["UNLOCK", "解锁"],
    ["ALARM", "闹钟"],
    ["RESET", "重置"],
    ["WATTCH 5", "启动"],
    ["POWER OFF", "关机画面"],
    ["CHARGING", "充电"],
    ["UPDATE", "固件升级"],
    ["DIAL LOAD", "表盘下载"],
    ["NOTICE", "通知入口"],
    ["CALL", "来电"],
    ["APP NOTICE", "应用通知"],
    ["ALARM", "闹钟提醒"],
    ["FIND WATCH", "查找手表"],
    ["LINK LOST", "连接断开"],
    ["MOVE", "久坐提醒"],
    ["LOW POWER", "低电量"],
    ["CAMERA", "遥控拍照"],
    ["TIME UP", "倒计时结束"],
    ["HR ALERT", "心率提醒"],
    ["CUSTOM", "自定义时间"],
    ["TIMER", "倒计时控制"],
    ["TIME UP", "倒计时结束"],
    ["SPORT TYPE", "运动类型"],
    ["READY", "准备运动"],
    ["WORKOUT", "运动数据"],
    ["CONTROL", "运动控制"],
    ["END SPORT", "结束运动"],
    ["MESSAGE", "消息详情"],
    ["PAIR CODE", "配对码"],
    ["GUIDE", "测量须知"],
    ["MEASURE", "测量确认"],
    ["THEME", "配色"],
    ["FORMAT", "时间制式"],
    ["REQUEST", "请求反馈"],
    ["START", "开始"],
    ["STOP", "停止"],
    ["PAUSE", "暂停"],
    ["RESUME", "继续"],
    ["APPLY", "确定"],
    ["CANCEL", "取消"],
    ["YES", "确认"],
    ["BACK", "返回"],
    ["DEMO", "演示"],
    ["NO DATA", "暂无数据"],
    ["MEASURING", "测量中"],
    ["RESULT", "测量结果"],
    ["TAP TO START", "点击开始测量"],
    ["UNAVAILABLE", "暂不可用"],
    ["WAITING", "等待数据"],
    ["RETRY", "测量失败重试"],
    ["HIGH", "最高"],
    ["LOW", "最低"],
    ["NO ACTION RUN", "未执行设备动作"],
    ["REQUEST ONLY", "仅发出请求"],
    ["CLEAR DATA?", "确认清除数据"],
    ["POWER OFF?", "确认关机"],
    ["RESTART?", "确认重启设备"],
    ["LOCK?", "确认锁屏"],
    ["SOURCE STUB", "原工程为空项"],
    ["NO SERVICE", "未接入服务"],
    ["CUSTOM", "自定义"],
    ["LAP", "计次"],
    ["RESET", "清零"],
    ["NEXT", "下一曲"],
    ["PREV", "上一曲"],
    ["PLAY", "播放"],
    ["VOLUME", "音量"],
    ["DEMO MESSAGE", "演示消息内容"],
    ["SMALL", "小字"],
    ["LARGE", "大字"],
    ["STEPS", "步数"],
    ["DISTANCE", "距离"],
    ["KCAL", "热量"],
    ["DEEP", "深睡"],
    ["LIGHT", "浅睡"],
    ["TOTAL", "总时长"],
    ["RUNNING", "运行中"],
    ["PAUSED", "已暂停"],
    ["STOPPED", "未开始"],
    ["TIME", "时长"],
    ["ANSWER", "接听"],
    ["REJECT", "拒接"],
    ["DISMISS", "关闭提醒"],
    ["SHOOT", "拍照"],
    ["READY", "就绪"],
    ["MODEL", "型号"],
    ["VERSION", "固件版本"],
    ["PAIR", "配对"],
    ["INSTALLED", "安装表盘"],
    ["CYCLING", "骑行"],
    ["HIKING", "登山"],
    ["BADMINTON", "羽毛球"],
    ["RUNNING", "跑步"],
    ["END", "结束"],
    ["LOCKED", "已锁定"],
    ["HOLD KEY", "长按侧键解锁"],
    ["FOLLOW GUIDE", "按说明操作"],
    ["START MEASURE?", "是否开始测量"],
    ["CHINESE", "简体中文"],
    ["ENGLISH", "英文"],
    ["REMAINING", "剩余时间"],
    ["MAX 20 LAPS", "最多二十条计次"],
    ["LOCALE REQUEST", "该语言尚未接入"],
    ["NO QR DATA", "暂无二维码数据"],
    ["MON", "周一"],
    ["TUE", "周二"],
    ["WED", "周三"],
    ["THU", "周四"],
    ["FRI", "周五"],
    ["SAT", "周六"],
    ["SUN", "周日"],
    ["SUNNY", "晴"],
    ["CLOUDY", "多云"],
    ["RAIN", "雨"],
    ["SNOW", "雪"],
    ["SMOG", "雾霾"],
    ["SAND", "扬沙"],
    ["UNKNOWN", "未知"],
    ["LOW / HIGH", "最低 / 最高"],
    ["NEXT", "下一条"],
];
impl Label {
    /// Fixed copy for an implemented locale.
    #[must_use]
    pub const fn text(self, locale: Locale) -> &'static str {
        COPY[self as usize][match locale {
            Locale::Zh => 1,
            Locale::En => 0,
        }]
    }
}
pub(crate) const fn title(s: Screen) -> Label {
    match s {
        Screen::Boot | Screen::Dial => Label::Boot,
        Screen::Activity => Label::Activity,
        Screen::Sport => Label::Sport,
        Screen::Heart => Label::Heart,
        Screen::Pressure => Label::Pressure,
        Screen::Oxygen => Label::Oxygen,
        Screen::Sleep => Label::Sleep,
        Screen::Alarm => Label::Alarm,
        Screen::Stopwatch => Label::Stopwatch,
        Screen::Findphone => Label::Findphone,
        Screen::About => Label::About,
        Screen::Power => Label::Power,
        Screen::Messages => Label::Messages,
        Screen::Weather => Label::Weather,
        Screen::Settings => Label::Settings,
        Screen::Ota => Label::Ota,
        Screen::Face => Label::Face,
        Screen::Notification => Label::Notification,
        Screen::Tools => Label::Tools,
        Screen::Quick => Label::Quick,
        Screen::Brightness => Label::Brightness,
        Screen::Clear => Label::Clear,
        Screen::Timer => Label::Timer,
        Screen::Reset => Label::Reset,
        Screen::Music => Label::Music,
        Screen::Language => Label::Language,
        Screen::Temperature => Label::Temperature,
        Screen::Shutdown => Label::Shutdown,
        Screen::Restart => Label::Restart,
        Screen::Textsize => Label::Textsize,
        Screen::Charge => Label::Charge,
        Screen::Lock => Label::Lock,
        Screen::Unlock => Label::Unlock,
        Screen::Applet => Label::Applet,
        Screen::Glucose => Label::Glucose,
        Screen::Hba1c => Label::Hba1c,
        Screen::Call => Label::Call,
        Screen::NoticeApp => Label::NoticeApp,
        Screen::NoticeAlarm => Label::NoticeAlarm,
        Screen::NoticeFind => Label::NoticeFind,
        Screen::NoticeDisconnect => Label::NoticeDisconnect,
        Screen::NoticeMove => Label::NoticeMove,
        Screen::NoticeBattery => Label::NoticeBattery,
        Screen::Camera => Label::Camera,
        Screen::NoticeTimer => Label::NoticeTimer,
        Screen::NoticeHeart => Label::NoticeHeart,
        Screen::TimerCustom => Label::TimerCustom,
        Screen::TimerRun => Label::TimerRun,
        Screen::TimerDone => Label::TimerDone,
        Screen::SportList => Label::SportList,
        Screen::SportCountdown => Label::SportCountdown,
        Screen::SportRun => Label::SportRun,
        Screen::SportControls => Label::SportControls,
        Screen::SportEnd => Label::SportEnd,
        Screen::MessageDetail => Label::MessageDetail,
        Screen::DeviceQr => Label::DeviceQr,
        Screen::MeasureTips => Label::MeasureTips,
        Screen::MeasureConfirm => Label::MeasureConfirm,
        Screen::Theme => Label::Theme,
        Screen::Format => Label::Format,
        Screen::Request => Label::Request,
    }
}
