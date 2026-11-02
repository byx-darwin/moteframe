// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Session transactions, recovery and one-frame service semantics.
#![cfg(feature = "refresh")]
use core::{cell::Cell, num::NonZeroU16};
use moteframe_core::{Dimensions, Display, Rect};
use moteframe_ui::graphics::geometry::Dimensions as _;
use moteframe_ui::{
    BindingState, ConfigError, CooperativeError, DisplayBinding, DrawOutcome, Palette, RedrawMode,
    RefreshError, RefreshSession, RefreshSettings, ResourceView, SessionError, TilePixels,
    UiSettings, UiState, UpdateImpact,
    graphics::{pixelcolor::Gray8, prelude::*, primitives::Rectangle},
};

#[derive(Debug)]
struct Screen {
    dims: Dimensions,
    pixels: Vec<Gray8>,
    writes: usize,
    fail_at: Option<usize>,
}
impl Screen {
    fn new() -> Self {
        Self {
            dims: Dimensions::new(5, 2),
            pixels: vec![Gray8::new(99); 10],
            writes: 0,
            fail_at: None,
        }
    }
}
impl Display for Screen {
    type Color = Gray8;
    type Error = u8;
    fn dimensions(&self) -> Dimensions {
        self.dims
    }
    fn write(&mut self, area: Rect, pixels: &[Gray8]) -> Result<(), u8> {
        assert!(area.fits(self.dims));
        assert_eq!(usize::try_from(area.area()), Ok(pixels.len()));
        self.writes += 1;
        if self.fail_at == Some(self.writes) {
            return Err(7);
        }
        let index = usize::from(area.y) * usize::from(self.dims.width) + usize::from(area.x);
        self.pixels[index..index + pixels.len()].copy_from_slice(pixels);
        Ok(())
    }
}
fn settings() -> UiSettings<Gray8> {
    UiSettings::new(Palette::new(
        Gray8::new(0),
        Gray8::new(1),
        Gray8::new(2),
        Gray8::new(3),
        Gray8::new(4),
    ))
}
fn empty() -> ResourceView<'static, ()> {
    ResourceView::empty(&())
}
fn policy(n: u16) -> Result<RefreshSettings, ConfigError> {
    Ok(RefreshSettings::new().with_tile_pixels(TilePixels::Fixed(
        NonZeroU16::new(n).ok_or(ConfigError::EmptyWorkspace)?,
    )))
}
#[cfg(not(feature = "frame-pacing"))]
macro_rules! draw {
    ($s:expr,$d:expr,$epoch:expr,$r:expr,$scene:expr,$service:expr) => {
        $s.draw(DisplayBinding::new($d, $epoch), $r, $scene, $service)
    };
}
#[cfg(feature = "frame-pacing")]
macro_rules! draw {
    ($s:expr,$d:expr,$epoch:expr,$r:expr,$scene:expr,$service:expr) => {
        $s.draw_timed(
            DisplayBinding::new($d, $epoch),
            $r,
            || 100,
            $scene,
            $service,
        )
    };
}
#[allow(clippy::unnecessary_wraps)] // Callback contract requires a Result.
fn no_scene(
    _: &mut moteframe_ui::Tile<'_, Gray8>,
    _: &moteframe_ui::UiContext<'_, Gray8, ()>,
) -> Result<(), u8> {
    Ok(())
}
#[allow(clippy::unnecessary_wraps)] // Callback contract requires a Result.
fn no_service(_: &mut moteframe_ui::DeferredInvalidation) -> Result<(), u8> {
    Ok(())
}

#[test]
fn workspace_policy_is_atomic_and_clean_frames_do_not_call_callbacks() -> Result<(), ConfigError> {
    let mut ui = UiState::new(settings(), Dimensions::new(5, 2), empty())?;
    assert_eq!(
        RefreshSession::new(&mut ui, &mut [], RefreshSettings::new()).err(),
        Some(ConfigError::EmptyWorkspace)
    );
    let mut scratch = [Gray8::new(9); 3];
    assert_eq!(
        RefreshSession::new(&mut ui, &mut scratch, policy(4)?).err(),
        Some(ConfigError::WorkspaceTooSmall)
    );
    let mut session = RefreshSession::new(&mut ui, &mut scratch, policy(2)?)?;
    assert_eq!(session.binding(), BindingState::Unbound);
    assert_eq!(
        session.try_update(
            settings().with_palette(Palette::new(
                Gray8::new(8),
                Gray8::new(1),
                Gray8::new(2),
                Gray8::new(3),
                Gray8::new(4)
            )),
            policy(4)?,
            empty()
        ),
        Err(ConfigError::WorkspaceTooSmall)
    );
    assert_eq!(*session.settings(), settings());
    let mut screen = Screen::new();
    assert_eq!(
        draw!(session, &mut screen, 1, empty(), no_scene, no_service),
        Ok(DrawOutcome::Drawn)
    );
    assert_eq!(screen.pixels, vec![Gray8::new(0); 10]);
    assert_eq!(screen.writes, 6);
    assert_eq!(
        session.try_update(settings(), policy(3)?, empty())?,
        UpdateImpact::PolicyOnly
    );
    assert_eq!(session.pending(), None);
    assert_eq!(
        draw!(
            session,
            &mut screen,
            1,
            empty(),
            |_, _| -> Result<(), u8> { panic!("idle scene") },
            |_| -> Result<(), u8> { panic!("idle service") }
        ),
        Ok(DrawOutcome::Idle)
    );
    Ok(())
}

#[test]
fn initial_resource_failure_stays_unbound_and_versions_are_checked_when_clean()
-> Result<(), ConfigError> {
    let mut ui = UiState::new(settings(), Dimensions::new(5, 2), empty())?;
    let mut scratch = [Gray8::new(9); 3];
    let mut s = RefreshSession::new(&mut ui, &mut scratch, RefreshSettings::new())?;
    let mut screen = Screen::new();
    let key = moteframe_ui::ResourceKey::new(core::num::NonZeroU32::MIN, 1);
    let resources = ResourceView::new(key, moteframe_ui::ResourceMetadata::new(1, 1, 1)?, &())?;
    assert_eq!(
        draw!(s, &mut screen, 1, resources, no_scene, no_service),
        Err(SessionError::Config(ConfigError::ResourceMismatch))
    );
    assert_eq!(s.binding(), BindingState::Unbound);
    assert_eq!(screen.writes, 0);
    assert_eq!(
        draw!(s, &mut screen, 1, empty(), no_scene, no_service),
        Ok(DrawOutcome::Drawn)
    );
    assert_eq!(
        draw!(s, &mut screen, 1, resources, no_scene, no_service),
        Err(SessionError::Config(ConfigError::ResourceMismatch))
    );
    assert_eq!(screen.writes, 4);
    Ok(())
}

#[test]
fn all_failure_sites_keep_original_and_merge_future_requests() -> Result<(), ConfigError> {
    for failure in 0..=5 {
        let mut ui = UiState::new(settings(), Dimensions::new(5, 2), empty())?;
        let mut scratch = [Gray8::new(9); 3];
        let mut s = RefreshSession::new(&mut ui, &mut scratch, policy(3)?)?;
        let mut screen = Screen::new();
        assert_eq!(
            draw!(s, &mut screen, 1, empty(), no_scene, no_service),
            Ok(DrawOutcome::Drawn)
        );
        let area = Rectangle::new(Point::new(0, 0), Size::new(4, 1));
        s.invalidate(area);
        let before = screen.writes;
        if failure == 1 || failure == 4 {
            screen.fail_at = Some(before + if failure == 1 { 1 } else { 2 });
        }
        let calls = Cell::new(0);
        let services = Cell::new(0);
        let result = draw!(
            s,
            &mut screen,
            1,
            empty(),
            |_, _| {
                calls.set(calls.get() + 1);
                if (failure == 0 && calls.get() == 1) || (failure == 3 && calls.get() == 2) {
                    Err(8)
                } else {
                    Ok(())
                }
            },
            |next| {
                services.set(services.get() + 1);
                next.invalidate(Rectangle::new(Point::new(4, 1), Size::new(2, 2)));
                if (failure == 2 && services.get() == 1) || (failure == 5 && services.get() == 2) {
                    Err(9)
                } else {
                    Ok(())
                }
            }
        );
        assert!(result.is_err());
        assert_eq!(
            s.pending(),
            Some(if services.get() > 0 {
                Rect::new(0, 0, 5, 2)
            } else {
                Rect::new(0, 0, 4, 1)
            })
        );
        screen.fail_at = None;
        assert_eq!(
            draw!(s, &mut screen, 1, empty(), no_scene, no_service),
            Ok(DrawOutcome::Drawn)
        );
        assert_eq!(s.pending(), None);
    }
    Ok(())
}

#[test]
fn success_merges_future_after_clear_and_does_not_loop() -> Result<(), ConfigError> {
    let mut ui = UiState::new(settings(), Dimensions::new(5, 2), empty())?;
    let mut scratch = [Gray8::new(9); 8];
    let mut s = RefreshSession::new(&mut ui, &mut scratch, RefreshSettings::new())?;
    let mut screen = Screen::new();
    assert_eq!(
        draw!(s, &mut screen, 1, empty(), no_scene, |next| {
            next.invalidate(Rectangle::new(Point::new(-2, 1), Size::new(3, 2)));
            Ok::<_, u8>(())
        }),
        Ok(DrawOutcome::Drawn)
    );
    assert_eq!(screen.writes, 2);
    assert_eq!(s.pending(), Some(Rect::new(0, 1, 1, 1)));
    assert_eq!(
        draw!(s, &mut screen, 1, empty(), no_scene, |next| {
            next.invalidate_all();
            Ok::<_, u8>(())
        }),
        Ok(DrawOutcome::Drawn)
    );
    assert_eq!(screen.writes, 3);
    assert_eq!(s.pending(), Some(Rect::new(0, 0, 5, 2)));
    Ok(())
}

#[test]
fn mismatch_latches_until_explicit_valid_rebind() -> Result<(), ConfigError> {
    let mut ui = UiState::new(settings(), Dimensions::new(5, 2), empty())?;
    let mut scratch = [Gray8::new(9); 8];
    let mut s = RefreshSession::new(&mut ui, &mut scratch, RefreshSettings::new())?;
    let mut screen = Screen::new();
    screen.dims = Dimensions::new(4, 2);
    assert_eq!(
        draw!(s, &mut screen, 1, empty(), no_scene, no_service),
        Err(SessionError::NeedsRebind)
    );
    screen.dims = Dimensions::new(5, 2);
    assert_eq!(
        draw!(s, &mut screen, 1, empty(), no_scene, no_service),
        Err(SessionError::NeedsRebind)
    );
    assert_eq!(
        s.try_rebind(
            settings(),
            RefreshSettings::new(),
            DisplayBinding::new(&mut screen, 1),
            empty()
        )?,
        UpdateImpact::FullRedraw
    );
    assert_eq!(
        draw!(s, &mut screen, 1, empty(), no_scene, no_service),
        Ok(DrawOutcome::Drawn)
    );
    // A new generation invalidates even a clean, same-sized display.
    assert_eq!(
        draw!(s, &mut screen, 2, empty(), no_scene, no_service),
        Err(SessionError::NeedsRebind)
    );
    assert_eq!(screen.writes, 2);
    assert_eq!(
        s.try_rebind(
            settings(),
            policy(9)?,
            DisplayBinding::new(&mut screen, 2),
            empty()
        ),
        Err(ConfigError::WorkspaceTooSmall)
    );
    assert_eq!(s.binding(), BindingState::NeedsRebind);
    screen.dims = Dimensions::new(0, 2);
    assert_eq!(
        s.try_rebind(
            settings(),
            RefreshSettings::new(),
            DisplayBinding::new(&mut screen, 2),
            empty()
        ),
        Err(ConfigError::InvalidDimensions)
    );
    screen.dims = Dimensions::new(3, 2);
    assert_eq!(
        s.try_rebind(
            settings(),
            RefreshSettings::new(),
            DisplayBinding::new(&mut screen, 2),
            empty()
        )?,
        UpdateImpact::FullRedraw
    );
    assert_eq!(s.pending(), Some(Rect::new(0, 0, 3, 2)));
    assert_eq!(
        draw!(s, &mut screen, 2, empty(), no_scene, no_service),
        Ok(DrawOutcome::Drawn)
    );
    // Invalid candidate without environmental change must not poison a valid binding.
    assert_eq!(
        s.try_rebind(
            settings(),
            policy(9)?,
            DisplayBinding::new(&mut screen, 2),
            empty()
        ),
        Err(ConfigError::WorkspaceTooSmall)
    );
    assert_eq!(
        draw!(s, &mut screen, 2, empty(), no_scene, no_service),
        Ok(DrawOutcome::Idle)
    );
    Ok(())
}

#[test]
fn context_and_explicit_colors_are_independent_of_chunk_width() -> Result<(), ConfigError> {
    let mut results = Vec::new();
    for n in [1, 2, 5, 8] {
        let mut ui = UiState::new(settings(), Dimensions::new(5, 2), empty())?;
        let mut scratch = [Gray8::new(9); 8];
        let mut s = RefreshSession::new(&mut ui, &mut scratch, policy(n)?)?;
        let mut screen = Screen::new();
        assert_eq!(
            draw!(
                s,
                &mut screen,
                1,
                empty(),
                |tile, ctx| {
                    assert_eq!(ctx.display_bounds(), Rect::new(0, 0, 5, 2));
                    tile.fill_solid(
                        &Rectangle::new(Point::new(2, 0), Size::new(2, 2)),
                        Gray8::new(7),
                    )
                    .map_err(|never| match never {})
                },
                no_service
            ),
            Ok(DrawOutcome::Drawn)
        );
        assert_eq!(
            screen.pixels,
            vec![
                Gray8::new(0),
                Gray8::new(0),
                Gray8::new(7),
                Gray8::new(7),
                Gray8::new(0),
                Gray8::new(0),
                Gray8::new(0),
                Gray8::new(7),
                Gray8::new(7),
                Gray8::new(0)
            ]
        );
        results.push(screen.pixels);
    }
    assert!(results.windows(2).all(|pair| pair[0] == pair[1]));
    Ok(())
}

#[test]
fn full_on_request_requires_work_and_resource_io_is_scene_error() -> Result<(), ConfigError> {
    let mut ui = UiState::new(settings(), Dimensions::new(5, 2), empty())?;
    let mut scratch = [Gray8::new(9); 8];
    let mut s = RefreshSession::new(
        &mut ui,
        &mut scratch,
        RefreshSettings::new().with_redraw_mode(RedrawMode::FullOnRequest),
    )?;
    let mut screen = Screen::new();
    assert_eq!(
        draw!(s, &mut screen, 1, empty(), no_scene, no_service),
        Ok(DrawOutcome::Drawn)
    );
    assert_eq!(
        draw!(s, &mut screen, 1, empty(), no_scene, no_service),
        Ok(DrawOutcome::Idle)
    );
    s.invalidate(Rectangle::new(Point::new(1, 0), Size::new(1, 1)));
    assert_eq!(
        draw!(s, &mut screen, 1, empty(), |_, _| Err(42_u8), no_service),
        Err(SessionError::Refresh(CooperativeError::Refresh(
            RefreshError::Scene(42)
        )))
    );
    assert_eq!(s.pending(), Some(Rect::new(0, 0, 5, 2)));
    assert_eq!(
        draw!(s, &mut screen, 1, empty(), no_scene, no_service),
        Ok(DrawOutcome::Drawn)
    );
    assert_eq!(screen.writes, 4);
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AssetError {
    Read,
    Decode,
}
#[derive(Debug)]
struct Reader {
    bytes: [u8; 10],
    calls: usize,
    fail_at: Option<usize>,
}
impl moteframe_core::ReadAt for Reader {
    type Error = AssetError;
    fn capacity(&self) -> u32 {
        10
    }
    fn read_exact(&mut self, offset: u32, out: &mut [u8]) -> Result<(), AssetError> {
        let offset = usize::try_from(offset).map_err(|_| AssetError::Read)?;
        let end = offset.checked_add(out.len()).ok_or(AssetError::Read)?;
        let bytes = self.bytes.get(offset..end).ok_or(AssetError::Read)?;
        self.calls += 1;
        if self.fail_at == Some(self.calls) {
            out.fill(255);
            return Err(AssetError::Read);
        }
        out.copy_from_slice(bytes);
        Ok(())
    }
}
// A borrowed two-byte cache window, not a full asset buffer or production codec.
struct Window<'a> {
    source: &'a mut Reader,
    bytes: [u8; 2],
}
impl Window<'_> {
    fn read(&mut self, offset: u32, count: usize) -> Result<&[u8], AssetError> {
        use moteframe_core::ReadAt;
        let out = self.bytes.get_mut(..count).ok_or(AssetError::Read)?;
        self.source.read_exact(offset, out)?;
        Ok(out)
    }
}
fn asset_scene(
    tile: &mut moteframe_ui::Tile<'_, Gray8>,
    context: &moteframe_ui::UiContext<'_, Gray8, core::cell::RefCell<Reader>>,
) -> Result<(), AssetError> {
    let bounds = tile.bounding_box();
    let x = u32::try_from(bounds.top_left.x).map_err(|_| AssetError::Read)?;
    let y = u32::try_from(bounds.top_left.y).map_err(|_| AssetError::Read)?;
    let width = usize::try_from(bounds.size.width).map_err(|_| AssetError::Read)?;
    let mut reader = context.resources().borrow_mut();
    let mut window = Window {
        source: &mut reader,
        bytes: [0; 2],
    };
    for (i, &byte) in window.read(y * 5 + x, width)?.iter().enumerate() {
        let color = match byte {
            0 => Gray8::new(0),
            1 => Gray8::new(7),
            _ => return Err(AssetError::Decode),
        };
        let dx = i32::try_from(i).map_err(|_| AssetError::Read)?;
        tile.draw_iter([Pixel(bounds.top_left + Point::new(dx, 0), color)])
            .map_err(|never| match never {})?;
    }
    Ok(())
}

#[test]
fn small_read_windows_propagate_actual_io_failures_and_retry_whole_frame() -> Result<(), ConfigError>
{
    for fail_at in 1..=6 {
        let provider = core::cell::RefCell::new(Reader {
            bytes: [0, 1, 0, 1, 0, 1, 0, 1, 0, 1],
            calls: 0,
            fail_at: Some(fail_at),
        });
        let key = moteframe_ui::ResourceKey::new(core::num::NonZeroU32::MIN, 1);
        let view = ResourceView::new(
            key,
            moteframe_ui::ResourceMetadata::new(5, 2, 1)?,
            &provider,
        )?;
        let candidate = settings().with_resources(key);
        let mut ui = UiState::new(candidate, Dimensions::new(5, 2), view)?;
        let mut scratch = [Gray8::new(0); 2];
        let mut s = RefreshSession::new(&mut ui, &mut scratch, RefreshSettings::new())?;
        let mut screen = Screen::new();
        assert_eq!(
            draw!(s, &mut screen, 1, view, asset_scene, no_service),
            Err(SessionError::Refresh(CooperativeError::Refresh(
                RefreshError::Scene(AssetError::Read)
            )))
        );
        assert_eq!(screen.writes, fail_at - 1);
        assert_eq!(s.pending(), Some(Rect::new(0, 0, 5, 2)));
        provider.borrow_mut().fail_at = None; // Only transport fault changes; asset contents/key remain stable.
        assert_eq!(
            draw!(s, &mut screen, 1, view, asset_scene, no_service),
            Ok(DrawOutcome::Drawn)
        );
        assert_eq!(
            screen.pixels,
            vec![
                Gray8::new(0),
                Gray8::new(7),
                Gray8::new(0),
                Gray8::new(7),
                Gray8::new(0),
                Gray8::new(7),
                Gray8::new(0),
                Gray8::new(7),
                Gray8::new(0),
                Gray8::new(7)
            ]
        );
        assert_eq!(s.pending(), None);
    }
    Ok(())
}

#[test]
fn actual_decode_failure_keeps_frame_pending() -> Result<(), ConfigError> {
    let provider = core::cell::RefCell::new(Reader {
        bytes: [0, 1, 0, 1, 0, 1, 0, 1, 0, 9],
        calls: 0,
        fail_at: None,
    });
    let key = moteframe_ui::ResourceKey::new(core::num::NonZeroU32::MIN, 1);
    let view = ResourceView::new(
        key,
        moteframe_ui::ResourceMetadata::new(5, 2, 1)?,
        &provider,
    )?;
    let mut ui = UiState::new(settings().with_resources(key), Dimensions::new(5, 2), view)?;
    let mut scratch = [Gray8::new(0); 2];
    let mut s = RefreshSession::new(&mut ui, &mut scratch, RefreshSettings::new())?;
    let mut screen = Screen::new();
    assert_eq!(
        draw!(s, &mut screen, 1, view, asset_scene, no_service),
        Err(SessionError::Refresh(CooperativeError::Refresh(
            RefreshError::Scene(AssetError::Decode)
        )))
    );
    assert_eq!(screen.writes, 5);
    assert_eq!(s.pending(), Some(Rect::new(0, 0, 5, 2)));
    Ok(())
}

#[test]
fn updated_background_and_resource_revision_reach_the_next_complete_frame()
-> Result<(), ConfigError> {
    let mut ui = UiState::new(settings(), Dimensions::new(5, 2), empty())?;
    let mut scratch = [Gray8::new(0); 3];
    let mut s = RefreshSession::new(&mut ui, &mut scratch, policy(2)?)?;
    let mut screen = Screen::new();
    assert_eq!(
        draw!(s, &mut screen, 1, empty(), no_scene, no_service),
        Ok(DrawOutcome::Drawn)
    );
    for (revision, value) in [(1, 7), (2, 9)] {
        let key = moteframe_ui::ResourceKey::new(core::num::NonZeroU32::MIN, revision);
        let provider = [value];
        let view = ResourceView::new(
            key,
            moteframe_ui::ResourceMetadata::new(1, 1, 1)?,
            &provider,
        )?;
        let candidate = settings().with_resources(key).with_palette(Palette::new(
            Gray8::new(8),
            Gray8::new(1),
            Gray8::new(2),
            Gray8::new(3),
            Gray8::new(4),
        ));
        assert_eq!(
            s.try_update(candidate, policy(1)?, view)?,
            UpdateImpact::FullRedraw
        );
        let writes = screen.writes;
        assert_eq!(
            draw!(s, &mut screen, 1, empty(), no_scene, no_service),
            Err(SessionError::Config(ConfigError::ResourceMismatch))
        );
        assert_eq!(screen.writes, writes);
        assert_eq!(
            draw!(
                s,
                &mut screen,
                1,
                view,
                |tile, ctx| tile
                    .draw_iter([Pixel(Point::new(0, 0), Gray8::new(ctx.resources()[0]))])
                    .map_err(|never| match never {}),
                no_service
            ),
            Ok(DrawOutcome::Drawn)
        );
        let mut expected = vec![Gray8::new(8); 10];
        expected[0] = Gray8::new(value);
        assert_eq!(screen.pixels, expected);
        assert_eq!(screen.writes, writes + 10);
        assert_eq!(
            s.try_update(candidate, policy(1)?, view)?,
            UpdateImpact::Unchanged
        );
        assert_eq!(s.pending(), None);
    }
    Ok(())
}
