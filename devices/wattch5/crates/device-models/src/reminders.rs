//! Reminder object schemas. Scheduling, RTC and alert hardware are separate ports.
use crate::{
    TimeWindow,
    body::{Reader, Writer},
};
use device_protocol::ProtocolError;
/// Common schema for kinds1..6, validated against the selected reminder kind.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Reminder<'a> {
    /// Module05 object kind, not serialized as part of the body.
    pub kind: u16,
    /// Whether scheduling is requested. This is not a claim an alert was executed.
    pub enabled: bool,
    /// 0 single absolute instant, 1 weekly local minute, 2 periodic.
    pub repeat: u8,
    /// UTC milliseconds for a one-shot reminder.
    pub trigger_utc_ms: i64,
    /// Local minute of day for weekly repetition.
    pub minute: u16,
    /// Periodic interval, seconds.
    pub interval_s: u32,
    /// Work window; weekday bit0 is Sunday, cross-midnight belongs to start day.
    pub work: TimeWindow,
    /// Additional independent suppression window.
    pub independent_dnd: TimeWindow,
    /// Sedentary-only real-step threshold, zero for other kinds.
    pub step_threshold: u32,
    /// UTF-8 label, with a byte limit supplied by product policy.
    pub title: &'a str,
}
impl core::fmt::Debug for Reminder<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Reminder")
            .field("kind", &self.kind)
            .field("title_length", &self.title.len())
            .finish_non_exhaustive()
    }
}
impl<'a> Reminder<'a> {
    /// Decode a complete object with a product-specific maximum label size.
    /// # Errors
    /// Rejects invalid kind, enum/window combinations, noncanonical unused fields or text.
    pub fn decode(kind: u16, bytes: &'a [u8], string_max: usize) -> Result<Self, ProtocolError> {
        let mut r = Reader::new(bytes);
        let enabled = r.u8()?;
        if enabled > 1 {
            return Err(ProtocolError::InvalidState);
        }
        let repeat = r.u8()?;
        let trigger_utc_ms = r.i64()?;
        let minute = r.u16()?;
        let interval_s = r.u32()?;
        let work = TimeWindow::decode(r.take(6)?)?;
        let independent_dnd = TimeWindow::decode(r.take(6)?)?;
        let step_threshold = r.u32()?;
        let title = r.string(string_max)?;
        r.finish()?;
        let result = Self {
            kind,
            enabled: enabled == 1,
            repeat,
            trigger_utc_ms,
            minute,
            interval_s,
            work,
            independent_dnd,
            step_threshold,
            title,
        };
        result.validate()?;
        Ok(result)
    }
    /// Encode this reminder without executing a scheduled action.
    /// # Errors
    /// Rejects invalid fields, excessive text or insufficient output capacity.
    pub fn encode(&self, out: &mut [u8], string_max: usize) -> Result<usize, ProtocolError> {
        self.validate()?;
        let mut w = Writer::new(out);
        w.u8(u8::from(self.enabled))?;
        w.u8(self.repeat)?;
        w.i64(self.trigger_utc_ms)?;
        w.u16(self.minute)?;
        w.u32(self.interval_s)?;
        let mut window = [0; 6];
        self.work.encode(&mut window)?;
        w.put(&window)?;
        self.independent_dnd.encode(&mut window)?;
        w.put(&window)?;
        w.u32(self.step_threshold)?;
        w.string(self.title, string_max)?;
        Ok(w.len())
    }
    /// Check kind-specific repeat and canonical field rules.
    /// # Errors
    /// Returns invalid state for incompatible fields or unsupported kind.
    pub fn validate(self) -> Result<(), ProtocolError> {
        if !(1..=6).contains(&self.kind) || self.repeat > 2 || self.minute >= 1440 {
            return Err(ProtocolError::InvalidState);
        }
        self.work.validate()?;
        self.independent_dnd.validate()?;
        if (self.kind <= 4 && self.repeat == 2)
            || (self.kind >= 5 && self.repeat != 2)
            || (self.kind != 6 && self.step_threshold != 0)
        {
            return Err(ProtocolError::InvalidState);
        }
        match self.repeat {
            0 => {
                if self.minute != 0
                    || self.interval_s != 0
                    || self.work.enabled != 0
                    || self.work.weekday_mask != 0
                    || self.work.start_min != 0
                    || self.work.end_min != 0
                {
                    return Err(ProtocolError::InvalidState);
                }
            }
            1 => {
                if self.trigger_utc_ms != 0
                    || self.interval_s != 0
                    || self.work.enabled != 1
                    || self.work.weekday_mask == 0
                {
                    return Err(ProtocolError::InvalidState);
                }
            }
            2 => {
                if self.trigger_utc_ms != 0
                    || self.minute != 0
                    || self.interval_s == 0
                    || self.work.enabled != 1
                    || self.work.weekday_mask == 0
                {
                    return Err(ProtocolError::InvalidState);
                }
            }
            _ => return Err(ProtocolError::InvalidState),
        }
        Ok(())
    }
}
