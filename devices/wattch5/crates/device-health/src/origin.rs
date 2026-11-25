//! Source-aware schema2 for the three retained legacy result types.
use crate::{AcquisitionKind, Error, ResultSource};

/// A stored data source, independent of the command trigger and timestamp validity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ResultOrigin {
    /// Legacy BP/oxygen calculation using actual heart-rate inputs.
    Estimate = 1,
    /// A value supplied by an authorized App import endpoint.
    AppImport = 2,
    /// Legacy time-generated glucose; does not declare a physiological measurement.
    LegacyGenerated = 3,
}
impl ResultOrigin {
    /// Decode the exact dictionary; zero/unknown sources never acquire a default.
    /// # Errors
    /// Unknown values are rejected.
    pub const fn from_wire(value: u8) -> Result<Self, Error> {
        match value {
            1 => Ok(Self::Estimate),
            2 => Ok(Self::AppImport),
            3 => Ok(Self::LegacyGenerated),
            _ => Err(Error::InvalidField),
        }
    }
    /// The existing backend source dictionary remains unchanged.
    #[must_use]
    pub const fn source(self) -> ResultSource {
        match self {
            Self::AppImport => ResultSource::PhoneImport,
            Self::Estimate | Self::LegacyGenerated => ResultSource::DeviceAlgorithm,
        }
    }
    /// Actual acquisition class; never implicitly a sensor measurement.
    #[must_use]
    pub const fn acquisition(self) -> AcquisitionKind {
        match self {
            Self::Estimate => AcquisitionKind::DerivedEstimate,
            Self::AppImport => AcquisitionKind::PhoneImport,
            Self::LegacyGenerated => AcquisitionKind::LegacyGenerated,
        }
    }
}

/// Borrowed, validated schema2: unchanged schema1 value body followed by one origin byte.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SourcedResult<'a> {
    module: u8,
    value: &'a [u8],
    origin: ResultOrigin,
}
impl core::fmt::Debug for SourcedResult<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SourcedResult")
            .field("value_length", &self.value.len())
            .finish()
    }
}
impl<'a> SourcedResult<'a> {
    /// Validate a selected module, its unchanged value body, and the actual source.
    /// # Errors
    /// Invalid/disabled modules, wrong source combinations or malformed values fail.
    pub fn new(module: u8, value: &'a [u8], origin: ResultOrigin) -> Result<Self, Error> {
        let kind = match module {
            0x25 if origin != ResultOrigin::LegacyGenerated => 0x2501,
            0x26 if origin != ResultOrigin::LegacyGenerated => 0x2601,
            0x27 if origin != ResultOrigin::Estimate => 0x2701,
            _ => return Err(Error::InvalidField),
        };
        crate::result::validate_plain_result(
            crate::Module::from_wire(module)?,
            kind,
            1,
            value,
            origin.source(),
        )?;
        if module == 0x26 && origin == ResultOrigin::Estimate && value[4] & 2 != 0 {
            return Err(Error::InvalidField);
        }
        Ok(Self {
            module,
            value,
            origin,
        })
    }
    /// Decode exact schema2; bytes without an explicit valid source are never upgraded.
    /// # Errors
    /// Empty/truncated/trailing values and invalid sources fail.
    pub fn decode(module: u8, bytes: &'a [u8]) -> Result<Self, Error> {
        let (&last, value) = bytes.split_last().ok_or(Error::InvalidLength)?;
        Self::new(module, value, ResultOrigin::from_wire(last)?)
    }
    /// Unchanged schema1 physiological body and units.
    #[must_use]
    pub const fn value(self) -> &'a [u8] {
        self.value
    }
    /// Explicit immutable source stored with this value.
    #[must_use]
    pub const fn origin(self) -> ResultOrigin {
        self.origin
    }
    /// Encode with exactly one extra byte; short output is unchanged.
    /// # Errors
    /// Insufficient output capacity fails before writing.
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
        let n = self.value.len();
        if out.len() <= n {
            return Err(Error::Capacity);
        }
        out[..n].copy_from_slice(self.value);
        out[n] = self.origin as u8;
        Ok(n + 1)
    }
}
