//! Original C coordinates; platform state and original assets are supplied by callers.
use crate::{AssetError, AssetId as Id, Bundle, Model, Page};
use device_ui::graphics::{
    image::{Image, ImageRawBE},
    pixelcolor::Rgb565,
    prelude::*,
    primitives::Rectangle,
};
/// Asset validation or drawing failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderError<E> {
    /// Invalid or missing asset frame.
    Asset(AssetError),
    /// Drawing target error.
    Target(E),
}
fn sprite<T: DrawTarget<Color = Rgb565>>(
    assets: &Bundle<'_>,
    target: &mut T,
    id: Id,
    xy: (i32, i32),
    size: (u16, u16),
    frame: u8,
) -> Result<(), RenderError<T::Error>> {
    let bytes = assets
        .frame(id, size.0, size.1, frame)
        .map_err(RenderError::Asset)?;
    let raw = ImageRawBE::<Rgb565>::new(bytes, u32::from(size.0));
    let position = Point::new(xy.0, xy.1);
    let visible = Rectangle::new(position, Size::new(u32::from(size.0), u32::from(size.1)))
        .intersection(&target.bounding_box());
    if !visible.is_zero_sized() {
        let source = Rectangle::new(visible.top_left - position, visible.size);
        let part = raw.sub_image(&source);
        Image::new(&part, visible.top_left)
            .draw(target)
            .map_err(RenderError::Target)?;
    }
    Ok(())
}
impl Model {
    /// Draws this128x128 visual snapshot; caller clears background and schedules tiles.
    /// # Errors
    /// Returns missing/malformed assets or the target's first drawing error.
    pub fn draw<T: DrawTarget<Color = Rgb565>>(
        self,
        assets: &Bundle<'_>,
        target: &mut T,
    ) -> Result<(), RenderError<T::Error>> {
        let (hour, minute) = self.time();
        match self.page() {
            Page::Dial => {
                for ((x, y), n) in [(5, 0), (69, 0), (5, 65), (69, 65)].into_iter().zip([
                    hour / 10,
                    hour % 10,
                    minute / 10,
                    minute % 10,
                ]) {
                    sprite(assets, target, Id::DialDigits, (x, y), (53, 63), n)?;
                }
            }
            Page::Menu | Page::Settings => {
                let settings = self.page() == Page::Settings;
                let (base, total) = if settings {
                    (self.settings_index() * 3, 8)
                } else {
                    (self.menu_index() * 3, 4)
                };
                let icons = if settings {
                    Id::SettingsIcons
                } else {
                    Id::MenuIcons
                };
                let labels: &[Id] = if settings {
                    &[
                        Id::SetInfo,
                        Id::SetDial,
                        Id::SetLight,
                        Id::SetFont,
                        Id::SetLanguage,
                        Id::SetClear,
                        Id::SetReboot,
                        Id::SetPower,
                    ]
                } else {
                    &[
                        Id::MenuCountdown,
                        Id::MenuStopwatch,
                        Id::MenuWeather,
                        Id::MenuSettings,
                    ]
                };
                for row in 0..3_u8 {
                    let item = base + row;
                    if item >= total {
                        break;
                    }
                    sprite(
                        assets,
                        target,
                        icons,
                        (4, 6 + 40 * i32::from(row)),
                        (36, 36),
                        item + u8::from(!settings),
                    )?;
                }
                for row in 0..3_u8 {
                    let item = base + row;
                    if item >= total {
                        break;
                    }
                    let y = [16, 55, 97][usize::from(row)];
                    sprite(
                        assets,
                        target,
                        labels[usize::from(item)],
                        (46, y),
                        (80, 21),
                        0,
                    )?;
                }
            }
            Page::Brightness => self.draw_brightness(assets, target)?,
        }
        Ok(())
    }
}

impl Model {
    fn draw_brightness<T: DrawTarget<Color = Rgb565>>(
        self,
        assets: &Bundle<'_>,
        target: &mut T,
    ) -> Result<(), RenderError<T::Error>> {
        let (hour, minute) = self.time();

        sprite(assets, target, Id::LightTitle, (1, 0), (70, 19), 0)?;
        for (x, n) in [
            (76, hour / 10),
            (88, hour % 10),
            (104, minute / 10),
            (115, minute % 10),
        ] {
            sprite(assets, target, Id::HeaderDigits, (x, 3), (10, 13), n)?;
        }
        sprite(assets, target, Id::HeaderColon, (100, 3), (4, 13), 0)?;
        sprite(assets, target, Id::LightBackground, (0, 49), (128, 24), 0)?;
        sprite(
            assets,
            target,
            Id::LightSmall,
            (59, 33),
            (10, 11),
            (self.level() + 4) % 5,
        )?;
        sprite(
            assets,
            target,
            Id::LightDigits,
            (57, 55),
            (13, 14),
            self.level(),
        )?;
        sprite(
            assets,
            target,
            Id::LightSmall,
            (59, 83),
            (10, 11),
            (self.level() + 1) % 5,
        )?;
        sprite(assets, target, Id::Confirm, (0, 103), (128, 24), 0)?;

        Ok(())
    }
}
