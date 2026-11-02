// Copyright 2026 MoteFrame contributors
// SPDX-License-Identifier: Apache-2.0
// See LICENSE and NOTICE for license and source attribution.

//! Fixed offline consumer. Layouts and frame metadata are not a product budget.
#![cfg_attr(target_os = "none", no_std)]
use core::hint::black_box;
#[cfg(any(not(target_os = "none"), ui_layout_probe))]
use core::mem::size_of;
use moteframe_core::{Dimensions, Display, Rect};
use moteframe_ui::{
    self as ui,
    graphics::{pixelcolor::Rgb565, prelude::*},
};

#[cfg(any(not(target_os = "none"), ui_layout_probe))]
use ui::graphics::pixelcolor::{BinaryColor, Gray8, Rgb888};
type C = Rgb565;
macro_rules! layout {
    ($name:ident, $ty:ty) => {
        #[doc = concat!("Compiler-emitted layout of ", stringify!($ty), ".")]
        #[cfg(any(not(target_os = "none"), ui_layout_probe))]
        #[used]
        pub static $name: [u8; size_of::<$ty>()] = [0; size_of::<$ty>()];
    };
}
layout!(MF_SETTINGS_565, ui::UiSettings<C>);
layout!(MF_SETTINGS_888, ui::UiSettings<Rgb888>);
layout!(MF_SETTINGS_GRAY, ui::UiSettings<Gray8>);
layout!(MF_SETTINGS_BINARY, ui::UiSettings<BinaryColor>);
layout!(MF_STATE, ui::UiState<C>);
layout!(MF_CONTEXT, ui::UiContext<'static, C, [u8; 1]>);
layout!(MF_RESOURCE_KEY, ui::ResourceKey);
layout!(MF_RESOURCE_META, ui::ResourceMetadata);
layout!(MF_RESOURCE_VIEW, ui::ResourceView<'static, [u8; 1]>);
layout!(MF_RUN_8, [C; 8]);
layout!(MF_RUN_32, [C; 32]);
layout!(MF_WORKSPACE, [C; 32]);
#[cfg(feature = "refresh")]
layout!(MF_SESSION, ui::RefreshSession<'static, C>);
#[cfg(feature = "refresh")]
layout!(MF_POLICY, ui::RefreshSettings);
#[cfg(feature = "refresh")]
layout!(MF_DEFERRED, ui::DeferredInvalidation);
#[cfg(feature = "frame-pacing")]
layout!(MF_PACING, ui::PacingState);

#[derive(Debug, Default)]
struct Screen {
    checksum: u32,
}
impl Display for Screen {
    type Color = C;
    type Error = ();
    fn dimensions(&self) -> Dimensions {
        Dimensions::new(8, 4)
    }
    fn write(&mut self, area: Rect, pixels: &[C]) -> Result<(), ()> {
        self.checksum = self.checksum.wrapping_add(area.area());
        for pixel in pixels {
            self.checksum = self.checksum.wrapping_add(u32::from(pixel.into_storage()));
        }
        Ok(())
    }
}
/// Exercises enabled configuration/drawing consumers with optimizer-opaque input.
/// # Errors
/// Returns metadata or configuration errors; transport results enter the checksum.
#[inline(never)]
pub fn exercise(seed: u32) -> Result<u32, ui::ConfigError> {
    let palette = ui::Palette::new(black_box(C::BLACK), C::WHITE, C::RED, C::GREEN, C::BLUE);
    let settings = ui::UiSettings::new(palette);
    let mut state = ui::UiState::new(
        black_box(settings),
        Dimensions::new(8, 4),
        ui::ResourceView::empty(&()),
    )?;
    let provider = [black_box(1u8)];
    let key = ui::ResourceKey::new(core::num::NonZeroU32::MIN, seed);
    let resources = ui::ResourceView::new(key, ui::ResourceMetadata::new(8, 4, 1)?, &provider)?;
    let candidate = black_box(settings.with_resources(key));
    let impact = state.try_update(candidate, resources)?;
    let context = state.context(resources)?;
    let mut screen = Screen::default();
    let bounds = context.display_bounds();
    ui::StreamTarget::<_, 8>::with_run(&mut screen)
        .fill_solid(
            &ui::graphics::primitives::Rectangle::new(
                Point::zero(),
                Size::new(u32::from(bounds.width), 1),
            ),
            context.settings().palette().foreground(),
        )
        .map_err(|()| ui::ConfigError::InvalidResources)?;
    #[cfg(feature = "layout")]
    {
        let mut stack = context
            .stack(ui::Axis::Horizontal)
            .map_err(|_| ui::ConfigError::InvalidPadding)?;
        let _ = black_box(stack.take(black_box(2)));
    }
    #[cfg(feature = "refresh")]
    {
        let mut scratch = [C::BLACK; 32];
        let policy = ui::RefreshSettings::new();
        #[cfg(feature = "frame-pacing")]
        let policy = policy.with_frame_interval(ui::FrameInterval::MinimumInterval(
            core::num::NonZeroU32::MIN,
        ));
        let mut session = ui::RefreshSession::new(&mut state, &mut scratch, black_box(policy))?;
        let scene = |tile: &mut ui::Tile<'_, C>, ctx: &ui::UiContext<'_, C, [u8; 1]>| {
            tile.fill_solid(
                &ui::graphics::primitives::Rectangle::new(Point::new(2, 1), Size::new(2, 2)),
                ctx.settings().palette().accent(),
            )
        };
        let service = |sink: &mut ui::DeferredInvalidation| {
            if seed & 1 != 0 {
                sink.invalidate_all();
            }
            Ok::<_, ()>(())
        };
        #[cfg(feature = "frame-pacing")]
        let result = session.draw_timed(
            ui::DisplayBinding::new(&mut screen, 1),
            resources,
            || u64::from(black_box(seed)),
            scene,
            service,
        );
        #[cfg(not(feature = "frame-pacing"))]
        let result = session.draw(
            ui::DisplayBinding::new(&mut screen, 1),
            resources,
            scene,
            service,
        );
        let _ = black_box(result);
        black_box(session.try_update(candidate, black_box(policy), resources)?);
        black_box(session.try_rebind(
            candidate,
            policy,
            ui::DisplayBinding::new(&mut screen, 2),
            resources,
        )?);
        black_box(session.pending());
    }
    #[cfg(feature = "damage")]
    {
        let mut rows = [C::BLACK; 16];
        let _ = black_box(ui::damage_cooperative(
            Dimensions::new(8, 4),
            &mut rows,
            C::BLACK,
            |_| Ok::<_, core::convert::Infallible>(()),
            |tile| tile.clear(black_box(C::WHITE)),
            || Ok::<_, ()>(()),
        ));
    }
    Ok(screen
        .checksum
        .wrapping_add(u32::from(impact == ui::UpdateImpact::FullRedraw)))
}

#[cfg(not(target_os = "none"))]
fn main() {
    macro_rules! emit {
        ($name:ident) => {
            println!("\"{}\":{},", stringify!($name), $name.len());
        };
    }
    println!("{{");
    emit!(MF_SETTINGS_565);
    emit!(MF_SETTINGS_888);
    emit!(MF_SETTINGS_GRAY);
    emit!(MF_SETTINGS_BINARY);
    emit!(MF_STATE);
    emit!(MF_CONTEXT);
    emit!(MF_RESOURCE_KEY);
    emit!(MF_RESOURCE_META);
    emit!(MF_RESOURCE_VIEW);
    emit!(MF_RUN_8);
    emit!(MF_RUN_32);
    emit!(MF_WORKSPACE);
    #[cfg(feature = "refresh")]
    {
        emit!(MF_SESSION);
        emit!(MF_POLICY);
        emit!(MF_DEFERRED);
    }
    #[cfg(feature = "frame-pacing")]
    emit!(MF_PACING);
    println!("\"checksum\":{} }}", exercise(black_box(1)).unwrap_or(0));
}
