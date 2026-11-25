//! Prepared immutable scene: validate once, then render bounded rows without revalidating text.
use crate::{
    Face, Label as L, Locale, Model, Screen, Snapshot, UiError,
    canvas::{COLORS, Canvas, RenderError},
    labels::title,
    model::menu,
};
use device_ui::graphics::{pixelcolor::Rgb565, prelude::*};
/// Borrowed immutable data and a small model copy; contains no framebuffer.
#[derive(Clone, Copy, Debug)]
pub struct Scene<'s, 'a, const PRODUCT: bool = false> {
    pub(crate) model: Model,
    pub(crate) data: &'s Snapshot<'a>,
}
impl Model {
    /// Validates caller data once for repeated row rendering.
    /// # Errors
    /// Invalid snapshots or unavailable glyphs reject before any display write.
    pub fn scene<'s, 'a>(&self, d: &'s Snapshot<'a>) -> Result<Scene<'s, 'a>, UiError> {
        let mut model = *self;
        model.inject(d)?;
        crate::text_layout::validate(&model, d)?;
        Ok(Scene { model, data: d })
    }
    /// Product layout with unbound mandatory scalar observations shown as missing.
    ///
    /// Health, sleep, weather and messages retain explicit caller-owned optional
    /// values. Clock, battery, activity, workout and update/charge providers are
    /// currently unbound; their scalar placeholders never become observations.
    /// Demo labels are suppressed without changing `scene` reference rendering.
    /// # Errors
    /// Invalid snapshots or unavailable glyphs reject before any display write.
    pub fn product_scene<'s, 'a>(
        &self,
        d: &'s Snapshot<'a>,
    ) -> Result<Scene<'s, 'a, true>, UiError> {
        let scene = self.scene(d)?;
        Ok(Scene {
            model: scene.model,
            data: scene.data,
        })
    }
}
impl<const PRODUCT: bool> Scene<'_, '_, PRODUCT> {
    /// Draws only into the target bounding box; all scene coordinates stay128x128.
    /// # Errors
    /// Rejects geometry, formatting and display failures; target writes may already exist on a target error.
    pub fn draw<D: DrawTarget<Color = Rgb565>>(
        &self,
        target: &mut D,
    ) -> Result<(), RenderError<D::Error>> {
        let m = &self.model;
        let theme = if m.page == Screen::Theme {
            m.draft
        } else {
            m.appearance.theme as u8
        };
        let colors = COLORS
            .get(usize::from(theme))
            .ok_or(RenderError::Bounds)?
            .map(|p| Rgb565::from(device_ui::graphics::pixelcolor::raw::RawU16::new(p)));
        let clip = target.bounding_box();
        let mut c = Canvas::<_, PRODUCT> {
            #[cfg(test)]
            visited_rows: 0,
            target,
            clip,
            colors,
            base: true,
        };
        c.rect(0, 0, 128, 128, 0)?;
        c.base_page(m, self.data)?;
        c.base = false;
        if m.page == Screen::Dial {
            if !PRODUCT && m.appearance.locale == Locale::Zh && m.appearance.face == Face::Stack {
                c.rect(87, 72, 41, 16, 0)?;
                let w = [
                    L::WeekMon,
                    L::WeekTue,
                    L::WeekWed,
                    L::WeekThu,
                    L::WeekFri,
                    L::WeekSat,
                    L::WeekSun,
                ][self.data.weekday as usize];
                c.text(w.text(Locale::Zh), 90, 73, 1, 1)?;
                c.rect(87, 106, 41, 20, 0)?;
                c.text(L::Demo.text(Locale::Zh), 90, 107, 1, 3)?;
            }
            return Ok(());
        }
        if menu(m.page).is_some() || m.page == Screen::SportList {
            return c.menu_page(m);
        }
        c.heading(m)?;
        c.content(m, self.data)
    }
}
impl<D: DrawTarget<Color = Rgb565>, const PRODUCT: bool> Canvas<'_, D, PRODUCT> {
    pub(crate) fn heading(&mut self, m: &Model) -> Result<(), RenderError<D::Error>> {
        self.rect(0, 0, 128, 21, 0)?;
        self.text(title(m.page).text(m.appearance.locale), 8, 4, 1, 2)?;
        if !PRODUCT {
            self.text("DEMO", 97, 6, 1, 3)?;
        }
        self.rect(8, 19, 112, 1, 3)
    }
    pub(crate) fn primary(&mut self, m: &Model, id: L) -> Result<(), RenderError<D::Error>> {
        self.rect(4, 100, 120, 26, 2)?;
        self.center(id.text(m.appearance.locale), 106, 1, 0)
    }
    pub(crate) fn pair(
        &mut self,
        m: &Model,
        left: L,
        right: L,
    ) -> Result<(), RenderError<D::Error>> {
        self.rect(0, 96, 64, 32, 0)?;
        self.rect(64, 96, 64, 32, 2)?;
        let a = left.text(m.appearance.locale);
        let b = right.text(m.appearance.locale);
        self.text(a, (64 - Self::width(a, 1)?).div_euclid(2), 106, 1, 1)?;
        self.text(b, 64 + (64 - Self::width(b, 1)?).div_euclid(2), 106, 1, 0)
    }
    pub(crate) fn lc(
        &mut self,
        m: &Model,
        id: L,
        y: i32,
        scale: i32,
        c: usize,
    ) -> Result<(), RenderError<D::Error>> {
        self.center(id.text(m.appearance.locale), y, scale, c)
    }
    fn base_page(&mut self, m: &Model, d: &Snapshot<'_>) -> Result<(), RenderError<D::Error>> {
        match m.page {
            Screen::Dial => self.dial(m, d, m.appearance.face as u8, false)?,
            Screen::Face if m.draft < 3 => {
                self.dial(m, d, m.draft, true)?;
                self.rect(0, 100, 128, 28, 0)?;
            }
            Screen::Brightness => {
                self.header("BRIGHT", if PRODUCT { "" } else { "DEMO" })?;
                self.formatted(format_args!("{}", m.draft + 1), None, 29, 5, 1)?;
                for i in 0..5 {
                    self.rect(
                        12 + i * 22,
                        77,
                        16,
                        12,
                        if i <= i32::from(m.draft) { 2 } else { 3 },
                    )?;
                }
            }
            Screen::Theme => {
                self.formatted(format_args!("{}/3", m.draft + 1), Some(104), 7, 1, 3)?;
                self.center(["PHOSPHOR", "AMBER", "ICE"][usize::from(m.draft)], 34, 2, 1)?;
                for i in 0..4 {
                    self.rect(
                        16 + i * 26,
                        65,
                        18,
                        18,
                        usize::try_from(i).map_err(|_| RenderError::Bounds)?,
                    )?;
                }
            }
            Screen::Format => {
                self.header("FORMAT", if PRODUCT { "" } else { "DEMO" })?;
                self.center(if m.draft == 0 { "24H" } else { "12H" }, 32, 3, 1)?;
                self.center(
                    if PRODUCT {
                        "--:--"
                    } else if m.draft == 0 {
                        "22:50"
                    } else {
                        "10:50 PM"
                    },
                    74,
                    1,
                    2,
                )?;
            }
            _ => {}
        }
        Ok(())
    }
    fn menu_page(&mut self, m: &Model) -> Result<(), RenderError<D::Error>> {
        let sport = [L::Cycling, L::Mountain, L::Badminton, L::Run];
        let count = if m.page == Screen::SportList {
            4
        } else {
            menu(m.page).map_or(0, <[Screen]>::len)
        };
        for row in 0..3_u8 {
            let i = usize::from(m.nav) * 3 + usize::from(row);
            let id = if m.page == Screen::SportList {
                sport.get(i).copied()
            } else {
                menu(m.page).and_then(|a| a.get(i)).map(|s| title(*s))
            };
            if let Some(id) = id {
                let selected = row == m.focus;
                let y = i32::from(row) * 42;
                if selected {
                    self.rect(4, 4 + y, 120, 38, 2)?;
                }
                let s = id.text(m.appearance.locale);
                let scale = if s.is_ascii() && s.len() <= 9 { 2 } else { 1 };
                self.text(s, 10, 13 + y, scale, usize::from(!selected))?;
            }
        }
        self.formatted(
            format_args!("{}/{}", m.nav + 1, count.div_ceil(3)),
            Some(104),
            117,
            1,
            if count - usize::from(m.nav) * 3 == 3 && m.focus == 2 {
                0
            } else {
                3
            },
        )
    }
}
#[path = "render_pages.rs"]
mod pages;
