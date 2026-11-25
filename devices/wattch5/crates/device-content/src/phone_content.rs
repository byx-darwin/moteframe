#[cfg(any(feature = "companion-radio", feature = "memo"))]
use crate::wire::ensure;
#[cfg(any(feature = "companion-radio", feature = "contacts", feature = "memo"))]
use crate::{
    Error, Manifest,
    wire::{Reader, Writer},
};
/// Existing strict forecast model; unknown writes are rejected by its versioned weather dictionary.
#[cfg(feature = "weather")]
pub use device_models::{Weather, WeatherDay};
/// Independent Classic Bluetooth readback, never inferred from BLE session state.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg(feature = "companion-radio")]
pub struct ClassicBluetooth {
    /// Desired enabled state.
    pub enabled: bool,
    /// Candidate 0 unknown, 1 disabled, 2 enabling, 3 idle, 4 connected, 5 disabling, 6 fault.
    pub state: u8,
    /// Address in display order, same as 02.P0007.
    pub address: [u8; 6],
    /// Address validity only, not validity of BLE or audio.
    pub valid: bool,
}
#[cfg(feature = "companion-radio")]
impl core::fmt::Debug for ClassicBluetooth {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ClassicBluetooth")
            .field("encoded_length", &9)
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "companion-radio")]
impl ClassicBluetooth {
    pub(crate) fn decode(reader: &mut Reader<'_>, manifest: &Manifest<'_>) -> Result<Self, Error> {
        if !manifest.independent_call_bluetooth {
            return Err(Error::Unavailable);
        }
        let value = Self {
            enabled: reader.boolean()?,
            state: reader.u8()?,
            address: reader.array()?,
            valid: reader.boolean()?,
        };
        ensure(value.state <= 6 && (value.valid || value.address == [0; 6]))?;
        Ok(value)
    }
    pub(crate) fn encode(
        self,
        writer: &mut Writer<'_>,
        manifest: &Manifest<'_>,
    ) -> Result<(), Error> {
        if !manifest.independent_call_bluetooth {
            return Err(Error::Unavailable);
        }
        ensure(self.state <= 6 && (self.valid || self.address == [0; 6]))?;
        writer.u8(u8::from(self.enabled))?;
        writer.u8(self.state)?;
        writer.bytes(&self.address)?;
        writer.u8(u8::from(self.valid))
    }
}
/// Frozen 34.O0001 contact. Storage/atomic COMMIT is supplied by the object service.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg(feature = "contacts")]
pub struct Contact<'a> {
    /// Display name.
    pub name: &'a str,
    /// Telephone number as UTF-8 text; never BCD or an automatic dial request.
    pub number: &'a str,
}
#[cfg(feature = "contacts")]
impl core::fmt::Debug for Contact<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Contact")
            .field("name_length", &self.name.len())
            .field("number_length", &self.number.len())
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "contacts")]
impl<'a> Contact<'a> {
    pub(crate) fn decode(bytes: &'a [u8], manifest: &Manifest<'_>) -> Result<Self, Error> {
        let max = manifest.string_max.ok_or(Error::Unavailable)?;
        let mut reader = Reader::new(bytes);
        let value = Self {
            name: reader.text(max)?,
            number: reader.text(max)?,
        };
        reader.end()?;
        Ok(value)
    }
    pub(crate) fn encode(self, out: &mut [u8], manifest: &Manifest<'_>) -> Result<usize, Error> {
        let max = manifest.string_max.ok_or(Error::Unavailable)?;
        let mut writer = Writer::new(out);
        writer.text(self.name, max)?;
        writer.text(self.number, max)?;
        Ok(writer.len())
    }
}
/// Frozen 36.O0001 memo; no undocumented content or validity field is appended.
#[derive(Clone, Copy, PartialEq, Eq)]
#[cfg(feature = "memo")]
pub struct Memo<'a> {
    /// Inclusive start UTC milliseconds.
    pub start_utc_ms: i64,
    /// End UTC milliseconds, at least the start.
    pub end_utc_ms: i64,
    /// Title bounded by actual product string capacity.
    pub title: &'a str,
}
#[cfg(feature = "memo")]
impl core::fmt::Debug for Memo<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Memo")
            .field("title_length", &self.title.len())
            .finish_non_exhaustive()
    }
}
#[cfg(feature = "memo")]
impl<'a> Memo<'a> {
    pub(crate) fn decode(bytes: &'a [u8], manifest: &Manifest<'_>) -> Result<Self, Error> {
        let mut reader = Reader::new(bytes);
        let value = Self {
            start_utc_ms: i64::from_le_bytes(reader.array()?),
            end_utc_ms: i64::from_le_bytes(reader.array()?),
            title: reader.text(manifest.string_max.ok_or(Error::Unavailable)?)?,
        };
        reader.end()?;
        value.validate(manifest)?;
        Ok(value)
    }
    fn validate(self, manifest: &Manifest<'_>) -> Result<(), Error> {
        let range = manifest.utc_ms.ok_or(Error::Unavailable)?;
        ensure(
            range.min <= range.max
                && self.start_utc_ms >= range.min
                && self.start_utc_ms <= self.end_utc_ms
                && self.end_utc_ms <= range.max,
        )
    }
    pub(crate) fn encode(self, out: &mut [u8], manifest: &Manifest<'_>) -> Result<usize, Error> {
        self.validate(manifest)?;
        let mut writer = Writer::new(out);
        writer.bytes(&self.start_utc_ms.to_le_bytes())?;
        writer.bytes(&self.end_utc_ms.to_le_bytes())?;
        writer.text(self.title, manifest.string_max.ok_or(Error::Unavailable)?)?;
        Ok(writer.len())
    }
}
