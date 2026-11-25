//! Checked, borrowed asset bundle; generated binary data stays outside the repository.
/// Stable ordering in the version1 reference bundle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AssetId {
    /// Style2 digit strip.
    DialDigits,
    /// Function icons (including unused frame0).
    MenuIcons,
    /// Countdown label.
    MenuCountdown,
    /// Stopwatch label.
    MenuStopwatch,
    /// Weather label.
    MenuWeather,
    /// Settings label.
    MenuSettings,
    /// Settings icon strip.
    SettingsIcons,
    /// Information label.
    SetInfo,
    /// Dial selection label.
    SetDial,
    /// Brightness label.
    SetLight,
    /// Font label.
    SetFont,
    /// Language label.
    SetLanguage,
    /// Clear label.
    SetClear,
    /// Restart label.
    SetReboot,
    /// Power label.
    SetPower,
    /// Brightness title.
    LightTitle,
    /// Small clock digits.
    HeaderDigits,
    /// Small clock colon.
    HeaderColon,
    /// Brightness background.
    LightBackground,
    /// Small level strip.
    LightSmall,
    /// Selected level strip.
    LightDigits,
    /// Confirm button.
    Confirm,
}
/// Missing, inconsistent or truncated asset bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AssetError;
/// Validated version1 bundle with no allocations or ownership of source assets.
#[derive(Clone, Copy, Debug)]
pub struct Bundle<'a> {
    bytes: &'a [u8],
}
const COUNT: usize = 22;
const HEADER: usize = 10 + COUNT * 12;
fn number(bytes: &[u8], at: usize, count: usize) -> Result<u32, AssetError> {
    let mut value = 0_u32;
    for byte in bytes
        .get(at..at.checked_add(count).ok_or(AssetError)?)
        .ok_or(AssetError)?
    {
        value = (value << 8) | u32::from(*byte);
    }
    Ok(value)
}
impl<'a> Bundle<'a> {
    /// Checks every indexed image and its complete RGB565 pixel range.
    /// # Errors
    /// Rejects wrong version/count, zero extents, header overlap and truncated ranges.
    pub fn new(bytes: &'a [u8]) -> Result<Self, AssetError> {
        if bytes.get(..8) != Some(b"W5UI\x01\0\0\0") || number(bytes, 8, 2)? != 22 {
            return Err(AssetError);
        }
        let this = Self { bytes };
        for i in 0..COUNT {
            let (w, h, offset, len) = this.record(i)?;
            let expected = usize::from(w)
                .checked_mul(usize::from(h))
                .and_then(|n| n.checked_mul(2))
                .ok_or(AssetError)?;
            if w == 0
                || h == 0
                || len != expected
                || offset < HEADER
                || bytes
                    .get(offset..offset.checked_add(len).ok_or(AssetError)?)
                    .is_none()
            {
                return Err(AssetError);
            }
        }
        Ok(this)
    }
    fn record(&self, index: usize) -> Result<(u16, u16, usize, usize), AssetError> {
        let at = 10 + index * 12;
        Ok((
            u16::try_from(number(self.bytes, at, 2)?).map_err(|_| AssetError)?,
            u16::try_from(number(self.bytes, at + 2, 2)?).map_err(|_| AssetError)?,
            usize::try_from(number(self.bytes, at + 4, 4)?).map_err(|_| AssetError)?,
            usize::try_from(number(self.bytes, at + 8, 4)?).map_err(|_| AssetError)?,
        ))
    }
    /// Borrows one full-width frame from a vertical strip, MSB-first RGB565.
    /// # Errors
    /// Rejects mismatched geometry, partial frames and an out-of-range index.
    pub fn frame(
        &self,
        id: AssetId,
        width: u16,
        height: u16,
        index: u8,
    ) -> Result<&'a [u8], AssetError> {
        let (w, h, offset, _) = self.record(id as usize)?;
        if width != w || height == 0 || !h.is_multiple_of(height) || u16::from(index) >= h / height
        {
            return Err(AssetError);
        }
        let len = usize::from(width)
            .checked_mul(usize::from(height))
            .and_then(|n| n.checked_mul(2))
            .ok_or(AssetError)?;
        let start = offset
            .checked_add(usize::from(index).checked_mul(len).ok_or(AssetError)?)
            .ok_or(AssetError)?;
        self.bytes
            .get(start..start.checked_add(len).ok_or(AssetError)?)
            .ok_or(AssetError)
    }
}
