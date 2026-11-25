//! Phone-owned notification content, separate from ANCS and call hardware.
use crate::body::{Reader, Writer};
use device_protocol::ProtocolError;
/// Optional notification object schema2. IDs are scoped to the authorized owner/connection.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Notification<'a> {
    /// Nonzero phone notification identity, distinct from stable object ID.
    pub notification: u32,
    /// 0 other, 1 incoming call, 2 missed call, 3 SMS, 4 email, 5 social, 6 calendar, 7 system.
    pub category: u16,
    /// Source application identifier.
    pub source_app: &'a str,
    /// Sender display label.
    pub sender: &'a str,
    /// Source telephone number or empty.
    pub number: &'a str,
    /// Short content title.
    pub title: &'a str,
    /// Content body.
    pub body: &'a str,
    /// Incoming-call-only bits0 answer,1 reject,2 mute; backend support checked separately.
    pub action_mask: u16,
    /// Phone-supplied UTC timestamp.
    pub created_utc_ms: i64,
    /// Whether the timestamp is available; false requires zero timestamp.
    pub created_time_valid: bool,
}
impl core::fmt::Debug for Notification<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Notification")
            .field("title_length", &self.title.len())
            .field("body_length", &self.body.len())
            .finish_non_exhaustive()
    }
}
impl<'a> Notification<'a> {
    /// Decode an exact object with bounded UTF-8 strings.
    /// # Errors
    /// Rejects malformed UTF-8/NUL, excessive strings, invalid enums/actions or absent timestamps.
    pub fn decode(bytes: &'a [u8], string_max: usize) -> Result<Self, ProtocolError> {
        if bytes.len() > 4096 {
            return Err(ProtocolError::Capacity);
        }
        let mut r = Reader::new(bytes);
        let notification = r.u32()?;
        let category = r.u16()?;
        let source_app = r.string(string_max)?;
        let sender = r.string(string_max)?;
        let number = r.string(string_max)?;
        let title = r.string(string_max)?;
        let body = r.string(string_max)?;
        let action_mask = r.u16()?;
        let created_utc_ms = r.i64()?;
        let time_valid = r.u8()?;
        if time_valid > 1 {
            return Err(ProtocolError::InvalidState);
        }
        r.finish()?;
        let value = Self {
            notification,
            category,
            source_app,
            sender,
            number,
            title,
            body,
            action_mask,
            created_utc_ms,
            created_time_valid: time_valid == 1,
        };
        value.validate()?;
        Ok(value)
    }
    /// Encode validated notification content; this never answers a phone call.
    /// # Errors
    /// Returns invalid fields, text limit or insufficient capacity.
    pub fn encode(&self, out: &mut [u8], string_max: usize) -> Result<usize, ProtocolError> {
        self.validate()?;
        let mut w = Writer::new(out);
        w.u32(self.notification)?;
        w.u16(self.category)?;
        for text in [
            self.source_app,
            self.sender,
            self.number,
            self.title,
            self.body,
        ] {
            w.string(text, string_max)?;
        }
        w.u16(self.action_mask)?;
        w.i64(self.created_utc_ms)?;
        w.u8(u8::from(self.created_time_valid))?;
        if w.len() > 4096 {
            return Err(ProtocolError::Capacity);
        }
        Ok(w.len())
    }
    /// Validate field relationships without authenticating the source phone.
    /// # Errors
    /// Rejects missing ID, unknown category/action bits or impossible missing-time representation.
    pub fn validate(self) -> Result<(), ProtocolError> {
        if self.notification == 0 {
            return Err(ProtocolError::InvalidId);
        }
        if self.category > 7
            || self.action_mask & !7 != 0
            || (self.category != 1 && self.action_mask != 0)
            || (!self.created_time_valid && self.created_utc_ms != 0)
        {
            return Err(ProtocolError::InvalidState);
        }
        Ok(())
    }
}
