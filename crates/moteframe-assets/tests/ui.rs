//! Real assets/storage/UI composition with separate metadata and mutable reader.
#![allow(clippy::unwrap_used)] // Assertions turn any unexpected fixture error into a failed test.
use core::num::NonZeroU32;
use embedded_graphics_core::geometry::Dimensions as _;
use embedded_graphics_core::{
    pixelcolor::{Gray8, GrayColor},
    prelude::*,
};
use moteframe_assets::{AssetError, DecodeError, Gray8Bytes, PixelFormat, Raster};
use moteframe_core::{Dimensions, Display, ReadAt, Rect};
use moteframe_storage::{ReadError, ReadWindow};
use moteframe_ui::{
    ConfigError, CooperativeError, DisplayBinding, DrawOutcome, Palette, RefreshError,
    RefreshSession, RefreshSettings, ResourceKey, ResourceMetadata, ResourceView, SessionError,
    UiSettings, UiState, UpdateImpact,
};

struct Reader {
    bytes: [u8; 9],
    calls: usize,
    fail_at: Option<usize>,
}
impl ReadAt for Reader {
    type Error = u8;
    fn capacity(&self) -> u32 {
        9
    }
    fn read_exact(&mut self, offset: u32, out: &mut [u8]) -> Result<(), u8> {
        self.calls += 1;
        if self.fail_at == Some(self.calls) {
            return Err(53);
        }
        let start = usize::try_from(offset).map_err(|_| 54)?;
        out.copy_from_slice(&self.bytes[start..start + out.len()]);
        Ok(())
    }
}
struct Screen([Gray8; 8]);
impl Display for Screen {
    type Color = Gray8;
    type Error = u8;
    fn dimensions(&self) -> Dimensions {
        Dimensions::new(4, 2)
    }
    fn write(&mut self, rect: Rect, pixels: &[Gray8]) -> Result<(), u8> {
        let start = usize::from(rect.y) * 4 + usize::from(rect.x);
        self.0[start..start + pixels.len()].copy_from_slice(pixels);
        Ok(())
    }
}
fn settings(key: ResourceKey) -> UiSettings<Gray8> {
    UiSettings::new(Palette::new(
        Gray8::new(0),
        Gray8::new(1),
        Gray8::new(2),
        Gray8::new(3),
        Gray8::new(4),
    ))
    .with_resources(key)
}
type DrawResult = Result<DrawOutcome, SessionError<u8, AssetError<ReadError<u8>>, u8>>;
fn draw<F: PixelFormat<Color = Gray8>>(
    session: &mut RefreshSession<'_, Gray8>,
    screen: &mut Screen,
    view: ResourceView<'_, Raster<F>>,
    reader: &mut ReadWindow<Reader>,
) -> DrawResult {
    session.draw(
        DisplayBinding::new(screen, 1),
        view,
        |tile, ctx| {
            let rect = tile.bounding_box();
            let mut output = [Gray8::new(0); 2];
            let mut bytes = [0; 1];
            let count = usize::try_from(rect.size.width).map_err(|_| AssetError::Bounds)?;
            let x = u16::try_from(rect.top_left.x).map_err(|_| AssetError::Bounds)?;
            let y = u16::try_from(rect.top_left.y).map_err(|_| AssetError::Bounds)?;
            ctx.resources()
                .read_span(reader, x, y, &mut output[..count], &mut bytes)?;
            tile.fill_contiguous(&rect, output[..count].iter().copied())
                .map_err(|never| match never {})
        },
        |_| Ok(()),
    )
}

struct StrictGray;
impl PixelFormat for StrictGray {
    type Color = Gray8;
    const BYTES_PER_PIXEL: usize = 1;
    fn decode(bytes: &[u8]) -> Result<Gray8, DecodeError> {
        let color = Gray8Bytes::decode(bytes)?;
        if color.luma() == 255 {
            Err(DecodeError::InvalidValue)
        } else {
            Ok(color)
        }
    }
}

#[test]
fn decoder_failure_retains_full_redraw_and_repaired_revision_repaints_prefix_too()
-> Result<(), ConfigError> {
    let raster = Raster::<StrictGray>::new(0, 4, 2).unwrap();
    let key = ResourceKey::new(NonZeroU32::MIN, 1);
    let view = ResourceView::new(key, ResourceMetadata::new(4, 2, 1)?, &raster)?;
    let mut state = UiState::new(settings(key), Dimensions::new(4, 2), view)?;
    let mut pixels = [Gray8::new(0); 2];
    let mut session = RefreshSession::new(&mut state, &mut pixels, RefreshSettings::new())?;
    let mut reader = ReadWindow::new(
        Reader {
            bytes: [99, 10, 20, 30, 255, 50, 60, 70, 80],
            calls: 0,
            fail_at: None,
        },
        1,
        8,
    )
    .unwrap();
    let mut screen = Screen([Gray8::new(0); 8]);
    assert_eq!(
        draw(&mut session, &mut screen, view, &mut reader),
        Err(SessionError::Refresh(CooperativeError::Refresh(
            RefreshError::Scene(AssetError::Decode(DecodeError::InvalidValue))
        )))
    );
    assert_eq!(session.pending(), Some(Rect::new(0, 0, 4, 2)));
    assert_eq!(
        screen.0.map(|pixel| pixel.luma()),
        [10, 20, 0, 0, 0, 0, 0, 0]
    );
    let mut backing = reader.into_inner();
    assert_eq!(backing.calls, 4);
    backing.bytes[4] = 40;
    reader = ReadWindow::new(backing, 1, 8).unwrap();
    let key2 = ResourceKey::new(NonZeroU32::MIN, 2);
    let view2 = ResourceView::new(key2, ResourceMetadata::new(4, 2, 1)?, &raster)?;
    assert_eq!(
        session.try_update(settings(key2), RefreshSettings::new(), view2)?,
        UpdateImpact::FullRedraw
    );
    assert_eq!(
        draw(&mut session, &mut screen, view2, &mut reader),
        Ok(DrawOutcome::Drawn)
    );
    assert_eq!(
        screen.0.map(|pixel| pixel.luma()),
        [10, 20, 30, 40, 50, 60, 70, 80]
    );
    assert_eq!(reader.into_inner().calls, 12);
    assert_eq!(session.pending(), None);
    Ok(())
}
#[test]
fn failed_asset_read_retains_redraw_and_revision_changes_refresh_exact_pixels()
-> Result<(), ConfigError> {
    let raster = Raster::<Gray8Bytes>::new(0, 4, 2).unwrap();
    let key = ResourceKey::new(NonZeroU32::MIN, 1);
    let view = ResourceView::new(key, ResourceMetadata::new(4, 2, 1)?, &raster)?;
    let mut state = UiState::new(settings(key), Dimensions::new(4, 2), view)?;
    let mut pixels = [Gray8::new(0); 2];
    let mut session = RefreshSession::new(&mut state, &mut pixels, RefreshSettings::new())?;
    let mut reader = ReadWindow::new(
        Reader {
            bytes: [99, 10, 20, 30, 40, 50, 60, 70, 80],
            calls: 0,
            fail_at: Some(4),
        },
        1,
        8,
    )
    .unwrap();
    let mut screen = Screen([Gray8::new(0); 8]);
    assert_eq!(
        draw(&mut session, &mut screen, view, &mut reader),
        Err(SessionError::Refresh(CooperativeError::Refresh(
            RefreshError::Scene(AssetError::Source(ReadError::Source(53)))
        )))
    );
    assert_eq!(session.pending(), Some(Rect::new(0, 0, 4, 2)));
    assert_eq!(
        draw(&mut session, &mut screen, view, &mut reader),
        Ok(DrawOutcome::Drawn)
    );
    assert_eq!(
        screen.0.map(|pixel| pixel.luma()),
        [10, 20, 30, 40, 50, 60, 70, 80]
    );
    assert_eq!(session.pending(), None);
    let key2 = ResourceKey::new(NonZeroU32::MIN, 2);
    let view2 = ResourceView::new(key2, ResourceMetadata::new(4, 2, 1)?, &raster)?;
    assert_eq!(
        session.try_update(settings(key2), RefreshSettings::new(), view2)?,
        UpdateImpact::FullRedraw
    );
    reader = ReadWindow::new(
        Reader {
            bytes: [99, 80, 70, 60, 50, 40, 30, 20, 10],
            calls: 0,
            fail_at: None,
        },
        1,
        8,
    )
    .unwrap();
    assert_eq!(
        draw(&mut session, &mut screen, view, &mut reader),
        Err(SessionError::Config(ConfigError::ResourceMismatch))
    );
    assert_eq!(
        draw(&mut session, &mut screen, view2, &mut reader),
        Ok(DrawOutcome::Drawn)
    );
    assert_eq!(
        screen.0.map(|pixel| pixel.luma()),
        [80, 70, 60, 50, 40, 30, 20, 10]
    );
    Ok(())
}
