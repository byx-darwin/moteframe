//! Fixed-depth, atomic presentation state with no device-side business execution.
use crate::types::{
    Appearance, DisplayData, Effect, Event, Face, Field, Notice, Screen, StopwatchState, Theme,
    TimeFormat, TimerState, UiError, UiRequest,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Frame {
    screen: Screen,
    focus: u8,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Target {
    Screen(Screen),
    Request(UiRequest),
    Confirm(UiRequest),
}
pub(crate) fn menu(screen: Screen) -> Option<[(&'static str, Target); 3]> {
    use Target::{Confirm as C, Request as R, Screen as S};
    Some(match screen {
        Screen::Tools => [
            ("TIMER", S(Screen::Timer)),
            ("STOPWATCH", S(Screen::Stopwatch)),
            ("SETTINGS", S(Screen::Settings)),
        ],
        Screen::Quick => [
            ("BRIGHT", S(Screen::Brightness)),
            ("THEME", S(Screen::Theme)),
            ("SLEEP", R(UiRequest::DisplaySleep)),
        ],
        Screen::Settings => [
            ("BRIGHT", S(Screen::Brightness)),
            ("DISPLAY", S(Screen::Display)),
            ("ABOUT", S(Screen::About)),
        ],
        Screen::Display => [
            ("FACE", S(Screen::Face)),
            ("THEME", S(Screen::Theme)),
            ("FORMAT", S(Screen::Format)),
        ],
        Screen::System => [
            ("RESTART", C(UiRequest::Restart)),
            ("POWER OFF", C(UiRequest::PowerOff)),
            ("RESET", C(UiRequest::ResetSettings)),
        ],
        _ => return None,
    })
}
pub(crate) const fn field(page: Screen) -> Option<Field> {
    match page {
        Screen::Brightness => Some(Field::Brightness),
        Screen::Face => Some(Field::Face),
        Screen::Theme => Some(Field::Theme),
        Screen::Format => Some(Field::Format),
        _ => None,
    }
}
impl Appearance {
    pub(crate) const fn value(self, field: Field) -> u8 {
        match field {
            Field::Brightness => self.brightness,
            Field::Face => self.face as u8,
            Field::Theme => self.theme as u8,
            Field::Format => self.format as u8,
        }
    }
    fn assign(&mut self, field: Field, value: u8) {
        match field {
            Field::Brightness => self.brightness = value,
            Field::Face => {
                self.face = match value {
                    0 => Face::Stack,
                    1 => Face::Line,
                    _ => Face::Segment,
                }
            }
            Field::Theme => {
                self.theme = match value {
                    0 => Theme::Phosphor,
                    1 => Theme::Amber,
                    _ => Theme::Ice,
                }
            }
            Field::Format => {
                self.format = if value == 0 {
                    TimeFormat::Hour24
                } else {
                    TimeFormat::Hour12
                }
            }
        }
    }
}
/// Complete portable presentation state. Input failures leave every field unchanged.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Model {
    pub(crate) page: Screen,
    pub(crate) focus: u8,
    pub(crate) appearance: Appearance,
    pub(crate) draft: u8,
    pub(crate) request: UiRequest,
    pub(crate) notice: Notice,
    pub(crate) confirm: UiRequest,
    pub(crate) data: DisplayData,
    trail: [Frame; 6],
    depth: u8,
}
impl Default for Model {
    fn default() -> Self {
        Self {
            page: Screen::Dial,
            focus: 0,
            appearance: Appearance::default(),
            draft: 0,
            request: UiRequest::TimerStart,
            notice: Notice::DemoOnly,
            confirm: UiRequest::Restart,
            data: DisplayData::default(),
            trail: [Frame {
                screen: Screen::Dial,
                focus: 0,
            }; 6],
            depth: 0,
        }
    }
}
impl Model {
    /// Creates validated injected data/appearance, starting at the dial.
    /// # Errors
    /// Rejects any out-of-range snapshot or brightness before constructing the model.
    pub fn new(data: DisplayData, appearance: Appearance) -> Result<Self, UiError> {
        data.validate()?;
        if appearance.brightness > 4 {
            return Err(UiError::Data);
        }
        Ok(Self {
            appearance,
            data,
            ..Self::default()
        })
    }
    /// Current screen.
    #[must_use]
    pub const fn screen(&self) -> Screen {
        self.page
    }
    /// Current menu focus0..2.
    #[must_use]
    pub const fn focus(&self) -> u8 {
        self.focus
    }
    /// RAM-only saved appearance.
    #[must_use]
    pub const fn appearance(&self) -> Appearance {
        self.appearance
    }
    /// Editor candidate index.
    #[must_use]
    pub const fn draft(&self) -> u8 {
        self.draft
    }
    /// Current caller-injected data snapshot.
    #[must_use]
    pub const fn data(&self) -> DisplayData {
        self.data
    }
    /// Updates display data atomically; no clocks or timers are started.
    /// # Errors
    /// Rejects out-of-range numeric fields without changing any state.
    pub fn set_data(&mut self, data: DisplayData) -> Result<(), UiError> {
        data.validate()?;
        self.data = data;
        Ok(())
    }
    /// Selects a feedback notice without claiming a device action ran.
    pub const fn set_notice(&mut self, notice: Notice) {
        self.notice = notice;
    }
    /// Constructs a direct demo view for host/diagnostic previews, preserving data and appearance.
    /// # Errors
    /// Propagates navigation capacity failure without committing partial state.
    pub fn preview(&mut self, page: Screen) -> Result<(), UiError> {
        let mut next = *self;
        next.home();
        if page != Screen::Dial {
            next.open(page)?;
        }
        *self = next;
        Ok(())
    }
    fn home(&mut self) {
        self.page = Screen::Dial;
        self.focus = 0;
        self.depth = 0;
    }
    fn open(&mut self, page: Screen) -> Result<(), UiError> {
        let frame = self
            .trail
            .get_mut(usize::from(self.depth))
            .ok_or(UiError::NavigationOverflow)?;
        *frame = Frame {
            screen: self.page,
            focus: self.focus,
        };
        self.depth += 1;
        self.page = page;
        self.focus = 0;
        if let Some(f) = field(page) {
            self.draft = self.appearance.value(f);
        }
        Ok(())
    }
    fn back(&mut self) {
        if self.depth == 0 {
            self.home();
        } else {
            self.depth -= 1;
            let parent = self.trail[usize::from(self.depth)];
            self.page = parent.screen;
            self.focus = parent.focus;
        }
    }
    fn request(&mut self, request: UiRequest) -> Result<Effect, UiError> {
        self.request = request;
        self.notice = Notice::DemoOnly;
        self.open(Screen::Request)?;
        Ok(Effect::Request(request))
    }
    fn activate(&mut self, index: u8) -> Result<Effect, UiError> {
        let Some(entries) = menu(self.page) else {
            return Ok(Effect::None);
        };
        let Some((_, target)) = entries.get(usize::from(index)) else {
            return Ok(Effect::None);
        };
        self.focus = index;
        match *target {
            Target::Screen(page) => {
                self.open(page)?;
                Ok(Effect::None)
            }
            Target::Request(request) => self.request(request),
            Target::Confirm(request) => {
                self.confirm = request;
                self.open(Screen::Confirm)?;
                Ok(Effect::None)
            }
        }
    }
    fn cancel_effect(&self) -> Effect {
        if self.page == Screen::Brightness && self.draft != self.appearance.brightness {
            Effect::Request(UiRequest::BrightnessPreview(self.appearance.brightness))
        } else {
            Effect::None
        }
    }
    /// Applies one normalized input atomically, producing at most one request.
    /// # Errors
    /// Invalid coordinates or exhausted navigation leave the complete model unchanged.
    pub fn handle(&mut self, event: Event) -> Result<Effect, UiError> {
        if let Event::Tap { x, y } = event
            && (x >= 128 || y >= 128)
        {
            return Err(UiError::Coordinate);
        }
        let mut next = *self;
        let effect = next.apply(event)?;
        *self = next;
        Ok(effect)
    }
    fn menu_input(&mut self, event: Event) -> Result<Effect, UiError> {
        match event {
            Event::Up => self.focus = (self.focus + 1).min(2),
            Event::Down => self.focus = self.focus.saturating_sub(1),
            Event::Select | Event::Left => return self.activate(self.focus),
            Event::Tap { y, .. } if (24..111).contains(&y) => {
                return self
                    .activate(u8::try_from((y - 24) / 29).map_err(|_| UiError::Coordinate)?);
            }
            _ => {}
        }
        Ok(Effect::None)
    }
    fn feedback_back(&mut self) {
        self.back();
        if self.page == Screen::Confirm {
            self.back();
        }
    }
    fn apply(&mut self, event: Event) -> Result<Effect, UiError> {
        if event == Event::KeyLongPress {
            let effect = self.cancel_effect();
            if self.page != Screen::System {
                self.home();
                self.open(Screen::System)?;
            }
            return Ok(effect);
        }
        if matches!(event, Event::KeyRelease | Event::Right) {
            if self.page == Screen::Dial {
                return if event == Event::KeyRelease {
                    self.request(UiRequest::DisplayToggle)
                } else {
                    Ok(Effect::None)
                };
            }
            let effect = self.cancel_effect();
            let request = self.page == Screen::Request;
            if event == Event::KeyRelease {
                self.home();
            } else {
                self.back();
                if request && self.page == Screen::Confirm {
                    self.back();
                }
            }
            return Ok(effect);
        }
        if self.page == Screen::Dial {
            match event {
                Event::Up => self.open(Screen::Tools)?,
                Event::Down => self.open(Screen::Quick)?,
                Event::LongPress => self.open(Screen::Face)?,
                _ => {}
            }
        } else if menu(self.page).is_some() {
            return self.menu_input(event);
        } else if let Some(f) = field(self.page) {
            let limit = match f {
                Field::Brightness => 5,
                Field::Face | Field::Theme => 3,
                Field::Format => 2,
            };
            let old = self.draft;
            match event {
                Event::Up => self.draft = (self.draft + 1).min(limit - 1),
                Event::Down => self.draft = self.draft.saturating_sub(1),
                _ => {}
            }
            if self.draft != old && f == Field::Brightness {
                return Ok(Effect::Request(UiRequest::BrightnessPreview(self.draft)));
            }
            if event == Event::Select || matches!(event,Event::Tap{y,..} if y>=100) {
                self.appearance.assign(f, self.draft);
                let effect = Effect::Request(UiRequest::AppearanceCommit {
                    field: f,
                    value: self.draft,
                });
                self.back();
                return Ok(effect);
            }
        } else if matches!(self.page, Screen::Timer | Screen::Stopwatch) {
            if event == Event::Select || matches!(event,Event::Tap{y,..} if y>=96) {
                let request = if self.page == Screen::Timer {
                    match self.data.timer_state {
                        TimerState::Unavailable => UiRequest::TimerDataRequest,
                        TimerState::Running => UiRequest::TimerPause,
                        TimerState::Paused => UiRequest::TimerResume,
                        _ => UiRequest::TimerStart,
                    }
                } else {
                    match self.data.stopwatch_state {
                        StopwatchState::Unavailable => UiRequest::StopwatchDataRequest,
                        StopwatchState::Running => UiRequest::StopwatchPause,
                        StopwatchState::Paused => UiRequest::StopwatchResume,
                        StopwatchState::Idle => UiRequest::StopwatchStart,
                    }
                };
                return self.request(request);
            }
        } else if self.page == Screen::Confirm {
            match event {
                Event::Tap { x, y } if y >= 96 => {
                    if x < 64 {
                        self.back();
                    } else {
                        return self.request(self.confirm);
                    }
                }
                Event::Select => self.back(),
                _ => {}
            }
        } else if self.page == Screen::Request
            && (event == Event::Select || matches!(event,Event::Tap{y,..} if y>=100))
        {
            self.feedback_back();
        }
        Ok(Effect::None)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_should_reject_exhausted_navigation() {
        let mut m = Model::default();
        for _ in 0..6 {
            assert!(m.open(Screen::About).is_ok());
        }
        let before = m;
        assert_eq!(m.handle(Event::KeyRelease), Ok(Effect::None));
        let mut full = before;
        assert_eq!(full.open(Screen::About), Err(UiError::NavigationOverflow));
        assert_eq!(full, before);
    }
}
