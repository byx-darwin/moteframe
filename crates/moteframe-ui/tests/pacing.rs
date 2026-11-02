// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Clock epochs, failed frame starts and atomic pacing updates.
#![cfg(feature = "frame-pacing")]
use core::{cell::Cell, num::NonZeroU32};
use moteframe_core::{Dimensions, Display, Rect};
use moteframe_ui::{
    ConfigError, DisplayBinding, DrawOutcome, FrameInterval, Palette, RefreshSession,
    RefreshSettings, ResourceKey, ResourceMetadata, ResourceView, SessionError, UiSettings,
    UiState, UpdateImpact, graphics::pixelcolor::BinaryColor,
};
#[derive(Debug)]
struct Screen {
    fail: bool,
    writes: usize,
}
impl Display for Screen {
    type Color = BinaryColor;
    type Error = u8;
    fn dimensions(&self) -> Dimensions {
        Dimensions::new(1, 1)
    }
    fn write(&mut self, _: Rect, _: &[BinaryColor]) -> Result<(), u8> {
        self.writes += 1;
        if self.fail { Err(7) } else { Ok(()) }
    }
}
fn settings() -> UiSettings<BinaryColor> {
    UiSettings::new(Palette::new(
        BinaryColor::Off,
        BinaryColor::On,
        BinaryColor::On,
        BinaryColor::Off,
        BinaryColor::On,
    ))
}
fn empty() -> ResourceView<'static, ()> {
    ResourceView::empty(&())
}
fn interval(n: u32) -> Result<RefreshSettings, ConfigError> {
    Ok(
        RefreshSettings::new().with_frame_interval(FrameInterval::MinimumInterval(
            NonZeroU32::new(n).ok_or(ConfigError::InvalidResources)?,
        )),
    )
}
macro_rules! draw {
    ($s:expr,$d:expr,$time:expr) => {
        $s.draw_timed(
            DisplayBinding::new($d, 1),
            empty(),
            || $time,
            |_, _| Ok::<_, u8>(()),
            |_| Ok::<_, u8>(()),
        )
    };
}
#[test]
fn failures_defer_and_clock_regression_starts_a_new_epoch() -> Result<(), ConfigError> {
    let mut ui = UiState::new(settings(), Dimensions::new(1, 1), empty())?;
    let mut scratch = [BinaryColor::Off];
    let mut s = RefreshSession::new(&mut ui, &mut scratch, interval(10)?)?;
    let mut d = Screen {
        fail: true,
        writes: 0,
    };
    assert!(draw!(s, &mut d, 100).is_err());
    assert_eq!(s.pacing().last_start(), Some(100));
    assert_eq!(s.pacing().last_observed(), Some(100));
    d.fail = false;
    assert_eq!(
        draw!(s, &mut d, 101),
        Ok(DrawOutcome::Deferred { remaining_us: 9 })
    );
    assert_eq!(d.writes, 1);
    assert!(s.pending().is_some());
    assert_eq!(s.pacing().last_start(), Some(100));
    assert_eq!(s.pacing().last_observed(), Some(101));
    assert_eq!(draw!(s, &mut d, 110), Ok(DrawOutcome::Drawn));
    s.invalidate_all();
    assert_eq!(draw!(s, &mut d, 90), Err(SessionError::ClockRegressed));
    assert_eq!(d.writes, 2);
    assert_eq!(s.pacing().last_start(), None);
    assert_eq!(s.pacing().last_observed(), Some(90));
    assert_eq!(draw!(s, &mut d, 91), Ok(DrawOutcome::Drawn));
    assert_eq!(d.writes, 3);
    Ok(())
}
#[test]
fn zero_time_updates_rebind_and_interval_changes_do_not_bypass_pacing() -> Result<(), ConfigError> {
    let mut ui = UiState::new(settings(), Dimensions::new(1, 1), empty())?;
    let mut scratch = [BinaryColor::Off];
    let mut s = RefreshSession::new(&mut ui, &mut scratch, interval(10)?)?;
    let mut d = Screen {
        fail: false,
        writes: 0,
    };
    assert_eq!(draw!(s, &mut d, 0), Ok(DrawOutcome::Drawn));
    s.invalidate_all();
    assert_eq!(
        s.try_update(settings(), interval(10)?, empty())?,
        UpdateImpact::Unchanged
    );
    assert_eq!(
        draw!(s, &mut d, 1),
        Ok(DrawOutcome::Deferred { remaining_us: 9 })
    );
    assert_eq!(
        s.try_update(settings(), interval(20)?, empty())?,
        UpdateImpact::PolicyOnly
    );
    assert_eq!(
        draw!(s, &mut d, 10),
        Ok(DrawOutcome::Deferred { remaining_us: 10 })
    );
    let changed = settings().with_palette(Palette::new(
        BinaryColor::On,
        BinaryColor::On,
        BinaryColor::On,
        BinaryColor::Off,
        BinaryColor::On,
    ));
    assert_eq!(
        s.try_update(changed, interval(20)?, empty())?,
        UpdateImpact::FullRedraw
    );
    assert_eq!(
        s.try_rebind(
            changed,
            interval(20)?,
            DisplayBinding::new(&mut d, 1),
            empty()
        )?,
        UpdateImpact::FullRedraw
    );
    assert_eq!(
        draw!(s, &mut d, 11),
        Ok(DrawOutcome::Deferred { remaining_us: 9 })
    );
    assert_eq!(
        s.try_update(changed, RefreshSettings::new(), empty())?,
        UpdateImpact::PolicyOnly
    );
    assert_eq!(s.pacing().last_start(), None);
    assert_eq!(s.pacing().last_observed(), None);
    assert_eq!(
        s.draw_timed(
            DisplayBinding::new(&mut d, 1),
            empty(),
            || panic!("unlimited clock"),
            |_, _| Ok::<_, u8>(()),
            |_| Ok::<_, u8>(())
        ),
        Ok(DrawOutcome::Drawn)
    );
    assert_eq!(
        s.try_update(changed, interval(10)?, empty())?,
        UpdateImpact::PolicyOnly
    );
    s.invalidate_all();
    assert_eq!(draw!(s, &mut d, 0), Ok(DrawOutcome::Drawn));
    Ok(())
}
#[test]
fn idle_invalid_and_deferred_never_call_render_or_service() -> Result<(), ConfigError> {
    let mut ui = UiState::new(settings(), Dimensions::new(1, 1), empty())?;
    let mut scratch = [BinaryColor::Off];
    let mut s = RefreshSession::new(&mut ui, &mut scratch, interval(10)?)?;
    let mut d = Screen {
        fail: false,
        writes: 0,
    };
    let calls = Cell::new(0);
    let key = ResourceKey::new(NonZeroU32::MIN, 1);
    let wrong = ResourceView::new(key, ResourceMetadata::new(1, 1, 1)?, &())?;
    assert_eq!(
        s.draw_timed(
            DisplayBinding::new(&mut d, 1),
            wrong,
            || panic!("invalid clock"),
            |_, _| -> Result<(), u8> { panic!("invalid scene") },
            |_| -> Result<(), u8> { panic!("invalid service") }
        ),
        Err(SessionError::Config(ConfigError::ResourceMismatch))
    );
    assert_eq!(draw!(s, &mut d, u64::MAX), Ok(DrawOutcome::Drawn));
    assert_eq!(
        s.draw_timed(
            DisplayBinding::new(&mut d, 1),
            empty(),
            || panic!("idle clock"),
            |_, _| -> Result<(), u8> { panic!("idle scene") },
            |_| -> Result<(), u8> { panic!("idle service") }
        ),
        Ok(DrawOutcome::Idle)
    );
    s.invalidate_all();
    assert_eq!(
        s.draw_timed(
            DisplayBinding::new(&mut d, 1),
            empty(),
            || {
                calls.set(calls.get() + 1);
                0
            },
            |_, _| -> Result<(), u8> { panic!("regression scene") },
            |_| -> Result<(), u8> { panic!("regression service") }
        ),
        Err(SessionError::ClockRegressed)
    );
    assert_eq!(calls.get(), 1);
    assert_eq!(draw!(s, &mut d, 0), Ok(DrawOutcome::Drawn));
    s.invalidate_all();
    assert_eq!(
        s.draw_timed(
            DisplayBinding::new(&mut d, 1),
            empty(),
            || 1,
            |_, _| -> Result<(), u8> { panic!("deferred scene") },
            |_| -> Result<(), u8> { panic!("deferred service") }
        ),
        Ok(DrawOutcome::Deferred { remaining_us: 9 })
    );
    Ok(())
}

#[test]
fn ordinary_draw_remains_feature_additive_and_cannot_bypass_a_required_clock()
-> Result<(), ConfigError> {
    let mut ui = UiState::new(settings(), Dimensions::new(1, 1), empty())?;
    let mut scratch = [BinaryColor::Off];
    let mut s = RefreshSession::new(&mut ui, &mut scratch, interval(10)?)?;
    let mut d = Screen {
        fail: false,
        writes: 0,
    };
    assert_eq!(
        s.draw(
            DisplayBinding::new(&mut d, 1),
            empty(),
            |_, _| Ok::<_, u8>(()),
            |_| Ok::<_, u8>(())
        ),
        Err(SessionError::ClockRequired)
    );
    assert_eq!(d.writes, 0);
    assert_eq!(s.pacing().last_start(), None);
    assert!(s.pending().is_some());
    s.try_update(settings(), RefreshSettings::new(), empty())?;
    assert_eq!(
        s.draw(
            DisplayBinding::new(&mut d, 1),
            empty(),
            |_, _| Ok::<_, u8>(()),
            |_| Ok::<_, u8>(())
        ),
        Ok(DrawOutcome::Drawn)
    );
    Ok(())
}
