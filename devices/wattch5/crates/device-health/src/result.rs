//! Product declarations and provenance are separate from frozen physiological wire payloads.
use crate::{Error, Module, ResultSource};
/// How a value was obtained; a derived estimate is never implicitly promoted to sensor measurement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcquisitionKind {
    /// A selected sensor/algorithm directly measures the claimed quantity, subject to product validation.
    SensorMeasurement,
    /// An estimate inferred from another quantity, including the legacy HR-derived SpO2/BP paths.
    DerivedEstimate,
    /// A value supplied by an authorized phone, not a device sensor measurement.
    PhoneImport,
    /// Original clock-generated display value; explicitly not a measurement or estimate.
    LegacyGenerated,
}
/// Actual provenance. These fields do not get appended to frozen HR/LATEST payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Provenance<'a> {
    /// Actual algorithm/import source.
    pub source: ResultSource,
    /// Actual acquisition method, validated against a trusted product declaration.
    pub acquisition: AcquisitionKind,
    /// Actual selected algorithm/provider identifier, not a synthetic default.
    pub algorithm_id: &'a str,
    /// Actual selected algorithm/provider version.
    pub algorithm_version: &'a str,
}
/// Versioned product-defined mood label; no universal mood enum is invented here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MoodLabel<'a> {
    /// Signed product dictionary value.
    pub value: i32,
    /// Nonempty human-readable label in the selected product dictionary.
    pub label: &'a str,
}
/// Temperature result mode. It is a result kind, not a display unit or a policy's acquisition mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum TemperatureMode {
    /// Object temperature.
    Object = 1,
    /// Forehead temperature.
    Forehead = 2,
    /// Body temperature.
    Body = 3,
    /// Skin temperature, only where a real selected backend supports it.
    Skin = 4,
}
impl TemperatureMode {
    /// Decode the proposed result mode dictionary.
    /// # Errors
    /// Zero and values above4 are invalid; no default mode is guessed.
    pub const fn from_wire(value: u8) -> Result<Self, Error> {
        match value {
            1 => Ok(Self::Object),
            2 => Ok(Self::Forehead),
            3 => Ok(Self::Body),
            4 => Ok(Self::Skin),
            _ => Err(Error::InvalidField),
        }
    }
    /// Product mode bit:bit0object,bit1forehead,bit2body,bit3skin.
    #[must_use]
    pub const fn product_bit(self) -> u8 {
        1 << (self as u8 - 1)
    }
}
pub(crate) mod sealed {
    pub trait Sealed {}
}
/// Metadata and validation for the concrete supported result schemas. Sealed to prevent arbitrary
/// external types from presenting themselves as an implemented built-in physiological schema.
pub trait ResultBody: sealed::Sealed {
    /// Stable owner module.
    fn module(&self) -> u8;
    /// Stable record type.
    fn record_type(&self) -> u16;
    /// Concrete payload schema implemented by this type.
    fn schema(&self) -> u16;
    /// Validate all represented fields before encoding or exposure.
    /// # Errors
    /// Returns explicit invalid field/length for malformed or noncanonical data.
    fn validate(&self) -> Result<(), Error>;
    /// Whether at least one physiological result field is explicitly present.
    fn has_value(&self) -> bool;
    /// Source already present in this pending schema, if known; never inserted into frozen bodies.
    fn embedded_source(&self) -> Option<ResultSource> {
        None
    }
    /// Temperature mode, only for a temperature result.
    fn temperature_mode(&self) -> Option<TemperatureMode> {
        None
    }
    /// Algorithm fields already present in ALK schema2.
    fn embedded_algorithm(&self) -> Option<(&str, &str)> {
        None
    }
    /// A present mood value requiring product dictionary validation.
    fn mood_value(&self) -> Option<i32> {
        None
    }
}
/// A trusted, audited product declaration, not information accepted blindly from a peer.
/// Its existence does not prove real hardware/algorithm accuracy; the product owns that evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductDeclaration<'a> {
    /// Compile-selected module; this alone does not advertise runtime backend availability.
    pub module: Module,
    /// Exact claimed result type.
    pub record_type: u16,
    /// Actual audited acquisition path. Legacy estimates must be declared `DerivedEstimate`.
    pub acquisition: AcquisitionKind,
    /// Actual selected algorithm/provider ID.
    pub algorithm_id: &'a str,
    /// Actual selected algorithm/provider version.
    pub algorithm_version: &'a str,
    /// Low4 mode bits for temperature; zero for every other module.
    pub temperature_modes: u8,
    /// Nonempty unique-value dictionary only for mood2A03; empty for other types.
    pub mood_labels: &'a [MoodLabel<'a>],
}
impl ProductDeclaration<'_> {
    /// Validate schema fields, actual provenance and the selected declaration's constraints.
    /// # Errors
    /// Rejects unsupported temperature mode, undeclared mood, inconsistent source/method/version,
    /// or malformed payload. A successful check is not proof of physiological accuracy.
    pub fn validate<P: ResultBody>(
        &self,
        body: &P,
        provenance: &Provenance<'_>,
    ) -> Result<(), Error> {
        body.validate()?;
        if !self.module.accepts(self.record_type)
            || body.module() != self.module.wire()
            || body.record_type() != self.record_type
            || !valid_identifier(self.algorithm_id)
            || !valid_identifier(self.algorithm_version)
            || !valid_identifier(provenance.algorithm_id)
            || !valid_identifier(provenance.algorithm_version)
            || self.acquisition != provenance.acquisition
            || self.algorithm_id != provenance.algorithm_id
            || self.algorithm_version != provenance.algorithm_version
        {
            return Err(Error::InvalidField);
        }
        let expected_source = match self.acquisition {
            AcquisitionKind::PhoneImport => ResultSource::PhoneImport,
            AcquisitionKind::SensorMeasurement
            | AcquisitionKind::DerivedEstimate
            | AcquisitionKind::LegacyGenerated => ResultSource::DeviceAlgorithm,
        };
        if provenance.source != expected_source
            || body
                .embedded_source()
                .is_some_and(|source| source != provenance.source)
        {
            return Err(Error::InvalidField);
        }
        if self.module.wire() == 0x28 {
            if self.temperature_modes == 0 || self.temperature_modes & !15 != 0 {
                return Err(Error::InvalidField);
            }
            let mode = body.temperature_mode().ok_or(Error::InvalidField)?;
            if self.temperature_modes & mode.product_bit() == 0 {
                return Err(Error::Unsupported);
            }
        } else if self.temperature_modes != 0 {
            return Err(Error::InvalidField);
        }
        if self.record_type == 0x2a03 {
            if self.mood_labels.is_empty() || u8::try_from(self.mood_labels.len()).is_err() {
                return Err(Error::InvalidField);
            }
            for (index, label) in self.mood_labels.iter().enumerate() {
                if !valid_identifier(label.label)
                    || self.mood_labels[..index]
                        .iter()
                        .any(|old| old.value == label.value)
                {
                    return Err(Error::InvalidField);
                }
            }
            if body
                .mood_value()
                .is_some_and(|value| !self.mood_labels.iter().any(|label| label.value == value))
            {
                return Err(Error::InvalidField);
            }
        } else if !self.mood_labels.is_empty() {
            return Err(Error::InvalidField);
        }
        if let Some((id, version)) = body.embedded_algorithm()
            && (body.has_value() || !id.is_empty() || !version.is_empty())
            && (id != self.algorithm_id || version != self.algorithm_version)
        {
            return Err(Error::InvalidField);
        }
        Ok(())
    }
    /// An additional gate for an actual sensor-measurement claim. Estimates/imports and absent
    /// values cannot pass; no encoding or helper automatically promotes their acquisition class.
    /// # Errors
    /// Returns `NotFound` for a canonical failed/absent result; other inconsistencies are explicit.
    pub fn validate_sensor_measurement<P: ResultBody>(
        &self,
        body: &P,
        provenance: &Provenance<'_>,
    ) -> Result<(), Error> {
        self.validate(body, provenance)?;
        if self.acquisition != AcquisitionKind::SensorMeasurement {
            return Err(Error::InvalidField);
        }
        if !body.has_value() {
            return Err(Error::NotFound);
        }
        Ok(())
    }
}
pub(crate) fn valid_identifier(value: &str) -> bool {
    !value.is_empty() && !value.as_bytes().contains(&0) && u16::try_from(value.len()).is_ok()
}
/// Validate one exact implemented physiological body and any embedded source against actual metadata.
/// This does not infer acquisition class or validate a product-specific mood dictionary; those require
/// `ProductDeclaration`. Unknown schemas are explicitly unsupported,not treated as arbitrary bytes.
/// # Errors
/// Rejects disabled module,wrong type,unknown schema,malformed payload or contradictory embeddedsource.
pub fn validate_result_body(
    module: u8,
    record_type: u16,
    schema: u16,
    payload: &[u8],
    actual_source: ResultSource,
) -> Result<(), Error> {
    let module = Module::from_wire(module)?;
    if !module.accepts(record_type) {
        return Err(Error::InvalidField);
    }
    if schema == 2 && matches!(module.wire(), 0x25..=0x27) {
        let sourced = crate::SourcedResult::decode(module.wire(), payload)?;
        return if sourced.origin().source() == actual_source {
            Ok(())
        } else {
            Err(Error::InvalidField)
        };
    }
    validate_plain_result(module, record_type, schema, payload, actual_source)
}
// Envelope validation calls this leaf directly, so a source wrapper cannot
// recurse through envelope decoding again. The public validator is unchanged.
pub(crate) fn validate_plain_result(
    module: Module,
    record_type: u16,
    schema: u16,
    payload: &[u8],
    actual_source: ResultSource,
) -> Result<(), Error> {
    if !module.accepts(record_type) {
        return Err(Error::InvalidField);
    }
    // Feature elision can remove every concrete arm.
    let _ = (schema, payload, actual_source);
    match module.wire() {
        #[cfg(feature = "heart-rate")]
        0x20 => {
            if schema != 1 {
                return Err(Error::Unsupported);
            }
            check_source(&crate::HeartRate::decode(payload)?, actual_source)
        }
        #[cfg(feature = "blood-pressure")]
        0x25 => {
            if schema != 1 {
                return Err(Error::Unsupported);
            }
            check_source(&crate::BloodPressure::decode(payload)?, actual_source)
        }
        #[cfg(feature = "oxygen")]
        0x26 => {
            if schema != 1 {
                return Err(Error::Unsupported);
            }
            check_source(&crate::Oxygen::decode(payload)?, actual_source)
        }
        #[cfg(feature = "glucose")]
        0x27 => {
            if schema != 1 {
                return Err(Error::Unsupported);
            }
            check_source(&crate::Glucose::decode(payload)?, actual_source)
        }
        #[cfg(feature = "temperature")]
        0x28 => {
            if schema != 1 {
                return Err(Error::Unsupported);
            }
            check_source(&crate::Temperature::decode(payload)?, actual_source)
        }
        #[cfg(feature = "ecg")]
        0x29 => {
            if schema != 2 {
                return Err(Error::Unsupported);
            }
            check_source(&crate::Ecg::decode(payload)?, actual_source)
        }
        #[cfg(feature = "wellbeing")]
        0x2a => {
            if record_type == 0x2a01 && schema == 2 {
                return check_source(&crate::FatigueSnapshot::decode(payload)?, actual_source);
            }
            if schema != 1 {
                return Err(Error::Unsupported);
            }
            check_source(
                &crate::Wellbeing::decode(record_type, payload)?,
                actual_source,
            )
        }
        #[cfg(feature = "microcirculation")]
        0x2b => {
            if schema != 2 {
                return Err(Error::Unsupported);
            }
            check_source(&crate::Alk::decode(payload)?, actual_source)
        }
        _ => Err(Error::Unsupported),
    }
}
#[cfg(any(
    feature = "heart-rate",
    feature = "blood-pressure",
    feature = "oxygen",
    feature = "glucose",
    feature = "temperature",
    feature = "ecg",
    feature = "wellbeing",
    feature = "microcirculation"
))]
fn check_source<P: ResultBody>(body: &P, source: ResultSource) -> Result<(), Error> {
    if body
        .embedded_source()
        .is_some_and(|embedded| embedded != source)
    {
        Err(Error::InvalidField)
    } else {
        Ok(())
    }
}
