//! State transitions with explicit requests and no service side effects.
use super::{Model, field, horizontal, measure, measure_screen, menu};
use crate::{
    Effect, Event, Face, Field, Language, Locale, MeasureKind, MeasureState, Screen, Snapshot,
    SportKind, SportPhase, StopwatchState, Theme, TimeFormat, TimerState, UiError, UiRequest,
};
macro_rules! requested {
    ($r:expr) => {
        Ok(Effect::Request($r))
    };
}
fn timer_action(d: &Snapshot<'_>) -> UiRequest {
    match d.timer.state {
        TimerState::Running => UiRequest::TimerPause,
        TimerState::Paused => UiRequest::TimerResume,
        _ => UiRequest::TimerStart,
    }
}
fn watch_action(d: &Snapshot<'_>) -> UiRequest {
    match d.stopwatch.state {
        StopwatchState::Running => UiRequest::StopwatchPause,
        StopwatchState::Paused => UiRequest::StopwatchResume,
        _ => UiRequest::StopwatchStart,
    }
}
fn selected(e: Event) -> bool {
    matches!(e, Event::Select | Event::Tap { .. })
}
impl Model {
    pub(super) fn apply(&mut self, e: Event, d: &Snapshot<'_>) -> Result<Effect, UiError> {
        use Screen as S;
        use UiRequest as R;
        if e == Event::KeyLongPress {
            return match self.page {
                S::Unlock => requested!(R::UnlockRequested),
                S::Dial => self.feedback(R::PowerOff),
                _ => Ok(Effect::None),
            };
        }
        if e == Event::KeyRelease {
            return match self.page {
                S::Stopwatch => requested!(watch_action(d)),
                S::TimerRun => requested!(timer_action(d)),
                S::Dial => requested!(R::DisplayToggle),
                _ => {
                    let r = self.restore();
                    self.home();
                    Ok(r)
                }
            };
        }
        if self.page == S::Face {
            return self.face_event(e, d);
        }
        if matches!(e, Event::Left | Event::Right)
            && let Some(s) = horizontal(self.page, e == Event::Left)
        {
            self.go(s, false)?;
            return Ok(Effect::None);
        }
        if e == Event::Right {
            let r = self.restore();
            match self.page {
                S::Request => self.close_request(),
                S::MessageDetail => {
                    self.page = S::Sleep;
                    self.depth = 0;
                }
                S::TimerRun | S::TimerCustom | S::TimerDone => {
                    self.page = S::Timer;
                    self.timer_field = 0;
                }
                S::DeviceQr => self.page = S::About,
                S::SportControls => self.page = S::SportRun,
                S::SportRun => {
                    if d.sport.locked {
                        return requested!(R::SportUnlockRequested);
                    }
                    self.page = S::SportControls;
                }
                _ => self.back(),
            }
            return Ok(r);
        }
        if menu(self.page).is_some() || self.page == S::SportList {
            return self.menu_event(e);
        }
        if let Some(f) = field(self.page) {
            return self.edit_event(e, f);
        }
        if let Some(k) = measure(self.page) {
            if selected(e) {
                if matches!(k, MeasureKind::Glucose | MeasureKind::Hba1c)
                    && d.measurement(k).state == MeasureState::Idle
                {
                    self.go(S::MeasureTips, true)?;
                } else {
                    return requested!(if d.measurement(k).state == MeasureState::Measuring {
                        R::MeasureStop(k)
                    } else {
                        R::MeasureStart(k)
                    });
                }
            }
            return Ok(Effect::None);
        }
        self.page_event(e, d)
    }
    fn page_event(&mut self, e: Event, d: &Snapshot<'_>) -> Result<Effect, UiError> {
        use Screen as S;
        use UiRequest as R;
        match self.page {
            S::Dial => match e {
                Event::Up => self.go(S::Tools, true)?,
                Event::Down => self.go(S::Quick, true)?,
                Event::LongPress => self.go(S::Face, true)?,
                _ => {}
            },
            S::About | S::DeviceQr if selected(e) || e == Event::Left => {
                self.page = if self.page == S::About {
                    S::DeviceQr
                } else {
                    S::About
                }
            }
            S::MeasureTips if selected(e) => self.go(S::MeasureConfirm, true)?,
            S::MeasureConfirm => {
                if let Event::Tap { x, y: 96.. } = e {
                    if x < 64 {
                        self.back();
                    } else {
                        let r = R::MeasureStart(self.measure_kind);
                        while self.page != measure_screen(self.measure_kind) && self.depth > 0 {
                            self.back();
                        }
                        return requested!(r);
                    }
                }
            }
            S::Timer | S::TimerCustom | S::TimerRun | S::TimerDone | S::Stopwatch => {
                return self.time_event(e, d);
            }
            S::Messages | S::MessageDetail => return self.message_event(e, d),
            S::Clear | S::Restart | S::Power | S::Lock | S::SportEnd => {
                return self.confirm_event(e);
            }
            S::Sport | S::SportCountdown | S::SportRun | S::SportControls => {
                return self.sport_event(e, d);
            }
            S::Music => {
                if e == Event::Select {
                    return requested!(R::MusicPlayPause);
                }
                if let Event::Tap { x, y } = e {
                    return requested!(if y < 58 {
                        if x < 43 {
                            R::MusicPrevious
                        } else if x > 85 {
                            R::MusicNext
                        } else {
                            R::MusicPlayPause
                        }
                    } else if x < 64 {
                        R::VolumeDown
                    } else {
                        R::VolumeUp
                    });
                }
            }
            S::Call => {
                if let Event::Tap { x, y: 96.. } = e {
                    return requested!(if x < 64 { R::CallReject } else { R::CallAnswer });
                }
            }
            S::Camera if selected(e) => return requested!(R::CameraShutter),
            S::Findphone if selected(e) => return requested!(R::FindPhoneToggle),
            S::Request if selected(e) => self.close_request(),
            S::Notification
            | S::NoticeApp
            | S::NoticeAlarm
            | S::NoticeFind
            | S::NoticeDisconnect
            | S::NoticeMove
            | S::NoticeBattery
            | S::NoticeTimer
            | S::NoticeHeart
                if selected(e) =>
            {
                let p = self.page;
                self.back();
                return requested!(R::NoticeDismiss(p));
            }
            _ => {}
        }
        Ok(Effect::None)
    }
    fn face_event(&mut self, e: Event, d: &Snapshot<'_>) -> Result<Effect, UiError> {
        match e {
            Event::Left => {
                self.draft = (self.draft + 1).min(if d.applet_available { 3 } else { 2 });
            }
            Event::Right => self.draft = self.draft.saturating_sub(1),
            e if selected(e) => {
                if self.draft == 3 {
                    return self.feedback(UiRequest::InstalledFaceSelected);
                }
                self.appearance.face =
                    [Face::Stack, Face::Line, Face::Segment][usize::from(self.draft)];
                self.back();
                return requested!(UiRequest::AppearanceCommit {
                    field: Field::Face,
                    value: self.draft,
                });
            }
            _ => {}
        }
        Ok(Effect::None)
    }
    fn menu_event(&mut self, e: Event) -> Result<Effect, UiError> {
        let count = if self.page == Screen::SportList {
            4
        } else {
            menu(self.page).map_or(0, <[Screen]>::len)
        };
        let last = u8::try_from(count.saturating_sub(1) / 3).map_err(|_| UiError::Data)?;
        match e {
            Event::Up => {
                if self.page == Screen::Quick {
                    self.back();
                } else {
                    self.nav = (self.nav + 1).min(last);
                }
            }
            Event::Down => {
                if self.page == Screen::Tools && self.nav == 0 {
                    self.back();
                } else if self.page != Screen::Quick {
                    self.nav = self.nav.saturating_sub(1);
                }
            }
            Event::Select => return self.activate(usize::from(self.nav) * 3),
            Event::Tap { y, .. } => {
                return self.activate(
                    usize::from(self.nav) * 3
                        + if y <= 42 {
                            0
                        } else if y <= 84 {
                            1
                        } else {
                            2
                        },
                );
            }
            _ => {}
        }
        Ok(Effect::None)
    }
    fn activate(&mut self, i: usize) -> Result<Effect, UiError> {
        if self.page == Screen::SportList {
            if let Some(k) = [
                SportKind::Cycling,
                SportKind::Mountain,
                SportKind::Badminton,
                SportKind::Run,
            ]
            .get(i)
            {
                self.sport_kind = *k;
                self.go(Screen::SportCountdown, true)?;
                return requested!(UiRequest::SportPrepare(*k));
            }
        } else if let Some(s) = menu(self.page).and_then(|a| a.get(i)) {
            self.go(*s, true)?;
        }
        Ok(Effect::None)
    }
    fn edit_event(&mut self, e: Event, f: Field) -> Result<Effect, UiError> {
        let limit = match f {
            Field::Brightness => 5,
            Field::Face | Field::Theme => 3,
            Field::Format | Field::Textsize => 2,
            Field::Language => 9,
        };
        let old = self.draft;
        if e == Event::Up {
            self.draft = if f == Field::Brightness {
                (old + 1) % 5
            } else {
                (old + 1).min(limit - 1)
            };
        } else if e == Event::Down {
            self.draft = if f == Field::Brightness {
                (old + 4) % 5
            } else {
                old.saturating_sub(1)
            };
        }
        if (e == Event::Up || e == Event::Down) && f == Field::Brightness && old != self.draft {
            return requested!(UiRequest::BrightnessPreview(self.draft));
        }
        if e == Event::Select || matches!(e, Event::Tap { y: 100.., .. }) {
            let v = self.draft;
            if f == Field::Language && v > 1 {
                return self.feedback(UiRequest::LanguageRequested(
                    [
                        Language::Zh,
                        Language::En,
                        Language::Fr,
                        Language::De,
                        Language::It,
                        Language::Pt,
                        Language::Ru,
                        Language::Es,
                        Language::Nl,
                    ][usize::from(v)],
                ));
            }
            match f {
                Field::Brightness => self.appearance.brightness = v,
                Field::Theme => {
                    self.appearance.theme =
                        [Theme::Phosphor, Theme::Amber, Theme::Ice][usize::from(v)];
                }
                Field::Format => {
                    self.appearance.format = if v == 0 {
                        TimeFormat::Hour24
                    } else {
                        TimeFormat::Hour12
                    }
                }
                Field::Textsize => self.appearance.textsize = v,
                Field::Language => {
                    self.appearance.locale = if v == 0 { Locale::Zh } else { Locale::En }
                }
                Field::Face => {}
            }
            self.back();
            return requested!(UiRequest::AppearanceCommit { field: f, value: v });
        }
        Ok(Effect::None)
    }
    fn confirm_event(&mut self, e: Event) -> Result<Effect, UiError> {
        if e == Event::Select {
            self.back();
        } else if let Event::Tap { x, y: 96.. } = e {
            if x < 64 {
                self.back();
            } else {
                let r = match self.page {
                    Screen::Clear => UiRequest::ClearData,
                    Screen::Restart => UiRequest::Restart,
                    Screen::Power => UiRequest::PowerOff,
                    Screen::Lock => UiRequest::Lock,
                    _ => UiRequest::SportStop,
                };
                return self.feedback(r);
            }
        }
        Ok(Effect::None)
    }
    fn time_event(&mut self, e: Event, d: &Snapshot<'_>) -> Result<Effect, UiError> {
        use Screen as S;
        use UiRequest as R;
        match self.page {
            S::Timer => match e {
                Event::Tap { y: 100.., .. } | Event::Select => self.page = S::TimerCustom,
                Event::Tap { x, y: 24.. } => {
                    let mins = [1, 3, 5, 10][usize::from(matches!(e, Event::Tap { y: 62.., .. }))
                        * 2
                        + usize::from(x >= 64)];
                    self.timer_hms = [0, mins, 0];
                    self.page = S::TimerRun;
                    return requested!(R::TimerPrepare {
                        seconds: u32::from(mins) * 60,
                    });
                }
                _ => {}
            },
            S::TimerCustom => {
                if e == Event::Left {
                    self.timer_field = (self.timer_field + 1) % 3;
                }
                if matches!(e, Event::Up | Event::Down) {
                    self.change_time(e == Event::Up);
                }
                if let Event::Tap { x, y: 0..100 } = e {
                    self.timer_field = u8::try_from((x / 43).min(2)).map_err(|_| UiError::Data)?;
                    self.change_time(!matches!(e, Event::Tap { y: 0..48, .. }));
                }
                if e == Event::Select || matches!(e, Event::Tap { y: 100.., .. }) {
                    self.page = S::TimerRun;
                    return requested!(R::TimerPrepare {
                        seconds: self.duration(),
                    });
                }
            }
            S::TimerRun => {
                if e == Event::Select {
                    return requested!(timer_action(d));
                }
                if let Event::Tap { x, y: 96.. } = e {
                    return requested!(if x < 64 {
                        R::TimerStop
                    } else {
                        timer_action(d)
                    });
                }
            }
            S::TimerDone if selected(e) => {
                self.page = S::Timer;
                return requested!(R::TimerAcknowledge);
            }
            S::Stopwatch => match e {
                Event::Up => {
                    self.lap_top = (self.lap_top + 1).min(d.stopwatch.laps.len().saturating_sub(2));
                }
                Event::Down => self.lap_top = self.lap_top.saturating_sub(1),
                Event::Select => return requested!(watch_action(d)),
                Event::Tap { x, y: 96.. } => {
                    return requested!(if x < 64 {
                        if d.stopwatch.state == StopwatchState::Running {
                            R::StopwatchLap
                        } else {
                            R::StopwatchReset
                        }
                    } else {
                        watch_action(d)
                    });
                }
                _ => {}
            },
            _ => {}
        }
        Ok(Effect::None)
    }
    pub(crate) fn duration(&self) -> u32 {
        u32::from(self.timer_hms[0]) * 3600
            + u32::from(self.timer_hms[1]) * 60
            + u32::from(self.timer_hms[2])
    }
    fn change_time(&mut self, up: bool) {
        let i = usize::from(self.timer_field);
        let max = if i == 0 { 100 } else { 60 };
        self.timer_hms[i] = (self.timer_hms[i] + if up { 1 } else { max - 1 }) % max;
    }
    fn message_event(&mut self, e: Event, d: &Snapshot<'_>) -> Result<Effect, UiError> {
        if self.page == Screen::Messages {
            match e {
                Event::Up => {
                    self.message_index =
                        (self.message_index + 1).min(d.messages.len().saturating_sub(1));
                }
                Event::Down => self.message_index = self.message_index.saturating_sub(1),
                e if selected(e) && !d.messages.is_empty() => {
                    self.go(Screen::MessageDetail, true)?;
                }
                _ => {}
            }
        } else {
            match e {
                Event::Left => {
                    self.page = Screen::Temperature;
                    self.depth = 0;
                }
                Event::Down => self.message_index = self.message_index.saturating_sub(1),
                e if selected(e) || e == Event::Up => {
                    if self.message_index + 1 < d.messages.len() {
                        self.message_index += 1;
                    } else {
                        self.return_to(Screen::Messages);
                        self.message_index = 0;
                        return requested!(UiRequest::MessagesReadAllRequested);
                    }
                }
                _ => {}
            }
        }
        Ok(Effect::None)
    }
    fn sport_event(&mut self, e: Event, d: &Snapshot<'_>) -> Result<Effect, UiError> {
        use Screen as S;
        use UiRequest as R;
        match self.page {
            S::Sport if selected(e) => self.go(S::SportList, true)?,
            S::SportCountdown if selected(e) => return requested!(R::SportStart(self.sport_kind)),
            S::SportRun if selected(e) || matches!(e, Event::Up | Event::Down) => {
                if d.sport.locked {
                    return requested!(R::SportUnlockRequested);
                }
                self.page = S::SportControls;
            }
            S::SportControls => {
                if let Event::Tap { x, y } = e {
                    if y < 64 {
                        if x < 64 {
                            return requested!(R::SportLockToggle(!d.sport.locked));
                        }
                        self.go(S::SportEnd, true)?;
                    } else if x < 64 {
                        self.page = S::SportRun;
                    } else {
                        return requested!(if d.sport.phase == SportPhase::Paused {
                            R::SportResume
                        } else {
                            R::SportPause
                        });
                    }
                }
            }
            _ => {}
        }
        Ok(Effect::None)
    }
}
