//! Concrete physiological body codecs. Field presence is explicit; no value is synthesized.
use crate::result::sealed::Sealed;
use crate::{Error, ResultBody};
macro_rules! private_debug {
    ($ty:ty, $label:literal) => {
        impl core::fmt::Debug for $ty {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.debug_struct($label).finish_non_exhaustive()
            }
        }
    };
}
#[cfg(any(
    feature = "heart-rate",
    feature = "blood-pressure",
    feature = "oxygen",
    feature = "glucose",
    feature = "ecg"
))]
macro_rules! identity {
    ($ty:ty, $module:expr, $record:expr, $schema:expr, $present:expr) => {
        impl Sealed for $ty {}
        impl ResultBody for $ty {
            fn module(&self) -> u8 {
                $module
            }
            fn record_type(&self) -> u16 {
                $record
            }
            fn schema(&self) -> u16 {
                $schema
            }
            fn validate(&self) -> Result<(), Error> {
                Self::validate(self)
            }
            fn has_value(&self) -> bool {
                $present(self)
            }
        }
    };
}
#[cfg(any(
    feature = "heart-rate",
    feature = "blood-pressure",
    feature = "oxygen",
    feature = "glucose",
    feature = "temperature",
    feature = "ecg",
    feature = "wellbeing"
))]
fn exact(bytes: &[u8], length: usize) -> Result<(), Error> {
    if bytes.len() == length {
        Ok(())
    } else {
        Err(Error::InvalidLength)
    }
}
#[cfg(any(
    feature = "heart-rate",
    feature = "blood-pressure",
    feature = "oxygen",
    feature = "ecg"
))]
fn optional_u16(bytes: &[u8], valid: bool) -> Result<Option<u16>, Error> {
    let value = u16::from_le_bytes([bytes[0], bytes[1]]);
    if valid {
        Ok(Some(value))
    } else if value == 0 {
        Ok(None)
    } else {
        Err(Error::InvalidField)
    }
}
#[cfg(any(
    feature = "heart-rate",
    feature = "blood-pressure",
    feature = "oxygen",
    feature = "ecg"
))]
fn put_u16(value: Option<u16>, output: &mut [u8]) {
    output[..2].copy_from_slice(&value.unwrap_or(0).to_le_bytes());
}
#[cfg(any(
    feature = "glucose",
    feature = "temperature",
    feature = "wellbeing",
    feature = "microcirculation"
))]
fn boolean(value: u8) -> Result<bool, Error> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(Error::InvalidField),
    }
}
#[cfg(any(feature = "wellbeing", feature = "microcirculation"))]
fn source(value: u8) -> Result<Option<crate::ResultSource>, Error> {
    if value == 0 {
        Ok(None)
    } else {
        crate::ResultSource::from_wire(value).map(Some)
    }
}

/// Frozen0020/schema1: bpm:u16,quality:u8,flags:u8. Debug hides all physiological values.
#[cfg(feature = "heart-rate")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct HeartRate {
    /// Present only when flagsbit0 is set; other flag bits are reserved.
    pub bpm: Option<u16>,
    ///0..100 or255unknown. This is quality metadata,not an accuracy guarantee.
    pub quality: u8,
}
#[cfg(feature = "heart-rate")]
private_debug!(HeartRate, "HeartRate");
#[cfg(feature = "heart-rate")]
identity!(HeartRate, 0x20, 0x0020, 1, |body: &HeartRate| body
    .bpm
    .is_some());
#[cfg(feature = "heart-rate")]
impl HeartRate {
    /// Exact body length, with no provenance byte appended.
    pub const LENGTH: usize = 4;
    /// Decode exact frozen body, rejecting reservedflags and nonzero missing values.
    /// # Errors
    /// Invalid length,flags,quality or canonical absence is rejected.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        exact(bytes, 4)?;
        if bytes[3] & !1 != 0 {
            return Err(Error::InvalidField);
        }
        let value = Self {
            bpm: optional_u16(&bytes[..2], bytes[3] & 1 != 0)?,
            quality: bytes[2],
        };
        value.validate()?;
        Ok(value)
    }
    /// Validate the frozen quality dictionary; no additional clinical range is invented.
    /// # Errors
    /// Quality101..254 is invalid.
    pub fn validate(&self) -> Result<(), Error> {
        if self.quality > 100 && self.quality != 255 {
            Err(Error::InvalidField)
        } else {
            Ok(())
        }
    }
    /// Encode exact4B, deriving the validbit from explicit field presence.
    /// # Errors
    /// Invalid quality or insufficient caller storage is rejected before writing.
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
        self.validate()?;
        if out.len() < 4 {
            return Err(Error::Capacity);
        }
        put_u16(self.bpm, out);
        out[2] = self.quality;
        out[3] = u8::from(self.bpm.is_some());
        Ok(4)
    }
}
/// Proposed2501/schema1,mmHg/bpm,maskbits0systolic/1diastolic/2pulse.
#[cfg(feature = "blood-pressure")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct BloodPressure {
    /// Actual systolic value if present.
    pub systolic: Option<u16>,
    /// Actual diastolic value if present.
    pub diastolic: Option<u16>,
    /// Actual pulse if present.
    pub pulse: Option<u16>,
}
#[cfg(feature = "blood-pressure")]
private_debug!(BloodPressure, "BloodPressure");
#[cfg(feature = "blood-pressure")]
identity!(BloodPressure, 0x25, 0x2501, 1, |b: &BloodPressure| b
    .systolic
    .is_some()
    || b.diastolic.is_some()
    || b.pulse.is_some());
#[cfg(feature = "blood-pressure")]
impl BloodPressure {
    /// Exact result body size.
    pub const LENGTH: usize = 8;
    /// Decode exact8B with independent field validity.
    /// # Errors
    /// Bad length,reservedmask or nonzero absent field is rejected.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        exact(bytes, 8)?;
        let mask = u16::from_le_bytes([bytes[6], bytes[7]]);
        if mask & !7 != 0 {
            return Err(Error::InvalidField);
        }
        Ok(Self {
            systolic: optional_u16(&bytes[..2], mask & 1 != 0)?,
            diastolic: optional_u16(&bytes[2..4], mask & 2 != 0)?,
            pulse: optional_u16(&bytes[4..6], mask & 4 != 0)?,
        })
    }
    /// The proposal defines u16 storage and independent validity,not clinical pass thresholds.
    /// # Errors
    /// No additional clinical rejection is invented beyond the typed representation.
    pub const fn validate(&self) -> Result<(), Error> {
        Ok(())
    }
    /// Encode exact8B; missing fields become canonical0 with clearbits.
    /// # Errors
    /// Short output is rejected before writing.
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
        if out.len() < 8 {
            return Err(Error::Capacity);
        }
        put_u16(self.systolic, out);
        put_u16(self.diastolic, &mut out[2..]);
        put_u16(self.pulse, &mut out[4..]);
        let mask = u16::from(self.systolic.is_some())
            | u16::from(self.diastolic.is_some()) << 1
            | u16::from(self.pulse.is_some()) << 2;
        out[6..8].copy_from_slice(&mask.to_le_bytes());
        Ok(8)
    }
}
/// Proposed2601/schema1,SpO2 andPI in percent*100,maskbits0/1.
#[cfg(feature = "oxygen")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Oxygen {
    /// Actual oxygen saturation percent*100.
    pub spo2_x100: Option<u16>,
    /// Actual perfusion index percent*100; never inferred from `SpO2`.
    pub pi_x100: Option<u16>,
}
#[cfg(feature = "oxygen")]
private_debug!(Oxygen, "Oxygen");
#[cfg(feature = "oxygen")]
identity!(Oxygen, 0x26, 0x2601, 1, |b: &Oxygen| b.spo2_x100.is_some()
    || b.pi_x100.is_some());
#[cfg(feature = "oxygen")]
impl Oxygen {
    /// Exact result body size.
    pub const LENGTH: usize = 6;
    /// Decode exact6B with canonical absence and reserved mask rejection.
    /// # Errors
    /// Invalid length,mask,absence or values above10000 fail.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        exact(bytes, 6)?;
        let mask = u16::from_le_bytes([bytes[4], bytes[5]]);
        if mask & !3 != 0 {
            return Err(Error::InvalidField);
        }
        let value = Self {
            spo2_x100: optional_u16(&bytes[..2], mask & 1 != 0)?,
            pi_x100: optional_u16(&bytes[2..4], mask & 2 != 0)?,
        };
        value.validate()?;
        Ok(value)
    }
    /// Validate the explicitly proposed percent range0..10000.
    /// # Errors
    /// Present values above10000 are invalid.
    pub fn validate(&self) -> Result<(), Error> {
        if self.spo2_x100.is_some_and(|v| v > 10000) || self.pi_x100.is_some_and(|v| v > 10000) {
            Err(Error::InvalidField)
        } else {
            Ok(())
        }
    }
    /// Encode exact6B without inventing a missingPI.
    /// # Errors
    /// Bad range/short output fails before writing.
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
        self.validate()?;
        if out.len() < 6 {
            return Err(Error::Capacity);
        }
        put_u16(self.spo2_x100, out);
        put_u16(self.pi_x100, &mut out[2..]);
        let mask = u16::from(self.spo2_x100.is_some()) | u16::from(self.pi_x100.is_some()) << 1;
        out[4..6].copy_from_slice(&mask.to_le_bytes());
        Ok(6)
    }
}
/// Proposed2701/schema1. Phone import and sensor claims are separated by provenance.
#[cfg(feature = "glucose")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Glucose {
    /// Actual provided value in mmol/L*100,or explicit absent result.
    pub mmol_l_x100: Option<u16>,
}
#[cfg(feature = "glucose")]
private_debug!(Glucose, "Glucose");
#[cfg(feature = "glucose")]
identity!(Glucose, 0x27, 0x2701, 1, |b: &Glucose| b
    .mmol_l_x100
    .is_some());
#[cfg(feature = "glucose")]
impl Glucose {
    /// Exact result body size.
    pub const LENGTH: usize = 3;
    /// Decode exact3B,not a meal reference or generated default.
    /// # Errors
    /// Invalid length/boolean or nonzero absent value is rejected.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        exact(bytes, 3)?;
        let value = u16::from_le_bytes([bytes[0], bytes[1]]);
        let valid = boolean(bytes[2])?;
        if !valid && value != 0 {
            return Err(Error::InvalidField);
        }
        Ok(Self {
            mmol_l_x100: if valid { Some(value) } else { None },
        })
    }
    /// u16 is the proposed encoding bound; clinical range belongs to a real product backend.
    /// # Errors
    /// No additional clinical rejection is invented by this codec.
    pub const fn validate(&self) -> Result<(), Error> {
        Ok(())
    }
    /// Encode exact3B with canonical absent0.
    /// # Errors
    /// Short output is rejected before writing.
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
        if out.len() < 3 {
            return Err(Error::Capacity);
        }
        out[..2].copy_from_slice(&self.mmol_l_x100.unwrap_or(0).to_le_bytes());
        out[2] = u8::from(self.mmol_l_x100.is_some());
        Ok(3)
    }
}
/// Proposed2801/schema1,four-byte signed temperature. No provenance is appended.
#[cfg(feature = "temperature")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Temperature {
    /// Signed Celsius*100,or explicit failed/missing value.
    pub centi_c: Option<i16>,
    /// Explicit physical measurement mode.
    pub mode: crate::TemperatureMode,
}
#[cfg(feature = "temperature")]
private_debug!(Temperature, "Temperature");
#[cfg(feature = "temperature")]
impl Sealed for Temperature {}
#[cfg(feature = "temperature")]
impl ResultBody for Temperature {
    fn module(&self) -> u8 {
        0x28
    }
    fn record_type(&self) -> u16 {
        0x2801
    }
    fn schema(&self) -> u16 {
        1
    }
    fn validate(&self) -> Result<(), Error> {
        Self::validate(self)
    }
    fn has_value(&self) -> bool {
        self.centi_c.is_some()
    }
    fn temperature_mode(&self) -> Option<crate::TemperatureMode> {
        Some(self.mode)
    }
}
#[cfg(feature = "temperature")]
impl Temperature {
    /// Exact result body size.
    pub const LENGTH: usize = 4;
    /// Decode exact4B; mode1..4 remains explicit even for a failed sample.
    /// # Errors
    /// Invalid length,mode,validity or noncanonical absent value is rejected.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        exact(bytes, 4)?;
        let mode = crate::TemperatureMode::from_wire(bytes[2])?;
        let value = i16::from_le_bytes([bytes[0], bytes[1]]);
        let valid = boolean(bytes[3])?;
        if !valid && value != 0 {
            return Err(Error::InvalidField);
        }
        Ok(Self {
            centi_c: if valid { Some(value) } else { None },
            mode,
        })
    }
    /// The type enforces signed storage and a known mode. Actual mode support is product-declared.
    /// # Errors
    /// No new clinical range beyond i16 is invented.
    pub const fn validate(&self) -> Result<(), Error> {
        Ok(())
    }
    /// Encode exact4B,retaining negative temperatures and explicit absent0.
    /// # Errors
    /// Short output is rejected before writing.
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
        if out.len() < 4 {
            return Err(Error::Capacity);
        }
        out[..2].copy_from_slice(&self.centi_c.unwrap_or(0).to_le_bytes());
        out[2] = self.mode as u8;
        out[3] = u8::from(self.centi_c.is_some());
        Ok(4)
    }
}
/// Proposed2901/schema2. Statistics refer to one real ECG measurement window.
#[cfg(feature = "ecg")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Ecg {
    /// Nonzero stable measurement identity,not a user slot or payload index.
    pub measurement: u64,
    ///0current owner;1..254product slots;255invalid.
    pub user_slot: u8,
    /// Current ECG heart rate,independent of PPG.
    pub ecg_bpm: Option<u16>,
    /// Current PPG heart rate; never substituted for absent ECG.
    pub ppg_bpm: Option<u16>,
    /// Average valid ECG rate over the actual measurement window.
    pub avg_bpm: Option<u16>,
    /// Minimum valid ECG rate over that same window.
    pub min_bpm: Option<u16>,
    /// Maximum valid ECG rate over that same window.
    pub max_bpm: Option<u16>,
}
#[cfg(feature = "ecg")]
private_debug!(Ecg, "Ecg");
#[cfg(feature = "ecg")]
identity!(Ecg, 0x29, 0x2901, 2, |b: &Ecg| b.ecg_bpm.is_some()
    || b.ppg_bpm.is_some()
    || b.avg_bpm.is_some()
    || b.min_bpm.is_some()
    || b.max_bpm.is_some());
#[cfg(feature = "ecg")]
impl Ecg {
    /// Exact body size,without a LATEST envelope.
    pub const LENGTH: usize = 21;
    /// Decode independent maskbits0ECG/1PPG/2average/3minimum/4maximum.
    /// # Errors
    /// Invalid identity,slot,mask,canonical absence,statistical ordering or exact length fails.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        exact(bytes, 21)?;
        let mut id = [0; 8];
        id.copy_from_slice(&bytes[..8]);
        let mask = u16::from_le_bytes([bytes[19], bytes[20]]);
        if mask & !31 != 0 {
            return Err(Error::InvalidField);
        }
        let value = Self {
            measurement: u64::from_le_bytes(id),
            user_slot: bytes[8],
            ecg_bpm: optional_u16(&bytes[9..11], mask & 1 != 0)?,
            ppg_bpm: optional_u16(&bytes[11..13], mask & 2 != 0)?,
            avg_bpm: optional_u16(&bytes[13..15], mask & 4 != 0)?,
            min_bpm: optional_u16(&bytes[15..17], mask & 8 != 0)?,
            max_bpm: optional_u16(&bytes[17..19], mask & 16 != 0)?,
        };
        value.validate()?;
        Ok(value)
    }
    /// Validate structural identity and internally consistent window statistics,not a clinical range.
    /// # Errors
    /// Zero measurement,slot255 or minimum>average>maximum inconsistency fails.
    pub fn validate(&self) -> Result<(), Error> {
        if self.measurement == 0
            || self.user_slot == 255
            || self
                .min_bpm
                .zip(self.avg_bpm)
                .is_some_and(|(min, avg)| min > avg)
            || self
                .avg_bpm
                .zip(self.max_bpm)
                .is_some_and(|(avg, max)| avg > max)
            || self
                .min_bpm
                .zip(self.max_bpm)
                .is_some_and(|(min, max)| min > max)
        {
            Err(Error::InvalidField)
        } else {
            Ok(())
        }
    }
    /// Encode exact21B with canonical clear fields and reservedmask0.
    /// # Errors
    /// Invalid structure/statistics or short output fails before writing.
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
        self.validate()?;
        if out.len() < 21 {
            return Err(Error::Capacity);
        }
        out[..8].copy_from_slice(&self.measurement.to_le_bytes());
        out[8] = self.user_slot;
        let mut mask = 0u16;
        for (index, value) in [
            self.ecg_bpm,
            self.ppg_bpm,
            self.avg_bpm,
            self.min_bpm,
            self.max_bpm,
        ]
        .into_iter()
        .enumerate()
        {
            let offset = 9 + index * 2;
            put_u16(value, &mut out[offset..]);
            if value.is_some() {
                mask |= 1 << index;
            }
        }
        out[19..21].copy_from_slice(&mask.to_le_bytes());
        Ok(21)
    }
}
/// One of five explicitly identified independent wellbeing results.
#[cfg(feature = "wellbeing")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WellbeingKind {
    ///2A01,five fatigue grades0..4,unit2,scale0.
    Fatigue,
    ///2A02,stress score0..100,unit1,scale0.
    Stress,
    ///2A03,product-declared signed mood enum,unit3,scale0.
    Mood,
    ///2A04,nonnegative RMSSD milliseconds,unit4,scale-3..0.
    Rmssd,
    ///2A05,nonnegative MET multiple,unit5,scale-2.
    Met,
}
#[cfg(feature = "wellbeing")]
impl WellbeingKind {
    /// Resolve exact independent record type,not a mixed result bundle.
    /// # Errors
    /// Unknown type is rejected.
    pub const fn from_record_type(value: u16) -> Result<Self, Error> {
        match value {
            0x2a01 => Ok(Self::Fatigue),
            0x2a02 => Ok(Self::Stress),
            0x2a03 => Ok(Self::Mood),
            0x2a04 => Ok(Self::Rmssd),
            0x2a05 => Ok(Self::Met),
            _ => Err(Error::InvalidField),
        }
    }
    /// Exact stable proposed record identity.
    #[must_use]
    pub const fn record_type(self) -> u16 {
        match self {
            Self::Fatigue => 0x2a01,
            Self::Stress => 0x2a02,
            Self::Mood => 0x2a03,
            Self::Rmssd => 0x2a04,
            Self::Met => 0x2a05,
        }
    }
    /// Required unit dictionary value; cannot be reinterpreted by changing payload length.
    #[must_use]
    pub const fn unit(self) -> u8 {
        match self {
            Self::Fatigue => 2,
            Self::Stress => 1,
            Self::Mood => 3,
            Self::Rmssd => 4,
            Self::Met => 5,
        }
    }
}
/// Proposed2A01..05/schema1. Physical `value=value*10^scale_exp` in the kind's required unit.
#[cfg(feature = "wellbeing")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Wellbeing {
    /// Exact independent metric,not inferred from source or scale.
    pub kind: WellbeingKind,
    /// Explicit result value,or canonical absence.
    pub value: Option<i32>,
    /// Decimal exponent,restricted by the selected metric.
    pub scale_exp: i8,
    /// Actual device algorithm/phone import; unknown allowed only for absent values.
    pub source: Option<crate::ResultSource>,
}
#[cfg(feature = "wellbeing")]
private_debug!(Wellbeing, "Wellbeing");
#[cfg(feature = "wellbeing")]
impl Sealed for Wellbeing {}
#[cfg(feature = "wellbeing")]
impl ResultBody for Wellbeing {
    fn module(&self) -> u8 {
        0x2a
    }
    fn record_type(&self) -> u16 {
        self.kind.record_type()
    }
    fn schema(&self) -> u16 {
        1
    }
    fn validate(&self) -> Result<(), Error> {
        Self::validate(self)
    }
    fn has_value(&self) -> bool {
        self.value.is_some()
    }
    fn embedded_source(&self) -> Option<crate::ResultSource> {
        self.source
    }
    fn mood_value(&self) -> Option<i32> {
        if self.kind == WellbeingKind::Mood {
            self.value
        } else {
            None
        }
    }
}
#[cfg(feature = "wellbeing")]
impl Wellbeing {
    /// Exact body size for every independent wellbeing type.
    pub const LENGTH: usize = 8;
    /// Decode exact8B and required type/unit/source/scale/range. Mood's actual value-label enum is
    /// intentionally validated by `ProductDeclaration`,not an invented universal dictionary.
    /// # Errors
    /// Invalid length,unit,source,validity,range,scale or noncanonical absence is rejected.
    pub fn decode(record_type: u16, bytes: &[u8]) -> Result<Self, Error> {
        exact(bytes, 8)?;
        let kind = WellbeingKind::from_record_type(record_type)?;
        if bytes[5] != kind.unit() {
            return Err(Error::InvalidField);
        }
        let mut raw = [0; 4];
        raw.copy_from_slice(&bytes[..4]);
        let value = i32::from_le_bytes(raw);
        let valid = boolean(bytes[7])?;
        if !valid && value != 0 {
            return Err(Error::InvalidField);
        }
        let result = Self {
            kind,
            value: if valid { Some(value) } else { None },
            scale_exp: i8::from_le_bytes([bytes[4]]),
            source: source(bytes[6])?,
        };
        result.validate()?;
        Ok(result)
    }
    /// Validate only the explicitly proposed metric encoding; mood labels require product context.
    /// # Errors
    /// Rejects missing source for present data,inconsistent scale or declared score/domain bounds.
    pub fn validate(&self) -> Result<(), Error> {
        if self.value.is_some() && self.source.is_none() {
            return Err(Error::InvalidField);
        }
        let scale_valid = match self.kind {
            WellbeingKind::Fatigue | WellbeingKind::Stress | WellbeingKind::Mood => {
                self.scale_exp == 0
            }
            WellbeingKind::Rmssd => (-3..=0).contains(&self.scale_exp),
            WellbeingKind::Met => self.scale_exp == -2,
        };
        let value_valid = self.value.is_none_or(|value| match self.kind {
            WellbeingKind::Fatigue => (0..=4).contains(&value),
            WellbeingKind::Stress => (0..=100).contains(&value),
            WellbeingKind::Mood => true,
            WellbeingKind::Rmssd | WellbeingKind::Met => value >= 0,
        });
        if scale_valid && value_valid {
            Ok(())
        } else {
            Err(Error::InvalidField)
        }
    }
    /// Encode exact8B with the kind's required unit. Source is already part of these pending schemas.
    /// # Errors
    /// Invalid fields or short output are rejected before writing.
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
        self.validate()?;
        if out.len() < 8 {
            return Err(Error::Capacity);
        }
        out[..4].copy_from_slice(&self.value.unwrap_or(0).to_le_bytes());
        out[4] = self.scale_exp.to_le_bytes()[0];
        out[5] = self.kind.unit();
        out[6] = self.source.map_or(0, |value| value as u8);
        out[7] = u8::from(self.value.is_some());
        Ok(8)
    }
}
/// Why a fatigue snapshot was produced, independent of algorithm/import provenance and E81 jobs.
#[cfg(feature = "wellbeing")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FatigueTrigger {
    /// No trustworthy trigger observation.
    Unknown = 0,
    /// App request.
    App = 1,
    /// Device UI or key.
    DeviceUser = 2,
    /// Scheduled policy.
    ScheduledPolicy = 3,
    /// Active sport context.
    SportContext = 4,
    /// Connection synchronization.
    ConnectionSync = 5,
}
#[cfg(feature = "wellbeing")]
impl FatigueTrigger {
    /// Decode only this snapshot's trigger dictionary; E81 keeps its original four triggers.
    /// # Errors
    /// Reserved values are invalid, never reinterpreted as an algorithm source.
    pub const fn from_wire(value: u8) -> Result<Self, Error> {
        match value {
            0 => Ok(Self::Unknown),
            1 => Ok(Self::App),
            2 => Ok(Self::DeviceUser),
            3 => Ok(Self::ScheduledPolicy),
            4 => Ok(Self::SportContext),
            5 => Ok(Self::ConnectionSync),
            _ => Err(Error::InvalidField),
        }
    }
}
/// Fatigue2A01/schema2: unchanged8B fatigue body plus independent actual resting BPM and trigger.
#[cfg(feature = "wellbeing")]
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct FatigueSnapshot {
    /// Original schema1 fatigue body; its source applies to the entire snapshot.
    pub fatigue: Wellbeing,
    /// Actual positive resting BPM from the same provider, independently valid from fatigue.
    pub resting_bpm: Option<u16>,
    /// Actual observation trigger, not algorithm or phone provenance.
    pub trigger: FatigueTrigger,
}
#[cfg(feature = "wellbeing")]
private_debug!(FatigueSnapshot, "FatigueSnapshot");
#[cfg(feature = "wellbeing")]
impl Sealed for FatigueSnapshot {}
#[cfg(feature = "wellbeing")]
impl ResultBody for FatigueSnapshot {
    fn module(&self) -> u8 {
        0x2a
    }
    fn record_type(&self) -> u16 {
        0x2a01
    }
    fn schema(&self) -> u16 {
        2
    }
    fn validate(&self) -> Result<(), Error> {
        Self::validate(self)
    }
    fn has_value(&self) -> bool {
        self.fatigue.value.is_some() || self.resting_bpm.is_some()
    }
    fn embedded_source(&self) -> Option<crate::ResultSource> {
        self.fatigue.source
    }
}
#[cfg(feature = "wellbeing")]
impl FatigueSnapshot {
    /// Exact schema2 body size, without an enclosing LATEST or history header.
    pub const LENGTH: usize = 12;
    /// Decode exact12B, preserving independent fatigue/resting validity and explicit trigger.
    /// # Errors
    /// Rejects malformed base body, invalid validity/source/trigger, zero valid BPM or nonzero absent BPM.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        exact(bytes, Self::LENGTH)?;
        let fatigue = Wellbeing::decode(0x2a01, &bytes[..8])?;
        let bpm = u16::from_le_bytes([bytes[8], bytes[9]]);
        let valid = boolean(bytes[10])?;
        if !valid && bpm != 0 {
            return Err(Error::InvalidField);
        }
        let value = Self {
            fatigue,
            resting_bpm: if valid { Some(bpm) } else { None },
            trigger: FatigueTrigger::from_wire(bytes[11])?,
        };
        value.validate()?;
        Ok(value)
    }
    /// Validate the complete snapshot without deriving resting BPM or inferring a trigger.
    /// # Errors
    /// Non-fatigue base, present resting BPM0 or unknown provenance for present resting data fails.
    pub fn validate(&self) -> Result<(), Error> {
        self.fatigue.validate()?;
        if self.fatigue.kind != WellbeingKind::Fatigue
            || self.resting_bpm == Some(0)
            || (self.resting_bpm.is_some() && self.fatigue.source.is_none())
        {
            return Err(Error::InvalidField);
        }
        Ok(())
    }
    /// Encode exact12B; independently absent fields stay canonical zero, never invented results.
    /// # Errors
    /// Invalid complete data or insufficient output fails before writing.
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
        self.validate()?;
        if out.len() < Self::LENGTH {
            return Err(Error::Capacity);
        }
        self.fatigue.encode(&mut out[..8])?;
        out[8..10].copy_from_slice(&self.resting_bpm.unwrap_or(0).to_le_bytes());
        out[10] = u8::from(self.resting_bpm.is_some());
        out[11] = self.trigger as u8;
        Ok(Self::LENGTH)
    }
}
/// Proposed2B01/schema2 with actual algorithm metadata,not a synthesized microcirculation algorithm.
#[cfg(feature = "microcirculation")]
#[derive(Clone, Copy, PartialEq)]
pub struct Alk<'a> {
    /// Finite strictly positive ALK in1/s,or canonical absence. No division/formula is evaluated here.
    pub per_s: Option<f32>,
    /// Only a real device algorithm may claim a present ALK result.
    pub source: Option<crate::ResultSource>,
    /// Actual selected algorithm ID,UTF-8 withoutNUL.
    pub algorithm_id: &'a str,
    /// Actual selected algorithm version,UTF-8 withoutNUL.
    pub algorithm_version: &'a str,
}
#[cfg(feature = "microcirculation")]
private_debug!(Alk<'_>, "Alk");
#[cfg(feature = "microcirculation")]
impl Sealed for Alk<'_> {}
#[cfg(feature = "microcirculation")]
impl ResultBody for Alk<'_> {
    fn module(&self) -> u8 {
        0x2b
    }
    fn record_type(&self) -> u16 {
        0x2b01
    }
    fn schema(&self) -> u16 {
        2
    }
    fn validate(&self) -> Result<(), Error> {
        Self::validate(self)
    }
    fn has_value(&self) -> bool {
        self.per_s.is_some()
    }
    fn embedded_source(&self) -> Option<crate::ResultSource> {
        self.source
    }
    fn embedded_algorithm(&self) -> Option<(&str, &str)> {
        Some((self.algorithm_id, self.algorithm_version))
    }
}
#[cfg(feature = "microcirculation")]
impl<'a> Alk<'a> {
    /// Fixed fields plus two emptyStr lengths.
    pub const MIN_LENGTH: usize = 10;
    /// Largest body fitting the existing512B LATEST envelope. Caller also checks response overhead.
    pub const MAX_LENGTH: usize = 497;
    /// Decode exact variable body with finite float,canonical absence,source and strict UTF-8Str.
    /// # Errors
    /// Invalid float/source/string/length/trailing bytes are rejected.
    pub fn decode(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() < Self::MIN_LENGTH || bytes.len() > Self::MAX_LENGTH {
            return Err(Error::InvalidLength);
        }
        let mut raw = [0; 4];
        raw.copy_from_slice(&bytes[..4]);
        let value = f32::from_le_bytes(raw);
        let valid = boolean(bytes[4])?;
        if !value.is_finite() || (!valid && raw != [0; 4]) {
            return Err(Error::InvalidField);
        }
        let mut offset = 6;
        let algorithm_id = take_str(bytes, &mut offset)?;
        let algorithm_version = take_str(bytes, &mut offset)?;
        if offset != bytes.len() {
            return Err(Error::InvalidLength);
        }
        let result = Self {
            per_s: if valid { Some(value) } else { None },
            source: source(bytes[5])?,
            algorithm_id,
            algorithm_version,
        };
        result.validate()?;
        Ok(result)
    }
    /// Validate finite positive ALK and actual algorithm metadata; does not claim algorithm accuracy.
    /// # Errors
    /// Present data needs device source and nonempty real metadata; all strings disallowNUL.
    pub fn validate(&self) -> Result<(), Error> {
        if self.algorithm_id.as_bytes().contains(&0)
            || self.algorithm_version.as_bytes().contains(&0)
        {
            return Err(Error::InvalidField);
        }
        if let Some(value) = self.per_s
            && (!value.is_finite()
                || value <= 0.0
                || self.source != Some(crate::ResultSource::DeviceAlgorithm)
                || !crate::result::valid_identifier(self.algorithm_id)
                || !crate::result::valid_identifier(self.algorithm_version))
        {
            return Err(Error::InvalidField);
        }
        self.encoded_len()?;
        Ok(())
    }
    /// Checked variable size; no truncation of UTF-8 or u16 lengths.
    /// # Errors
    /// Excessive string/body length is rejected.
    pub fn encoded_len(&self) -> Result<usize, Error> {
        if u16::try_from(self.algorithm_id.len()).is_err()
            || u16::try_from(self.algorithm_version.len()).is_err()
        {
            return Err(Error::InvalidLength);
        }
        let length = Self::MIN_LENGTH
            .checked_add(self.algorithm_id.len())
            .and_then(|n| n.checked_add(self.algorithm_version.len()))
            .ok_or(Error::InvalidLength)?;
        if length > Self::MAX_LENGTH {
            Err(Error::InvalidLength)
        } else {
            Ok(length)
        }
    }
    /// Encode variable body only after full validation and output capacity check.
    /// # Errors
    /// Invalid fields/length or short caller storage is rejected before any write.
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
        self.validate()?;
        let length = self.encoded_len()?;
        if out.len() < length {
            return Err(Error::Capacity);
        }
        let id_len = u16::try_from(self.algorithm_id.len()).map_err(|_| Error::InvalidLength)?;
        let version_len =
            u16::try_from(self.algorithm_version.len()).map_err(|_| Error::InvalidLength)?;
        out[..4].copy_from_slice(&self.per_s.unwrap_or(0.0).to_le_bytes());
        out[4] = u8::from(self.per_s.is_some());
        out[5] = self.source.map_or(0, |value| value as u8);
        out[6..8].copy_from_slice(&id_len.to_le_bytes());
        let version_offset = 8 + self.algorithm_id.len();
        out[8..version_offset].copy_from_slice(self.algorithm_id.as_bytes());
        out[version_offset..version_offset + 2].copy_from_slice(&version_len.to_le_bytes());
        out[version_offset + 2..length].copy_from_slice(self.algorithm_version.as_bytes());
        Ok(length)
    }
}
#[cfg(feature = "microcirculation")]
fn take_str<'a>(bytes: &'a [u8], offset: &mut usize) -> Result<&'a str, Error> {
    let prefix_end = offset.checked_add(2).ok_or(Error::InvalidLength)?;
    let prefix = bytes.get(*offset..prefix_end).ok_or(Error::InvalidLength)?;
    let count = usize::from(u16::from_le_bytes([prefix[0], prefix[1]]));
    let end = prefix_end.checked_add(count).ok_or(Error::InvalidLength)?;
    let raw = bytes.get(prefix_end..end).ok_or(Error::InvalidLength)?;
    let result = core::str::from_utf8(raw).map_err(|_| Error::InvalidField)?;
    if raw.contains(&0) {
        return Err(Error::InvalidField);
    }
    *offset = end;
    Ok(result)
}
