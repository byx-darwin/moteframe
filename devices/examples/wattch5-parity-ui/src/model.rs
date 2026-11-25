//! Small transactional navigation state. Data remains borrowed and caller-owned.
use crate::{
    Appearance, Effect, Event, Field, MeasureKind, Screen, Snapshot, SportKind, SportPhase,
    TimerState, UiError, UiRequest,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Frame {
    page: Screen,
    nav: u8,
    focus: u8,
}
/// Finite navigation and RAM appearance; never owns sensor/time/message services.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Model {
    pub(crate) page: Screen,
    pub(crate) appearance: Appearance,
    pub(crate) nav: u8,
    pub(crate) focus: u8,
    pub(crate) draft: u8,
    trail: [Frame; 6],
    depth: usize,
    pub(crate) measure_kind: MeasureKind,
    pub(crate) timer_hms: [u8; 3],
    pub(crate) timer_field: u8,
    pub(crate) lap_top: usize,
    pub(crate) message_index: usize,
    pub(crate) sport_kind: SportKind,
    pub(crate) request: Option<UiRequest>,
}
impl Default for Model {
    fn default() -> Self {
        Self {
            page: Screen::Dial,
            appearance: Appearance::default(),
            nav: 0,
            focus: 0,
            draft: 0,
            trail: [Frame {
                page: Screen::Dial,
                nav: 0,
                focus: 0,
            }; 6],
            depth: 0,
            measure_kind: MeasureKind::Heart,
            timer_hms: [0, 5, 0],
            timer_field: 0,
            lap_top: 0,
            message_index: 0,
            sport_kind: SportKind::Run,
            request: None,
        }
    }
}
pub(crate) const fn measure(s: Screen) -> Option<MeasureKind> {
    match s {
        Screen::Heart => Some(MeasureKind::Heart),
        Screen::Pressure => Some(MeasureKind::Pressure),
        Screen::Oxygen => Some(MeasureKind::Oxygen),
        Screen::Temperature => Some(MeasureKind::Temperature),
        Screen::Glucose => Some(MeasureKind::Glucose),
        Screen::Hba1c => Some(MeasureKind::Hba1c),
        _ => None,
    }
}
pub(crate) const fn measure_screen(k: MeasureKind) -> Screen {
    match k {
        MeasureKind::Heart => Screen::Heart,
        MeasureKind::Pressure => Screen::Pressure,
        MeasureKind::Oxygen => Screen::Oxygen,
        MeasureKind::Temperature => Screen::Temperature,
        MeasureKind::Glucose => Screen::Glucose,
        MeasureKind::Hba1c => Screen::Hba1c,
    }
}
pub(crate) const fn field(s: Screen) -> Option<Field> {
    match s {
        Screen::Brightness => Some(Field::Brightness),
        Screen::Face => Some(Field::Face),
        Screen::Theme => Some(Field::Theme),
        Screen::Format => Some(Field::Format),
        Screen::Textsize => Some(Field::Textsize),
        Screen::Language => Some(Field::Language),
        _ => None,
    }
}
pub(crate) const fn menu(s: Screen) -> Option<&'static [Screen]> {
    match s {
        Screen::Tools => Some(&[
            Screen::Timer,
            Screen::Stopwatch,
            Screen::Weather,
            Screen::Settings,
        ]),
        Screen::Quick => Some(&[Screen::About, Screen::Brightness, Screen::Settings]),
        Screen::Settings => Some(&[
            Screen::About,
            Screen::Face,
            Screen::Brightness,
            Screen::Textsize,
            Screen::Language,
            Screen::Clear,
            Screen::Restart,
            Screen::Power,
        ]),
        _ => None,
    }
}
pub(crate) const fn horizontal(s: Screen, left: bool) -> Option<Screen> {
    use Screen as S;
    Some(match (s, left) {
        (S::Dial, true) | (S::Heart, false) => S::Activity,
        (S::Dial, false) | (S::Messages | S::Sport, true) => S::Temperature,
        (S::Activity, true) | (S::Glucose, false) => S::Heart,
        (S::Activity, false) | (S::Temperature, true) => S::Dial,
        (S::Heart, true) | (S::Hba1c, false) => S::Glucose,
        (S::Glucose, true) | (S::Pressure, false) => S::Hba1c,
        (S::Hba1c, true) | (S::Oxygen, false) => S::Pressure,
        (S::Pressure, true) | (S::Sleep, false) => S::Oxygen,
        (S::Oxygen, true) | (S::Messages, false) => S::Sleep,
        (S::Sleep, true) | (S::Temperature | S::Sport, false) => S::Messages,
        _ => return None,
    })
}
impl Model {
    /// Measurement-session adapter point; a caller may schedule its own delayed start.
    #[must_use]
    pub const fn measurement_session(&self) -> Option<MeasureKind> {
        measure(self.page)
    }
    /// Displays an external notice while retaining the source navigation frame.
    /// # Errors
    /// Invalid data or navigation overflow reject atomically.
    pub fn show_notice(
        &mut self,
        n: crate::NoticeSnapshot,
        d: &Snapshot<'_>,
    ) -> Result<(), UiError> {
        self.open(n.kind.screen(), d)
    }

    /// Sets validated RAM appearance from an external settings adapter.
    /// # Errors
    /// Out-of-range brightness or text size leaves the model unchanged.
    pub fn set_appearance(&mut self, a: Appearance) -> Result<(), UiError> {
        if a.brightness > 4 || a.textsize > 1 {
            return Err(UiError::Data);
        }
        self.appearance = a;
        Ok(())
    }

    /// Current logical page.
    #[must_use]
    pub const fn screen(&self) -> Screen {
        self.page
    }
    /// RAM-only appearance.
    #[must_use]
    pub const fn appearance(&self) -> Appearance {
        self.appearance
    }
    /// Source menu page.
    #[must_use]
    pub const fn nav_page(&self) -> u8 {
        self.nav
    }
    /// Current editor index.
    #[must_use]
    pub const fn draft(&self) -> u8 {
        self.draft
    }
    /// Current message index, clamped after injection.
    #[must_use]
    pub const fn message_index(&self) -> usize {
        self.message_index
    }
    /// First visible lap.
    #[must_use]
    pub const fn lap_top(&self) -> usize {
        self.lap_top
    }
    /// Countdown editor values.
    #[must_use]
    pub const fn timer_hms(&self) -> [u8; 3] {
        self.timer_hms
    }
    /// Internal navigation depth for host diagnostics.
    #[must_use]
    pub const fn depth(&self) -> usize {
        self.depth
    }
    fn home(&mut self) {
        self.page = Screen::Dial;
        self.depth = 0;
        self.nav = 0;
        self.focus = 0;
    }
    pub(crate) fn return_to(&mut self, page: Screen) {
        while self.page != page && self.depth > 0 {
            self.back();
        }
        if self.page != page {
            self.home();
            self.page = page;
        }
    }
    pub(crate) fn back(&mut self) {
        if self.depth > 0 {
            self.depth -= 1;
            let f = self.trail[self.depth];
            self.page = f.page;
            self.nav = f.nav;
            self.focus = f.focus;
        } else {
            self.home();
        }
    }
    pub(crate) fn go(&mut self, s: Screen, push: bool) -> Result<(), UiError> {
        if push {
            let f = self
                .trail
                .get_mut(self.depth)
                .ok_or(UiError::NavigationOverflow)?;
            *f = Frame {
                page: self.page,
                nav: self.nav,
                focus: self.focus,
            };
            self.depth += 1;
        }
        self.page = s;
        self.nav = 0;
        self.focus = 0;
        if let Some(k) = measure(s) {
            self.measure_kind = k;
        }
        if let Some(f) = field(s) {
            self.draft = match f {
                Field::Brightness => self.appearance.brightness,
                Field::Face => self.appearance.face as u8,
                Field::Theme => self.appearance.theme as u8,
                Field::Format => self.appearance.format as u8,
                Field::Textsize => self.appearance.textsize,
                Field::Language => self.appearance.locale as u8,
            };
        }
        Ok(())
    }
    /// Opens a bounded source page atomically.
    /// # Errors
    /// Invalid snapshots and full navigation reject without mutation.
    pub fn open(&mut self, s: Screen, d: &Snapshot<'_>) -> Result<(), UiError> {
        d.validate()?;
        let mut n = *self;
        n.go(s, true)?;
        *self = n;
        Ok(())
    }
    /// Host inspection entry; does not add an invented product menu.
    /// # Errors
    /// Rejects invalid snapshots before changing navigation.
    pub fn preview(&mut self, s: Screen, d: &Snapshot<'_>) -> Result<(), UiError> {
        d.validate()?;
        let mut n = *self;
        n.home();
        if s != Screen::Dial {
            n.go(s, true)?;
        }
        *self = n;
        Ok(())
    }
    /// Applies validated caller data to navigation only; never edits that data.
    /// # Errors
    /// Invalid data leaves navigation unchanged.
    pub fn inject(&mut self, d: &Snapshot<'_>) -> Result<(), UiError> {
        d.validate()?;
        let mut n = *self;
        n.normalize_collections(d);
        if n.page == Screen::TimerRun && d.timer.state == TimerState::Done {
            n.page = Screen::TimerDone;
        }
        if matches!(
            n.page,
            Screen::SportCountdown | Screen::SportRun | Screen::SportControls | Screen::SportEnd
        ) {
            match d.sport.phase {
                SportPhase::Running | SportPhase::Paused if n.page == Screen::SportCountdown => {
                    n.page = Screen::SportRun;
                }
                SportPhase::Done => n.return_to(Screen::Sport),
                _ => {}
            }
        }
        *self = n;
        Ok(())
    }
    fn normalize_collections(&mut self, d: &Snapshot<'_>) {
        self.message_index = self.message_index.min(d.messages.len().saturating_sub(1));
        self.lap_top = self.lap_top.min(d.stopwatch.laps.len().saturating_sub(2));
        if d.messages.is_empty() && self.page == Screen::MessageDetail {
            self.return_to(Screen::Messages);
        }
        if self.page == Screen::Face && !d.applet_available {
            self.draft = self.draft.min(2);
        }
    }
    /// Processes a normalized input transactionally.
    /// # Errors
    /// Rejects invalid coordinates, snapshots or exhausted navigation without partial changes.
    pub fn handle(&mut self, e: Event, d: &Snapshot<'_>) -> Result<Effect, UiError> {
        if matches!(e,Event::Tap{x,y}if x>=128||y>=128) {
            return Err(UiError::Coordinate);
        }
        d.validate()?;
        let mut n = *self;
        n.inject(d)?;
        let effect = n.apply(e, d)?;
        n.normalize_collections(d);
        *self = n;
        Ok(effect)
    }
    pub(crate) fn feedback(&mut self, r: UiRequest) -> Result<Effect, UiError> {
        self.request = Some(r);
        self.go(Screen::Request, true)?;
        Ok(Effect::Request(r))
    }
    pub(crate) fn restore(&self) -> Effect {
        if self.page == Screen::Brightness && self.draft != self.appearance.brightness {
            Effect::Request(UiRequest::BrightnessPreview(self.appearance.brightness))
        } else {
            Effect::None
        }
    }
    pub(crate) fn close_request(&mut self) {
        self.back();
        if matches!(
            self.page,
            Screen::Clear
                | Screen::Power
                | Screen::Restart
                | Screen::Lock
                | Screen::SportEnd
                | Screen::MeasureConfirm
        ) {
            self.back();
        }
    }
}
#[path = "events.rs"]
mod events;
