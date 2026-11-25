//! Layout-owned conservative invalidation. No pixel scan or old-scene draw.
use crate::{Face, Scene, Screen, Snapshot, StopwatchSnapshot, StopwatchState, TimeFormat};
use device_core::{Dimensions, Rect};
use device_ui::{
    DirtyRegion,
    graphics::{prelude::*, primitives::Rectangle},
};
const FULL: Rect = Rect::new(0, 0, 128, 128);
const BODY: Rect = Rect::new(0, 21, 128, 107);
fn add(dirty: &mut DirtyRegion, r: Rect) {
    dirty.invalidate(Rectangle::new(
        Point::new(i32::from(r.x), i32::from(r.y)),
        Size::new(u32::from(r.width), u32::from(r.height)),
    ));
}
impl Scene<'_, '_> {
    /// Conservative changed bounds relative to the last fully displayed demo scene.
    /// Passing `None` forces a full image after reset or transport recovery.
    #[must_use]
    pub fn damage_from(&self, previous: Option<&Scene<'_, '_>>) -> Option<Rect> {
        self.damage(previous)
    }
}
impl Scene<'_, '_, true> {
    /// Conservative changed bounds relative to a fully displayed product or demo scene.
    /// A mode change always forces full repaint. Use `::<true>(None)` after reset.
    #[must_use]
    pub fn damage_from<const OLD_PRODUCT: bool>(
        &self,
        previous: Option<&Scene<'_, '_, OLD_PRODUCT>>,
    ) -> Option<Rect> {
        self.damage(previous)
    }
}
impl<const PRODUCT: bool> Scene<'_, '_, PRODUCT> {
    /// Marks conservative layout bounds directly from validated presentation fields.
    ///
    /// No rendering, framebuffer, pixel comparison or hardware access is performed.
    /// Both old and new text extents fit the marked region, including removals.
    /// Unknown fields/layout changes fall back to the body or full image. Callers
    /// commit the new shown snapshot only after all returned pixels are written;
    /// after display reset/transport recovery pass `None` to force a full image.
    #[must_use]
    fn damage<const OLD_PRODUCT: bool>(
        &self,
        previous: Option<&Scene<'_, '_, OLD_PRODUCT>>,
    ) -> Option<Rect> {
        let Some(old) = previous else {
            return Some(FULL);
        };
        if PRODUCT != OLD_PRODUCT {
            return Some(FULL);
        }
        if self.model == old.model && self.data == old.data {
            return None;
        }
        let m = &self.model;
        if m.page != old.model.page || m.appearance != old.model.appearance {
            return Some(FULL);
        }
        if matches!(m.page, Screen::Theme | Screen::Face | Screen::SportList)
            || crate::model::menu(m.page).is_some()
        {
            return Some(FULL);
        }
        if m != &old.model {
            return Some(match m.page {
                Screen::Dial => FULL,
                Screen::Brightness | Screen::Textsize => Rect::new(0, 21, 128, 79),
                Screen::Language => Rect::new(0, 30, 128, 68),
                _ => BODY,
            });
        }
        let mut dirty = DirtyRegion::new(Dimensions::new(128, 128));
        let mut rest = *old.data;
        let d = self.data;
        if m.page == Screen::Dial {
            dial(self, &mut rest, &mut dirty);
            if rest != *d {
                return Some(FULL);
            }
        } else {
            macro_rules! field {
                ($field:ident,$r:expr) => {
                    if rest.$field != d.$field {
                        rest.$field = d.$field;
                        add(&mut dirty, $r);
                    }
                };
            }
            match m.page {
                Screen::Heart
                | Screen::Pressure
                | Screen::Oxygen
                | Screen::Temperature
                | Screen::Glucose
                | Screen::Hba1c => {
                    field!(measurements, BODY);
                    field!(heart_high, Rect::new(0, 100, 128, 28));
                    field!(heart_low, Rect::new(0, 100, 128, 28));
                }
                Screen::Ota | Screen::Applet => {
                    field!(progress, Rect::new(0, 30, 128, 58));
                }
                Screen::Charge => {
                    field!(charging, Rect::new(0, 30, 128, 45));
                }
                Screen::TimerRun => {
                    if rest.timer != d.timer {
                        add(
                            &mut dirty,
                            if rest.timer.state == d.timer.state {
                                Rect::new(0, 37, 128, 24)
                            } else {
                                BODY
                            },
                        );
                        rest.timer = d.timer;
                    }
                }
                Screen::Stopwatch => {
                    stopwatch(&rest.stopwatch, &d.stopwatch, &mut dirty);
                    rest.stopwatch = d.stopwatch;
                }
                Screen::Weather => {
                    field!(weather, Rect::new(0, 45, 128, 56));
                    field!(month, Rect::new(0, 110, 128, 18));
                    field!(day, Rect::new(0, 110, 128, 18));
                    field!(weekday, Rect::new(0, 110, 128, 18));
                }
                _ => {}
            }
            if rest != *d {
                add(&mut dirty, BODY);
            }
        }
        dirty.pending()
    }
}
fn dial<const PRODUCT: bool>(
    next: &Scene<'_, '_, PRODUCT>,
    rest: &mut Snapshot<'_>,
    dirty: &mut DirtyRegion,
) {
    let d = next.data;
    let appearance = next.model.appearance;
    let (hour, minute, date, week, battery) = match appearance.face {
        Face::Stack => (
            Rect::new(8, 23, 66, 42),
            Rect::new(8, 74, 66, 42),
            Rect::new(90, 25, 22, 35),
            Rect::new(87, 72, 41, 16),
            Rect::new(90, 96, 36, 12),
        ),
        Face::Line => (
            Rect::new(7, 39, 116, 48),
            Rect::new(7, 39, 116, 48),
            Rect::new(0, 81, 128, 12),
            Rect::new(0, 81, 128, 12),
            Rect::new(0, 107, 128, 12),
        ),
        Face::Segment => (
            Rect::new(12, 50, 108, 37),
            Rect::new(12, 50, 108, 37),
            Rect::new(0, 29, 128, 12),
            Rect::new(0, 29, 128, 12),
            Rect::new(0, 107, 128, 12),
        ),
    };
    if rest.hour != d.hour {
        rest.hour = d.hour;
        add(dirty, hour);
        if appearance.format == TimeFormat::Hour12 {
            add(dirty, Rect::new(94, 5, 31, 14));
        }
    }
    if rest.minute != d.minute {
        rest.minute = d.minute;
        add(dirty, minute);
    }
    if rest.month != d.month || rest.day != d.day {
        rest.month = d.month;
        rest.day = d.day;
        add(dirty, date);
    }
    if rest.weekday != d.weekday {
        rest.weekday = d.weekday;
        add(dirty, week);
    }
    if rest.battery != d.battery {
        rest.battery = d.battery;
        add(dirty, battery);
    }
    // All other fields remain in `rest`, so future/unknown changes force full.
}

fn stopwatch(old: &StopwatchSnapshot<'_>, new: &StopwatchSnapshot<'_>, dirty: &mut DirtyRegion) {
    if old == new {
        return;
    }
    if old.state == StopwatchState::Unavailable || new.state == StopwatchState::Unavailable {
        add(dirty, BODY);
        return;
    }
    if old.centis != new.centis {
        add(dirty, Rect::new(0, 29, 128, 24));
    }
    if old.laps != new.laps {
        add(dirty, Rect::new(0, 54, 128, 42));
    }
    if old.state != new.state {
        add(dirty, Rect::new(0, 96, 128, 32));
    }
}
