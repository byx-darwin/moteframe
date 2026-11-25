//! Strict device-side weather object validation; unknown future App values require a separate tolerant view.
use crate::body::{Reader, Writer};
use device_protocol::ProtocolError;
/// One forecast day in the version1 weather dictionary.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct WeatherDay {
    /// Civil day number since 1970-01-01 in the declared fixed timezone.
    pub date_days: i32,
    /// 1 simple, 2 detailed.
    pub category: u8,
    /// Simple0..5 or detailed0..60; FFFF explicitly unknown for both.
    pub code: u16,
    /// Predicted high in centi-Celsius, present only when `valid_mask` bit0 is set.
    pub high_centi_c: i16,
    /// Current value in centi-Celsius, present only when bit1 is set.
    pub current_centi_c: i16,
    /// Predicted low in centi-Celsius, present only when bit2 is set.
    pub low_centi_c: i16,
    /// Bits0 high,1 current,2 low; absent fields canonical zero.
    pub valid_mask: u8,
}
impl core::fmt::Debug for WeatherDay {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("WeatherDay")
            .field("encoded_length", &14)
            .finish_non_exhaustive()
    }
}
impl WeatherDay {
    fn decode(bytes: &[u8]) -> Result<Self, ProtocolError> {
        if bytes.len() != 14 {
            return Err(ProtocolError::InvalidLength);
        }
        let date_days = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        let category = bytes[4];
        let code = u16::from_le_bytes([bytes[5], bytes[6]]);
        let high_centi_c = i16::from_le_bytes([bytes[7], bytes[8]]);
        let current_centi_c = i16::from_le_bytes([bytes[9], bytes[10]]);
        let low_centi_c = i16::from_le_bytes([bytes[11], bytes[12]]);
        let valid_mask = bytes[13];
        if valid_mask & !7 != 0 || !(1..=2).contains(&category) {
            return Err(ProtocolError::Reserved);
        }
        if code != u16::MAX && ((category == 1 && code > 5) || (category == 2 && code > 60)) {
            return Err(ProtocolError::InvalidState);
        }
        for (i, temp) in [high_centi_c, current_centi_c, low_centi_c]
            .iter()
            .enumerate()
        {
            if valid_mask & (1 << i) == 0 && *temp != 0 {
                return Err(ProtocolError::InvalidState);
            }
        }
        if valid_mask & 5 == 5 && low_centi_c > high_centi_c {
            return Err(ProtocolError::InvalidState);
        }
        Ok(Self {
            date_days,
            category,
            code,
            high_centi_c,
            current_centi_c,
            low_centi_c,
            valid_mask,
        })
    }
}
/// Bounded borrowed forecast. No freshness is asserted without a product age policy.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Weather<'a> {
    issued_utc_ms: i64,
    timezone_min: i16,
    days: &'a [u8],
}
impl core::fmt::Debug for Weather<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Weather")
            .field("day_count", &self.day_count())
            .finish_non_exhaustive()
    }
}
impl<'a> Weather<'a> {
    /// Decode a complete object; `max_days` comes from product capacity.
    /// # Errors
    /// Rejects invalid dates/dictionary/valid masks, timezone, empty or excessive day lists.
    pub fn decode(bytes: &'a [u8], max_days: u8) -> Result<Self, ProtocolError> {
        let mut r = Reader::new(bytes);
        let issued_utc_ms = r.i64()?;
        let timezone_bits = r.u16()?;
        let timezone_min = i16::from_le_bytes(timezone_bits.to_le_bytes());
        let count = r.u8()?;
        if !(-840..=840).contains(&timezone_min) || count == 0 || count > max_days {
            return Err(ProtocolError::InvalidLength);
        }
        let days = r.take(usize::from(count) * 14)?;
        r.finish()?;
        let mut previous = None;
        for bytes in days.chunks_exact(14) {
            let day = WeatherDay::decode(bytes)?;
            if previous.is_some_and(|p| day.date_days <= p) {
                return Err(ProtocolError::Sequence);
            }
            previous = Some(day.date_days);
        }
        Ok(Self {
            issued_utc_ms,
            timezone_min,
            days,
        })
    }
    /// Number of complete day entries.
    #[must_use]
    pub const fn day_count(&self) -> usize {
        self.days.len() / 14
    }
    /// Decode one already validated forecast day.
    /// # Errors
    /// Returns invalid length for an index outside the forecast.
    pub fn day(&self, index: usize) -> Result<WeatherDay, ProtocolError> {
        let start = index.checked_mul(14).ok_or(ProtocolError::InvalidLength)?;
        let end = start.checked_add(14).ok_or(ProtocolError::InvalidLength)?;
        WeatherDay::decode(
            self.days
                .get(start..end)
                .ok_or(ProtocolError::InvalidLength)?,
        )
    }
    /// Encode a validated forecast; time is issue-time, not expiry or measured freshness.
    /// # Errors
    /// Rejects product capacity or insufficient output storage.
    pub fn encode(&self, out: &mut [u8], max_days: u8) -> Result<usize, ProtocolError> {
        if self.day_count() > usize::from(max_days) {
            return Err(ProtocolError::Capacity);
        }
        let mut w = Writer::new(out);
        w.i64(self.issued_utc_ms)?;
        w.u16(u16::from_le_bytes(self.timezone_min.to_le_bytes()))?;
        w.u8(u8::try_from(self.day_count()).map_err(|_| ProtocolError::Capacity)?)?;
        w.put(self.days)?;
        Ok(w.len())
    }
}
