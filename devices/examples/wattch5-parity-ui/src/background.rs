//! Image identity and bounded dynamic overlays for the existing V2 design.
use crate::{Model, RenderError, Scene, Screen, Snapshot};
use device_ui::graphics::{pixelcolor::Rgb565, prelude::*, primitives::Rectangle};

impl Model {
    /// Stable image slot. Dynamic observations, timer values and request text are excluded.
    #[must_use]
    pub fn background_key(&self) -> u16 {
        use Screen as S;
        let variant = match self.page {
            S::Dial => self.appearance.face as u8 * 2 + self.appearance.format as u8,
            S::Face => self.draft * 2 + self.appearance.format as u8,
            S::Tools | S::Quick | S::Settings | S::SportList => self.nav * 3 + self.focus,
            S::Brightness | S::Language | S::Theme | S::Format | S::Textsize => self.draft,
            S::MeasureTips => self.measure_kind as u8,
            _ => 0,
        };
        (self.page as u16 * 2 + self.appearance.locale as u16) * 18 + u16::from(variant)
    }
    /// Paint the immutable product backdrop. Caller observations are never captured.
    /// # Errors
    /// Returns existing design geometry, glyph or display errors.
    pub fn draw_background<D: DrawTarget<Color = Rgb565>>(
        &self,
        out: &mut D,
    ) -> Result<(), RenderError<D::Error>> {
        let data = Snapshot::unavailable();
        let scene = Scene::<true> {
            model: *self,
            data: &data,
        };
        scene.draw(out)
    }
    /// Number of statically generated image variants for a page.
    #[must_use]
    pub const fn background_variants(page: Screen) -> u8 {
        use Screen as S;
        match page {
            S::Dial | S::Tools | S::SportList | S::MeasureTips => 6,
            S::Face => 8,
            S::Settings | S::Language => 9,
            S::Brightness => 5,
            S::Quick | S::Theme => 3,
            S::Format | S::Textsize => 2,
            _ => 1,
        }
    }
    /// Canonical host-only image-generation model. Never introduces a device service.
    /// # Errors
    /// Rejects a variant outside the page catalogue.
    #[cfg(feature = "asset-build")]
    pub fn background_model(
        page: Screen,
        locale: crate::Locale,
        variant: u8,
    ) -> Result<Self, crate::UiError> {
        use Screen as S;
        if variant >= Self::background_variants(page) {
            return Err(crate::UiError::Data);
        }
        let mut m = Self::default();
        m.page = page;
        m.appearance.locale = locale;
        match page {
            S::Dial | S::Face => {
                m.appearance.format = if variant.is_multiple_of(2) {
                    crate::TimeFormat::Hour24
                } else {
                    crate::TimeFormat::Hour12
                };
                if page == S::Dial {
                    m.appearance.face =
                        [crate::Face::Stack, crate::Face::Line, crate::Face::Segment]
                            [usize::from(variant / 2)];
                } else {
                    m.draft = variant / 2;
                }
            }
            S::Tools | S::Quick | S::Settings | S::SportList => {
                m.nav = variant / 3;
                m.focus = variant % 3;
            }
            S::Brightness | S::Language | S::Theme | S::Format | S::Textsize => m.draft = variant,
            S::MeasureTips => {
                m.measure_kind = [
                    crate::MeasureKind::Heart,
                    crate::MeasureKind::Pressure,
                    crate::MeasureKind::Oxygen,
                    crate::MeasureKind::Temperature,
                    crate::MeasureKind::Glucose,
                    crate::MeasureKind::Hba1c,
                ][usize::from(variant)];
            }
            _ => {}
        }
        Ok(m)
    }
}
impl Scene<'_, '_, true> {
    /// Prepared navigation identity, after snapshot-induced page transitions.
    #[must_use]
    pub const fn background_model(&self) -> &Model {
        &self.model
    }
    /// Selected design palette; image pixels are stored in canonical Phosphor colors.
    #[must_use]
    pub fn palette(&self) -> [Rgb565; 4] {
        let theme = if self.model.page == Screen::Theme {
            self.model.draft
        } else {
            self.model.appearance.theme as u8
        };
        crate::canvas::COLORS[usize::from(theme)]
            .map(|v| Rgb565::from(device_ui::graphics::pixelcolor::raw::RawU16::new(v)))
    }
    /// Repaint only data-owned cells. Each cell clears old digits from panel GRAM.
    /// Static labels/icons outside these cells remain from the loaded image.
    /// # Errors
    /// Returns existing renderer/display errors; partial output must be retried.
    pub fn draw_dynamic<D: DrawTarget<Color = Rgb565>>(
        &self,
        target: &mut D,
    ) -> Result<(), RenderError<D::Error>> {
        let regions = self.dynamic_bands();
        for &(y, h) in regions {
            let area = Rectangle::new(Point::new(0, y), Size::new(128, h))
                .intersection(&target.bounding_box());
            if area.size != Size::zero() {
                self.draw(&mut target.clipped(&area))?;
            }
        }
        Ok(())
    }
    fn dynamic_bands(&self) -> &'static [(i32, u32)] {
        use Screen as S;
        match self.model.page {
            S::Heart | S::Pressure | S::Oxygen | S::Temperature | S::Glucose | S::Hba1c => {
                &[(31, 36), (82, 14), (108, 20)]
            }
            S::Activity => &[(27, 28), (77, 51)],
            S::Sleep => &[(31, 36), (89, 39)],
            S::Weather => &[(49, 22), (89, 14)],
            S::About => &[(28, 60)],
            S::DeviceQr => &[(23, 92)],
            S::TimerCustom => &[(29, 62)],
            S::Messages
            | S::MessageDetail
            | S::NoticeApp
            | S::TimerRun
            | S::Stopwatch
            | S::SportControls => &[(21, 107)],
            S::Request => &[(57, 28)],
            _ => &[],
        }
    }
    /// Draw a single bounded row without introducing a second renderer target type.
    /// # Errors
    /// Rejects a non-row target, or returns the existing render/display failure.
    pub fn draw_dynamic_row<D: DrawTarget<Color = Rgb565>>(
        &self,
        target: &mut D,
    ) -> Result<(), RenderError<D::Error>> {
        let area = target.bounding_box();
        if area.size.height != 1 {
            return Err(RenderError::Bounds);
        }
        if self.dynamic_bands().iter().any(|&(y, h)| {
            area.top_left.y >= y && area.top_left.y < y + i32::try_from(h).unwrap_or(128)
        }) {
            self.draw(target)?;
        }
        Ok(())
    }
}
