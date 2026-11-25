//! Pure presentation state. Requests do not execute product functions.
/// First-batch pages.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Page {
    /// Four large digits, source style2.
    Dial,
    /// Four-entry function menu.
    Menu,
    /// Eight-entry settings list.
    Settings,
    /// Five-level brightness editor.
    Brightness,
}
/// Input normalized by a host or board adapter.
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
    /// Validated panel coordinates.
    Tap {
        /// Horizontal coordinate.
        x: u16,
        /// Vertical coordinate.
        y: u16,
    },
    /// Touch long press.
    LongPress,
    /// Debounced short key release.
    KeyRelease,
    /// Held key request.
    KeyLongPress,
}
/// Destinations outside this first visual slice. No corresponding product action runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Destination {
    /// Countdown page.
    Countdown,
    /// Stopwatch page.
    Stopwatch,
    /// Weather page.
    Weather,
    /// Information page.
    Information,
    /// Dial picker.
    DialPicker,
    /// Font page.
    Font,
    /// Language page.
    Language,
    /// Data-clear confirmation page.
    Clear,
    /// Restart confirmation page.
    Reboot,
    /// Shutdown confirmation page.
    Power,
    /// Quick settings page.
    Shortcut,
    /// Temperature page.
    Temperature,
    /// Activity page.
    Activity,
}
/// Observable requests for the caller. None performs hardware or storage I/O.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// No external action.
    None,
    /// Preview a0..4 brightness value.
    PreviewBrightness(u8),
    /// Commit to this demo model only.
    CommitBrightness(u8),
    /// A page outside the delivered slice was requested.
    Open(Destination),
    /// A dial key requested display toggle.
    ToggleDisplay,
}
/// Invalid injected time, brightness or panel coordinate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputError;
/// Device-independent original UI state, using caller-supplied sample time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Model {
    page: Page,
    menu: u8,
    settings: u8,
    level: u8,
    saved: u8,
    hour: u8,
    minute: u8,
}
impl Model {
    /// Creates a dial with validated display data, never reading a clock or settings.
    /// # Errors
    /// Rejects hour>=24, minute>=60 or level>=5.
    pub fn new(hour: u8, minute: u8, level: u8) -> Result<Self, InputError> {
        if hour >= 24 || minute >= 60 || level >= 5 {
            return Err(InputError);
        }
        Ok(Self {
            page: Page::Dial,
            menu: 0,
            settings: 0,
            level,
            saved: level,
            hour,
            minute,
        })
    }
    /// Visible page.
    #[must_use]
    pub const fn page(self) -> Page {
        self.page
    }
    /// Function-menu page0..1.
    #[must_use]
    pub const fn menu_index(self) -> u8 {
        self.menu
    }
    /// Settings page0..2.
    #[must_use]
    pub const fn settings_index(self) -> u8 {
        self.settings
    }
    /// Preview level0..4.
    #[must_use]
    pub const fn level(self) -> u8 {
        self.level
    }
    /// Injected hour/minute.
    #[must_use]
    pub const fn time(self) -> (u8, u8) {
        (self.hour, self.minute)
    }
    /// Applies the source page's input rules and emits an explicit request.
    /// # Errors
    /// Out-of-screen taps leave the entire state unchanged.
    pub fn handle(&mut self, event: Event) -> Result<Action, InputError> {
        if let Event::Tap { x, y } = event
            && (x >= 128 || y >= 128)
        {
            return Err(InputError);
        }
        let action = match self.page {
            Page::Dial => self.dial(event),
            Page::Menu => self.menu(event),
            Page::Settings => self.settings(event),
            Page::Brightness => self.brightness(event),
        };
        Ok(action)
    }
    fn dial(&mut self, e: Event) -> Action {
        match e {
            Event::Up => {
                self.page = Page::Menu;
                self.menu = 0;
                Action::None
            }
            Event::LongPress => Action::Open(Destination::DialPicker),
            Event::Down => Action::Open(Destination::Shortcut),
            Event::Right => Action::Open(Destination::Temperature),
            Event::Left => Action::Open(Destination::Activity),
            Event::KeyRelease => Action::ToggleDisplay,
            Event::KeyLongPress => Action::Open(Destination::Power),
            Event::Tap { .. } => Action::None,
        }
    }
    fn menu(&mut self, e: Event) -> Action {
        match e {
            Event::Up => self.menu = 1,
            Event::Down if self.menu > 0 => self.menu = 0,
            Event::Down | Event::Right | Event::KeyRelease => self.page = Page::Dial,
            Event::Tap { y, .. } => {
                let row = row(y);
                let item = self.menu * 3 + row;
                return match item {
                    0 => Action::Open(Destination::Countdown),
                    1 => Action::Open(Destination::Stopwatch),
                    2 => Action::Open(Destination::Weather),
                    3 => {
                        self.page = Page::Settings;
                        self.settings = 0;
                        Action::None
                    }
                    _ => Action::None,
                };
            }
            _ => {}
        }
        Action::None
    }
    fn settings(&mut self, e: Event) -> Action {
        match e {
            Event::Up => self.settings = (self.settings + 1).min(2),
            Event::Down => self.settings = self.settings.saturating_sub(1),
            Event::Right => self.page = Page::Menu,
            Event::KeyRelease => self.page = Page::Dial,
            Event::Tap { y, .. } => {
                let n = self.settings * 3 + row(y);
                return match n {
                    0 => Action::Open(Destination::Information),
                    1 => Action::Open(Destination::DialPicker),
                    2 => {
                        self.page = Page::Brightness;
                        self.level = self.saved;
                        Action::None
                    }
                    3 => Action::Open(Destination::Font),
                    4 => Action::Open(Destination::Language),
                    5 => Action::Open(Destination::Clear),
                    6 => Action::Open(Destination::Reboot),
                    7 => Action::Open(Destination::Power),
                    _ => Action::None,
                };
            }
            _ => {}
        }
        Action::None
    }
    fn brightness(&mut self, e: Event) -> Action {
        match e {
            Event::Up => {
                self.level = (self.level + 1) % 5;
                Action::PreviewBrightness(self.level)
            }
            Event::Down => {
                self.level = (self.level + 4) % 5;
                Action::PreviewBrightness(self.level)
            }
            Event::Right | Event::KeyRelease => {
                self.level = self.saved;
                self.page = if e == Event::Right {
                    Page::Settings
                } else {
                    Page::Dial
                };
                Action::PreviewBrightness(self.saved)
            }
            Event::Tap { y, .. } if (103..127).contains(&y) => {
                self.saved = self.level;
                self.page = Page::Settings;
                Action::CommitBrightness(self.saved)
            }
            _ => Action::None,
        }
    }
}
fn row(y: u16) -> u8 {
    if y <= 42 {
        0
    } else if y <= 84 {
        1
    } else {
        2
    }
}
