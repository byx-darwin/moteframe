//! Page-specific integer layouts, using the bounded canvas.
use crate::{
    Label as L, Locale, MeasureKind, MeasureState, Model, Reading, Screen, Snapshot, SportPhase,
    StopwatchState, TimerState, UiRequest,
    canvas::{Canvas, RenderError, Text},
    labels::title,
    model::{measure, measure_screen},
};
use core::fmt::Write;
use device_ui::graphics::{pixelcolor::Rgb565, prelude::*};
fn request_name(r: UiRequest) -> &'static str {
    match r {
        UiRequest::PowerOff => "POWEROFF",
        UiRequest::Restart => "RESTART",
        UiRequest::ClearData => "CLEARDATA",
        UiRequest::Lock => "LOCK",
        UiRequest::SportStop => "SPORTSTOP",
        UiRequest::LanguageRequested(_) => "LANGUAGEREQUESTED",
        UiRequest::InstalledFaceSelected => "INSTALLEDFACESELECTED",
        _ => "REQUEST",
    }
}
impl<D: DrawTarget<Color = Rgb565>, const PRODUCT: bool> Canvas<'_, D, PRODUCT> {
    // Page bodies follow base/chrome drawing. Their formatting temporaries
    // must not stay live while a different base layout draws its clock face.
    #[inline(never)]
    pub(crate) fn content(
        &mut self,
        model: &Model,
        data: &Snapshot<'_>,
    ) -> Result<(), RenderError<D::Error>> {
        use Screen as S;
        if let Some(kind) = measure(model.page) {
            return self.measure_page(model, data, kind);
        }
        match model.page {
            S::Brightness | S::Theme | S::Format => self.primary(model, L::Apply),
            S::Face => {
                if model.draft == 3 {
                    self.lc(model, L::Installed, 41, 1, 1)?;
                    self.lc(model, L::RequestOnly, 70, 1, 3)?;
                }
                self.primary(model, L::Apply)
            }
            S::Activity | S::Sleep | S::Weather | S::Charge | S::Ota | S::Applet => {
                self.data_page(model, data)
            }
            S::Messages | S::MessageDetail | S::NoticeApp => self.message_page(model, data),
            S::Timer | S::TimerCustom | S::TimerRun | S::Stopwatch => self.time_page(model, data),
            S::Textsize | S::Language | S::About | S::DeviceQr => self.settings_page(model, data),
            S::Clear | S::Restart | S::Power | S::Lock | S::SportEnd => {
                let id = match model.page {
                    S::Clear => L::ClearQuestion,
                    S::Restart => L::RestartQuestion,
                    S::Power => L::PowerQuestion,
                    S::Lock => L::LockQuestion,
                    _ => L::End,
                };
                self.lc(model, id, 38, 1, 1)?;
                self.lc(model, L::RequestOnly, 67, 1, 3)?;
                self.pair(model, L::Cancel, L::Yes)
            }
            S::Sport | S::SportCountdown | S::SportRun | S::SportControls => {
                self.sport_page(model, data)
            }
            _ => self.event_page(model),
        }
    }
    fn measure_page(
        &mut self,
        model: &Model,
        data: &Snapshot<'_>,
        kind: MeasureKind,
    ) -> Result<(), RenderError<D::Error>> {
        let value = data.measurement(kind);
        let mut text = Text::default();
        match if matches!(value.state, MeasureState::Idle | MeasureState::Unavailable) {
            Reading::Missing
        } else {
            value.reading
        } {
            Reading::Missing => text.write_str(if kind == MeasureKind::Pressure {
                "--/--"
            } else {
                "--"
            }),
            Reading::Number(n) => write!(&mut text, "{n}"),
            Reading::Tenth(n) => write!(&mut text, "{}.{}", n / 10, n % 10),
            Reading::Pressure { upper, lower } => write!(&mut text, "{upper}/{lower}"),
        }
        .map_err(|_| RenderError::Text)?;
        self.center(
            text.value().map_err(|_| RenderError::Text)?,
            31,
            if kind == MeasureKind::Pressure { 3 } else { 4 },
            1,
        )?;
        self.center(
            ["BPM", "MMHG", "%", "C", "MMOL/L", "%"][kind as usize],
            67,
            1,
            2,
        )?;
        let state = match value.state {
            MeasureState::Idle => L::Idle,
            MeasureState::Pending => L::Pending,
            MeasureState::Measuring => L::Measuring,
            MeasureState::Result => L::Result,
            MeasureState::Error => L::Error,
            MeasureState::Unavailable => L::Unavailable,
        };
        self.lc(model, state, 82, 1, 3)?;
        if kind == MeasureKind::Heart {
            self.optional_label(model, L::High, data.heart_high, 8, 109, 3)?;
            self.optional_label(model, L::Low, data.heart_low, 74, 109, 3)
        } else {
            self.lc(
                model,
                if value.state == MeasureState::Measuring {
                    L::Stop
                } else {
                    L::Start
                },
                108,
                1,
                2,
            )
        }
    }
    fn optional_label(
        &mut self,
        model: &Model,
        id: L,
        value: Option<u16>,
        x: i32,
        y: i32,
        color_index: usize,
    ) -> Result<(), RenderError<D::Error>> {
        let mut text = Text::default();
        text.write_str(id.text(model.appearance.locale))
            .map_err(|_| RenderError::Text)?;
        text.write_str(" ").map_err(|_| RenderError::Text)?;
        if let Some(value) = value {
            write!(&mut text, "{value}").map_err(|_| RenderError::Text)?;
        } else {
            text.write_str("--").map_err(|_| RenderError::Text)?;
        }
        self.text(
            text.value().map_err(|_| RenderError::Text)?,
            x,
            y,
            1,
            color_index,
        )
    }
    fn label_text(
        &mut self,
        label: &str,
        value: &str,
        x: i32,
        y: i32,
        color_index: usize,
    ) -> Result<(), RenderError<D::Error>> {
        let mut text = Text::default();
        text.write_str(label).map_err(|_| RenderError::Text)?;
        text.write_str(" ").map_err(|_| RenderError::Text)?;
        text.write_str(value).map_err(|_| RenderError::Text)?;
        self.text(
            text.value().map_err(|_| RenderError::Text)?,
            x,
            y,
            1,
            color_index,
        )
    }
    fn data_page(
        &mut self,
        model: &Model,
        data: &Snapshot<'_>,
    ) -> Result<(), RenderError<D::Error>> {
        use Screen as S;
        match model.page {
            S::Activity if PRODUCT => {
                self.center("--", 27, 3, 1)?;
                self.lc(model, L::Steps, 56, 1, 2)?;
                self.optional_label(model, L::Heart, data.activity_heart, 8, 77, 1)?;
                self.optional_label(model, L::Distance, None, 8, 95, 1)?;
                self.optional_label(model, L::Calories, None, 8, 112, 3)
            }
            S::Activity => {
                self.formatted(format_args!("{}", data.steps), None, 27, 3, 1)?;
                self.lc(model, L::Steps, 56, 1, 2)?;
                self.optional_label(model, L::Heart, data.activity_heart, 8, 77, 1)?;
                self.formatted(
                    format_args!(
                        "{} {}.{:02}",
                        L::Distance.text(model.appearance.locale),
                        data.distance / 100,
                        data.distance % 100
                    ),
                    Some(8),
                    95,
                    1,
                    1,
                )?;
                self.formatted(
                    format_args!(
                        "{} {}",
                        L::Calories.text(model.appearance.locale),
                        data.calories
                    ),
                    Some(8),
                    112,
                    1,
                    3,
                )
            }
            S::Sleep => self.sleep_page(model, data),
            S::Weather => {
                self.lc(model, L::RangeLabel, 29, 1, 3)?;
                if let Some(w) = data.weather {
                    self.formatted(format_args!("{}/{}", w.low, w.high), None, 49, 2, 1)?;
                } else {
                    self.center("--/--", 49, 2, 1)?;
                }
                self.center("C", 72, 1, 2)?;
                let id = data.weather.map_or(L::NoData, |w| {
                    [
                        L::WeatherFine,
                        L::WeatherCloudy,
                        L::WeatherRain,
                        L::WeatherSnow,
                        L::WeatherSmog,
                        L::WeatherSand,
                        L::WeatherUnknown,
                    ][w.condition as usize]
                });
                self.lc(model, id, 89, 1, 1)?;
                if PRODUCT {
                    return self.center("--/-- --", 112, 1, 3);
                }
                self.formatted(
                    format_args!(
                        "{:02}/{:02} {}",
                        data.month,
                        data.day,
                        weekday(data.weekday)
                    ),
                    None,
                    112,
                    1,
                    3,
                )
            }
            S::Charge if PRODUCT => {
                self.center("--%", 33, 3, 1)?;
                self.lc(model, L::Unavailable, 91, 1, 3)
            }
            S::Charge => {
                self.formatted(format_args!("{}%", data.charging), None, 33, 3, 1)?;
                self.lc(model, L::RequestOnly, 91, 1, 3)
            }
            _ if PRODUCT => {
                self.center("--%", 35, 3, 1)?;
                self.rect(8, 80, 112, 6, 3)?;
                self.lc(model, L::Unavailable, 103, 1, 3)
            }
            _ => {
                self.formatted(format_args!("{}%", data.progress), None, 35, 3, 1)?;
                self.rect(8, 80, 112, 6, 3)?;
                self.rect(8, 80, 112 * u32::from(data.progress) / 100, 6, 2)?;
                self.lc(model, L::Demo, 103, 1, 3)
            }
        }
    }
    fn sleep_page(
        &mut self,
        model: &Model,
        data: &Snapshot<'_>,
    ) -> Result<(), RenderError<D::Error>> {
        let mut text = Text::default();
        sleep_text(&mut text, data.sleep_total)?;
        self.center(text.value().map_err(|_| RenderError::Text)?, 31, 4, 1)?;
        self.lc(model, L::Total, 68, 1, 2)?;
        for (id, value, y, color_index) in [
            (L::Deep, data.sleep_deep, 89, 1),
            (L::Light, data.sleep_light, 108, 3),
        ] {
            let mut text = Text::default();
            text.write_str(id.text(model.appearance.locale))
                .map_err(|_| RenderError::Text)?;
            text.write_str(" ").map_err(|_| RenderError::Text)?;
            sleep_text(&mut text, value)?;
            self.text(
                text.value().map_err(|_| RenderError::Text)?,
                8,
                y,
                1,
                color_index,
            )?;
        }
        Ok(())
    }
    fn time_page(
        &mut self,
        model: &Model,
        data: &Snapshot<'_>,
    ) -> Result<(), RenderError<D::Error>> {
        use Screen as S;
        if (model.page == S::TimerRun && data.timer.state == TimerState::Unavailable)
            || (model.page == S::Stopwatch && data.stopwatch.state == StopwatchState::Unavailable)
        {
            self.lc(model, L::NoData, 39, 2, 1)?;
            self.lc(model, L::Unavailable, 72, 1, 3)?;
            return self.pair(
                model,
                if model.page == S::TimerRun {
                    L::Stop
                } else {
                    L::ResetAction
                },
                L::Start,
            );
        }
        match model.page {
            S::Timer => {
                for (i, n) in [1, 3, 5, 10].into_iter().enumerate() {
                    let x = if i % 2 == 0 { 5 } else { 67 };
                    let y = if i < 2 { 25 } else { 62 };
                    self.rect(x, y, 56, 32, 3)?;
                    self.formatted(format_args!("{n} MIN"), Some(x + 7), y + 9, 1, 0)?;
                }
                self.primary(model, L::Custom)
            }
            S::TimerCustom => {
                self.formatted(
                    format_args!(
                        "{:02}:{:02}:{:02}",
                        model.timer_hms[0], model.timer_hms[1], model.timer_hms[2]
                    ),
                    Some(17),
                    48,
                    2,
                    1,
                )?;
                for i in 0..3 {
                    self.text("-", 27 + 36 * i, 29, 1, 2)?;
                    self.text("+", 27 + 36 * i, 79, 1, 2)?;
                }
                self.rect(18 + i32::from(model.timer_field) * 36, 72, 23, 2, 2)?;
                self.primary(model, L::Apply)
            }
            S::TimerRun => {
                let seconds = if data.timer.state == TimerState::Idle {
                    model.duration()
                } else {
                    data.timer.seconds
                };
                self.hms(seconds, 37)?;
                self.lc(
                    model,
                    match data.timer.state {
                        TimerState::Running => L::Running,
                        TimerState::Paused => L::Paused,
                        _ => L::Stopped,
                    },
                    72,
                    1,
                    3,
                )?;
                self.pair(
                    model,
                    L::Stop,
                    if data.timer.state == TimerState::Running {
                        L::Pause
                    } else {
                        L::Start
                    },
                )
            }
            _ => self.stopwatch_page(model, data),
        }
    }
    fn stopwatch_page(
        &mut self,
        model: &Model,
        data: &Snapshot<'_>,
    ) -> Result<(), RenderError<D::Error>> {
        let color_index = data.stopwatch.centis;
        self.formatted(
            format_args!(
                "{:02}:{:02}.{:02}",
                color_index / 6000,
                (color_index / 100) % 60,
                color_index % 100
            ),
            None,
            29,
            2,
            1,
        )?;
        for (i, color_index) in data
            .stopwatch
            .laps
            .iter()
            .skip(model.lap_top)
            .take(2)
            .enumerate()
        {
            self.formatted(
                format_args!(
                    "{:02} {:02}:{:02}.{:02}",
                    i + model.lap_top + 1,
                    color_index / 6000,
                    (color_index / 100) % 60,
                    color_index % 100
                ),
                Some(8),
                57 + i32::try_from(i).map_err(|_| RenderError::Bounds)? * 19,
                1,
                3,
            )?;
        }
        self.pair(
            model,
            if data.stopwatch.state == StopwatchState::Running {
                L::Lap
            } else {
                L::ResetAction
            },
            if data.stopwatch.state == StopwatchState::Running {
                L::Pause
            } else {
                L::Start
            },
        )
    }

    fn hms(&mut self, text: u32, y: i32) -> Result<(), RenderError<D::Error>> {
        self.formatted(
            format_args!(
                "{:02}:{:02}:{:02}",
                text / 3600,
                (text / 60) % 60,
                text % 60
            ),
            None,
            y,
            2,
            1,
        )
    }
    fn settings_page(
        &mut self,
        model: &Model,
        data: &Snapshot<'_>,
    ) -> Result<(), RenderError<D::Error>> {
        use Screen as S;
        match model.page {
            S::Textsize => {
                if model.appearance.locale == Locale::Zh {
                    self.center("文字", 32, if model.draft == 0 { 1 } else { 2 }, 1)?;
                } else {
                    self.center("ABC", 36, if model.draft == 0 { 2 } else { 4 }, 1)?;
                }
                self.lc(
                    model,
                    if model.draft == 0 { L::Small } else { L::Large },
                    76,
                    1,
                    2,
                )?;
                self.primary(model, L::Apply)
            }
            S::Language => {
                let a = [
                    "简体中文",
                    "ENGLISH",
                    "FRANCAIS",
                    "DEUTSCH",
                    "ITALIANO",
                    "PORTUGUESE",
                    "RUSSIAN",
                    "SPANISH",
                    "DUTCH",
                ];
                self.center(a[usize::from(model.draft)], 39, 1, 1)?;
                self.formatted(format_args!("{}/9", model.draft + 1), None, 70, 1, 3)?;
                if model.draft > 1 {
                    self.lc(model, L::LanguagePending, 84, 1, 3)?;
                }
                self.primary(model, L::Apply)
            }
            S::About => {
                self.label_text(
                    L::Info.text(model.appearance.locale),
                    data.info.model.unwrap_or("--"),
                    8,
                    28,
                    1,
                )?;
                self.label_text(
                    L::Firmware.text(model.appearance.locale),
                    data.info.version.unwrap_or("--"),
                    8,
                    49,
                    1,
                )?;
                self.label_text("MAC", data.info.address.unwrap_or("--"), 8, 70, 1)?;
                self.lc(model, L::Pair, 104, 1, 2)
            }
            _ => {
                if let Some(q) = data.qr {
                    self.rect(18, 23, 92, 92, 1)?;
                    for (y, row) in q.iter().enumerate() {
                        for (x, value) in row.iter().enumerate() {
                            if *value {
                                self.rect(
                                    22 + i32::try_from(x).map_err(|_| RenderError::Bounds)? * 4,
                                    27 + i32::try_from(y).map_err(|_| RenderError::Bounds)? * 4,
                                    4,
                                    4,
                                    0,
                                )?;
                            }
                        }
                    }
                    Ok(())
                } else {
                    self.lc(model, L::NoQr, 48, 1, 3)
                }
            }
        }
    }
    fn sport_page(
        &mut self,
        model: &Model,
        data: &Snapshot<'_>,
    ) -> Result<(), RenderError<D::Error>> {
        use Screen as S;
        match model.page {
            S::Sport => {
                self.lc(model, L::Sport, 39, 2, 1)?;
                self.primary(model, L::Start)
            }
            S::SportCountdown if PRODUCT => {
                self.center("--", 30, 6, 1)?;
                self.lc(model, L::Unavailable, 91, 1, 3)
            }
            S::SportCountdown => {
                self.formatted(format_args!("{}", data.sport.countdown), None, 30, 6, 1)?;
                self.lc(model, L::RequestOnly, 91, 1, 3)
            }
            S::SportRun if PRODUCT => {
                self.center("--:--:--", 27, 2, 1)?;
                self.lc(model, L::Unavailable, 50, 1, 2)?;
                self.optional_label(model, L::Distance, None, 8, 70, 1)?;
                self.optional_label(model, L::Heart, None, 8, 89, 1)?;
                self.optional_label(model, L::Calories, None, 8, 108, 3)
            }
            S::SportRun => {
                let text = data.sport;
                self.hms(text.elapsed_seconds, 27)?;
                self.lc(
                    model,
                    if text.phase == SportPhase::Paused {
                        L::Paused
                    } else {
                        L::Running
                    },
                    50,
                    1,
                    2,
                )?;
                self.formatted(
                    format_args!(
                        "{} {}.{:02}",
                        L::Distance.text(model.appearance.locale),
                        text.distance / 100,
                        text.distance % 100
                    ),
                    Some(8),
                    70,
                    1,
                    1,
                )?;
                self.optional_label(model, L::Heart, text.heart, 8, 89, 1)?;
                self.formatted(
                    format_args!(
                        "{} {}",
                        L::Calories.text(model.appearance.locale),
                        text.calories
                    ),
                    Some(8),
                    108,
                    1,
                    3,
                )
            }
            _ => {
                for (i, id) in [
                    if data.sport.locked {
                        L::Locked
                    } else {
                        L::Lock
                    },
                    L::End,
                    L::Back,
                    if data.sport.phase == SportPhase::Paused {
                        L::Resume
                    } else {
                        L::Pause
                    },
                ]
                .into_iter()
                .enumerate()
                {
                    let x = if i % 2 == 0 { 5 } else { 67 };
                    let y = if i < 2 { 25 } else { 68 };
                    self.rect(x, y, 56, 38, 3)?;
                    let text = id.text(model.appearance.locale);
                    self.text(
                        text,
                        x + (56 - Self::width(text, 1)?).div_euclid(2),
                        y + 12,
                        1,
                        0,
                    )?;
                }
                Ok(())
            }
        }
    }
    fn event_page(&mut self, model: &Model) -> Result<(), RenderError<D::Error>> {
        use Screen as S;
        match model.page {
            S::Music => {
                for (i, id) in [L::Prev, L::Play, L::Next].into_iter().enumerate() {
                    let i = i32::try_from(i).map_err(|_| RenderError::Bounds)?;
                    self.rect(4 + 41 * i, 30, 38, 32, if i == 1 { 2 } else { 3 })?;
                    let text = id.text(model.appearance.locale);
                    self.text(
                        text,
                        5 + 41 * i + (36 - Self::width(text, 1)?).div_euclid(2),
                        40,
                        1,
                        0,
                    )?;
                }
                self.text("-", 17, 82, 2, 2)?;
                self.lc(model, L::Vol, 82, 1, 1)?;
                self.text("+", 102, 82, 2, 2)
            }
            S::Call if PRODUCT => {
                self.center("--", 35, 2, 1)?;
                self.center("--", 63, 1, 3)?;
                self.pair(model, L::Reject, L::Accept)
            }
            S::Call => {
                self.center("DEMO", 35, 2, 1)?;
                self.center("000000", 63, 1, 3)?;
                self.pair(model, L::Reject, L::Accept)
            }
            S::Camera => self.camera_page(model),
            S::Unlock => {
                self.lc(model, L::Locked, 38, 1, 1)?;
                self.lc(model, L::HoldKey, 72, 1, 3)
            }
            S::Alarm | S::Reset | S::NoticeDisconnect => {
                self.lc(model, L::SourceStub, 45, 1, 1)?;
                self.lc(model, L::NoService, 72, 1, 3)
            }
            S::MeasureTips => {
                self.lc(model, title(measure_screen(model.measure_kind)), 30, 1, 1)?;
                self.lc(model, L::Safety, 55, 1, 3)?;
                self.primary(model, L::Next)
            }
            S::MeasureConfirm => {
                self.lc(model, L::MeasureAsk, 39, 1, 1)?;
                self.lc(model, L::RequestOnly, 67, 1, 3)?;
                self.pair(model, L::Cancel, L::Yes)
            }
            S::Request => {
                self.lc(model, L::NotRun, 34, 1, 2)?;
                let text = model.request.map_or("", request_name);
                let split = text.len().min(18);
                self.text(text.get(..split).ok_or(RenderError::Text)?, 8, 57, 1, 1)?;
                if text.len() > 18 {
                    self.text(
                        text.get(18..text.len().min(36)).ok_or(RenderError::Text)?,
                        8,
                        71,
                        1,
                        1,
                    )?;
                }
                self.primary(model, L::Back)
            }
            S::Boot | S::Shutdown => {
                self.center("WATTCH", 35, 3, 1)?;
                self.center("5", 67, 4, 2)?;
                if PRODUCT {
                    Ok(())
                } else {
                    self.lc(model, L::Demo, 113, 1, 3)
                }
            }
            S::Findphone => {
                self.lc(model, L::Findphone, 43, 1, 1)?;
                self.primary(model, L::Start)
            }
            _ => {
                self.lc(model, title(model.page), 37, 1, 1)?;
                self.lc(
                    model,
                    if PRODUCT { L::Unavailable } else { L::Demo },
                    65,
                    1,
                    3,
                )?;
                self.primary(
                    model,
                    if model.page == S::TimerDone {
                        L::Back
                    } else {
                        L::Dismiss
                    },
                )
            }
        }
    }
    fn camera_page(&mut self, model: &Model) -> Result<(), RenderError<D::Error>> {
        self.lc(
            model,
            if PRODUCT { L::Unavailable } else { L::Ready },
            44,
            if PRODUCT { 1 } else { 2 },
            1,
        )?;
        self.primary(model, L::Shoot)
    }
    fn message_page(
        &mut self,
        model: &Model,
        data: &Snapshot<'_>,
    ) -> Result<(), RenderError<D::Error>> {
        if data.messages.is_empty() {
            self.lc(model, L::NoData, 47, 1, 3)?;
        } else if model.page == Screen::Messages {
            self.formatted(format_args!("{}", data.messages.len()), None, 30, 4, 1)?;
            self.lc(model, L::Messages, 77, 1, 2)?;
        } else {
            let body = data
                .messages
                .get(model.message_index)
                .ok_or(RenderError::Bounds)?;
            let text = if model.appearance.locale == Locale::Zh {
                body.zh
            } else {
                body.en
            };
            self.message_text(model, text)?;
            if model.appearance.locale == Locale::Zh {
                self.formatted(
                    format_args!("{}/{}", model.message_index + 1, data.messages.len()),
                    None,
                    83,
                    1,
                    3,
                )?;
            }
        }
        if model.page != Screen::Messages {
            self.primary(
                model,
                if model.page == Screen::MessageDetail
                    && model.message_index + 1 < data.messages.len()
                {
                    L::NextMessage
                } else {
                    L::Back
                },
            )?;
        }
        Ok(())
    }
    fn message_text(&mut self, model: &Model, text: &str) -> Result<(), RenderError<D::Error>> {
        if model.appearance.locale == Locale::Zh {
            if model.appearance.textsize == 0 {
                self.center(text, 37, 1, 1)
            } else {
                let end = text.char_indices().nth(4).map_or(text.len(), |(i, _)| i);
                self.text(text.get(..end).ok_or(RenderError::Text)?, 16, 28, 2, 1)?;
                self.text(text.get(end..).ok_or(RenderError::Text)?, 16, 56, 2, 1)
            }
        } else {
            let size = if model.appearance.textsize == 0 { 1 } else { 2 };
            let lines = crate::text_layout::lines(text, size).map_err(RenderError::Snapshot)?;
            for (i, line) in lines.into_iter().enumerate() {
                if !line.is_empty() {
                    self.text(
                        line,
                        8,
                        34 + i32::try_from(i).map_err(|_| RenderError::Bounds)? * 22,
                        i32::try_from(size).map_err(|_| RenderError::Bounds)?,
                        1,
                    )?;
                }
            }

            Ok(())
        }
    }
}
fn weekday(value: crate::Weekday) -> &'static str {
    ["MON", "TUE", "WED", "THU", "FRI", "SAT", "SUN"][value as usize]
}
fn sleep_text<E>(out: &mut Text, value: Option<u16>) -> Result<(), RenderError<E>> {
    if let Some(value) = value {
        write!(out, "{:02}:{:02}", value / 60, value % 60)
    } else {
        out.write_str("--")
    }
    .map_err(|_| RenderError::Text)
}
