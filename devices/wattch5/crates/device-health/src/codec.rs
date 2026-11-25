//! Typed business envelopes. These codecs do not produce physiological values.
use crate::Error;

/// A module selected at compile time. Disabled modules cannot be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Module(u8);
impl Module {
    /// Resolve a wire module only if its Cargo feature was selected.
    /// # Errors
    /// Returns `Unsupported` for disabled or unknown modules.
    pub fn from_wire(value: u8) -> Result<Self, Error> {
        match value {
            #[cfg(feature = "heart-rate")]
            0x20 => Ok(Self(value)),
            #[cfg(feature = "blood-pressure")]
            0x25 => Ok(Self(value)),
            #[cfg(feature = "oxygen")]
            0x26 => Ok(Self(value)),
            #[cfg(feature = "glucose")]
            0x27 => Ok(Self(value)),
            #[cfg(feature = "temperature")]
            0x28 => Ok(Self(value)),
            #[cfg(feature = "ecg")]
            0x29 => Ok(Self(value)),
            #[cfg(feature = "wellbeing")]
            0x2a => Ok(Self(value)),
            #[cfg(feature = "microcirculation")]
            0x2b => Ok(Self(value)),
            _ => Err(Error::Unsupported),
        }
    }
    /// Stable wire module number, never a registration index.
    #[must_use]
    pub const fn wire(self) -> u8 {
        self.0
    }
    /// Whether this selected module owns the proposed result type.
    #[must_use]
    pub const fn accepts(self, record_type: u16) -> bool {
        match (self.0, record_type) {
            #[cfg(feature = "heart-rate")]
            (0x20, 0x0020) => true,
            #[cfg(feature = "blood-pressure")]
            (0x25, 0x2501) => true,
            #[cfg(feature = "oxygen")]
            (0x26, 0x2601) => true,
            #[cfg(feature = "glucose")]
            (0x27, 0x2701) => true,
            #[cfg(feature = "temperature")]
            (0x28, 0x2801) => true,
            #[cfg(feature = "ecg")]
            (0x29, 0x2901) => true,
            #[cfg(feature = "wellbeing")]
            (0x2a, 0x2a01..=0x2a05) => true,
            #[cfg(feature = "microcirculation")]
            (0x2b, 0x2b01) => true,
            _ => false,
        }
    }
}
/// Proposed start operation, not the policy property's mode dictionary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MeasurementMode {
    /// One measurement.
    Single = 1,
    /// Repeated measurement only if the backend supports it.
    Continuous = 2,
}
/// Exact eight-byte health start request; backend validates live context/modes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HealthStart {
    module: Module,
    mode: MeasurementMode,
    record_type: u16,
    context_id: u32,
    temp_mode: u8,
}
impl HealthStart {
    /// Exact business request size, excluding the common frame header.
    pub const LENGTH: usize = 8;
    /// Decode a selected module's exact START body.
    /// # Errors
    /// Rejects trailing/missing bytes, unknown module/type/mode and inapplicable context/temperature.
    pub fn decode(module: u8, bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() != Self::LENGTH {
            return Err(Error::InvalidLength);
        }
        let module = Module::from_wire(module)?;
        let mode = match bytes[0] {
            1 => MeasurementMode::Single,
            2 => MeasurementMode::Continuous,
            _ => return Err(Error::InvalidField),
        };
        let record_type = u16::from_le_bytes([bytes[1], bytes[2]]);
        let context_id = u32::from_le_bytes([bytes[3], bytes[4], bytes[5], bytes[6]]);
        let temp_mode = bytes[7];
        if !module.accepts(record_type)
            || (context_id != 0 && module.wire() != 0x20)
            || (module.wire() == 0x28 && !(1..=4).contains(&temp_mode))
            || (module.wire() != 0x28 && temp_mode != 0)
        {
            return Err(Error::InvalidField);
        }
        Ok(Self {
            module,
            mode,
            record_type,
            context_id,
            temp_mode,
        })
    }
    /// Encode the exact body into caller storage.
    /// # Errors
    /// Returns `Capacity` without a partial write when output is short.
    pub fn encode(self, output: &mut [u8]) -> Result<usize, Error> {
        if output.len() < Self::LENGTH {
            return Err(Error::Capacity);
        }
        output[0] = self.mode as u8;
        output[1..3].copy_from_slice(&self.record_type.to_le_bytes());
        output[3..7].copy_from_slice(&self.context_id.to_le_bytes());
        output[7] = self.temp_mode;
        Ok(Self::LENGTH)
    }
    /// Selected owner module.
    #[must_use]
    pub const fn module(self) -> Module {
        self.module
    }
    /// Requested acquisition mode; the backend must explicitly accept it.
    #[must_use]
    pub const fn mode(self) -> MeasurementMode {
        self.mode
    }
    /// Requested result type.
    #[must_use]
    pub const fn record_type(self) -> u16 {
        self.record_type
    }
    /// Zero for independent measurement; live nonzero aliases need backend validation.
    #[must_use]
    pub const fn context_id(self) -> u32 {
        self.context_id
    }
    /// Zero outside temperature; proposed object/forehead/body/skin values1..4.
    #[must_use]
    pub const fn temp_mode(self) -> u8 {
        self.temp_mode
    }
}
/// UTC validity is independent from measurement validity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timestamp {
    utc_ms: i64,
    valid: bool,
}
impl Timestamp {
    /// Explicit absence of a trustworthy UTC timestamp, not the Unix epoch.
    #[must_use]
    pub const fn unknown() -> Self {
        Self {
            utc_ms: 0,
            valid: false,
        }
    }
    /// Construct a canonical timestamp. Epoch0 may be valid.
    /// # Errors
    /// Rejects a nonzero timestamp marked invalid.
    pub const fn new(utc_ms: i64, valid: bool) -> Result<Self, Error> {
        if !valid && utc_ms != 0 {
            return Err(Error::InvalidField);
        }
        Ok(Self { utc_ms, valid })
    }
    /// Raw UTC milliseconds; meaningful only with `is_valid`.
    #[must_use]
    pub const fn utc_ms(self) -> i64 {
        self.utc_ms
    }
    /// Whether UTC is trustworthy.
    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.valid
    }
}
/// Borrowed LATEST business envelope, without a response's common status/reason.
/// Its payload is opaque to this service; a selected schema codec must validate physiological fields.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Latest<'a> {
    module: Module,
    record_type: u16,
    schema: u16,
    timestamp: Timestamp,
    payload: &'a [u8],
}
impl core::fmt::Debug for Latest<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("Latest")
            .field("module", &self.module)
            .field("record_type", &self.record_type)
            .field("schema", &self.schema)
            .field("timestamp", &self.timestamp)
            .field("payload_length", &self.payload.len())
            .finish()
    }
}
impl<'a> Latest<'a> {
    /// Header length excluding the payload.
    pub const HEADER_LENGTH: usize = 15;
    /// Largest business envelope accepted by wire1.2. Response callers also reserve their common4B.
    pub const MAX_LENGTH: usize = 512;
    /// Wrap an actual backend payload; this does not assert sensor accuracy or payload-schema validity.
    /// # Errors
    /// Rejects unsupported ownership, schema0, empty result and excessive logical length.
    pub fn new(
        module: u8,
        record_type: u16,
        schema: u16,
        timestamp: Timestamp,
        payload: &'a [u8],
    ) -> Result<Self, Error> {
        let module = Module::from_wire(module)?;
        if !module.accepts(record_type) || schema == 0 {
            return Err(Error::InvalidField);
        }
        if payload.is_empty() || payload.len() > Self::MAX_LENGTH - Self::HEADER_LENGTH {
            return Err(Error::InvalidLength);
        }
        Ok(Self {
            module,
            record_type,
            schema,
            timestamp,
            payload,
        })
    }
    /// Decode one exact envelope. No trailing bytes or permissive validity flags.
    /// # Errors
    /// Rejects malformed length, flags or module/type/schema ownership.
    pub fn decode(module: u8, bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() < Self::HEADER_LENGTH || bytes.len() > Self::MAX_LENGTH {
            return Err(Error::InvalidLength);
        }
        let record_type = u16::from_le_bytes([bytes[0], bytes[1]]);
        let schema = u16::from_le_bytes([bytes[2], bytes[3]]);
        let mut utc = [0; 8];
        utc.copy_from_slice(&bytes[4..12]);
        let valid = match bytes[12] {
            0 => false,
            1 => true,
            _ => return Err(Error::InvalidField),
        };
        let timestamp = Timestamp::new(i64::from_le_bytes(utc), valid)?;
        let length = usize::from(u16::from_le_bytes([bytes[13], bytes[14]]));
        if length != bytes.len() - Self::HEADER_LENGTH {
            return Err(Error::InvalidLength);
        }
        Self::new(
            module,
            record_type,
            schema,
            timestamp,
            &bytes[Self::HEADER_LENGTH..],
        )
    }
    /// Encode without a command status prefix.
    /// # Errors
    /// Returns `Capacity` before writing if output is too short.
    pub fn encode(self, output: &mut [u8]) -> Result<usize, Error> {
        let length = self.payload.len() + Self::HEADER_LENGTH;
        if output.len() < length {
            return Err(Error::Capacity);
        }
        let payload_length = u16::try_from(self.payload.len()).map_err(|_| Error::InvalidLength)?;
        output[..2].copy_from_slice(&self.record_type.to_le_bytes());
        output[2..4].copy_from_slice(&self.schema.to_le_bytes());
        output[4..12].copy_from_slice(&self.timestamp.utc_ms().to_le_bytes());
        output[12] = u8::from(self.timestamp.is_valid());
        output[13..15].copy_from_slice(&payload_length.to_le_bytes());
        output[15..length].copy_from_slice(self.payload);
        Ok(length)
    }
    /// Owning selected module.
    #[must_use]
    pub const fn module(self) -> Module {
        self.module
    }
    /// Explicit result type, not inferred from payload size.
    #[must_use]
    pub const fn record_type(self) -> u16 {
        self.record_type
    }
    /// Versioned payload schema.
    #[must_use]
    pub const fn schema(self) -> u16 {
        self.schema
    }
    /// Independent timestamp validity.
    #[must_use]
    pub const fn timestamp(self) -> Timestamp {
        self.timestamp
    }
    /// Borrowed actual payload; never copied into a Debug log.
    #[must_use]
    pub const fn payload(self) -> &'a [u8] {
        self.payload
    }
}
/// Distinct logical carriers for one physiological body. Coding ceilings do not grant carrier capacity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultCarrier {
    /// Event LATEST envelope15B,without a command status prefix.
    EventLatest,
    /// Successful command response:commonstatus4B plus LATEST envelope15B.
    ResponseLatest,
    /// History RECORD envelope38B; its body does not nest the LATEST15B header.
    HistoryRecord,
}
impl ResultCarrier {
    /// Check actual encoded logical length against the direction's negotiated maximum.
    /// Disabled direction0 is unsupported; otherwise wire1.2 maxima must be128..512.
    /// This never truncates a real result. Extra carrier-specific headers/CRC,if any,must also be
    /// accounted for by the final business/transport encoder before it admits an event.
    /// # Errors
    /// Overflow,invalid/disabled limit or insufficient actual carrier capacity is explicit.
    pub fn checked_length(self, body_length: usize, outbound_max: usize) -> Result<usize, Error> {
        if outbound_max == 0 {
            return Err(Error::Unsupported);
        }
        if !(128..=512).contains(&outbound_max) {
            return Err(Error::InvalidField);
        }
        let overhead = match self {
            Self::EventLatest => 15,
            Self::ResponseLatest => 19,
            Self::HistoryRecord => 38,
        };
        let length = body_length
            .checked_add(overhead)
            .ok_or(Error::InvalidLength)?;
        if length > outbound_max {
            Err(Error::Capacity)
        } else {
            Ok(length)
        }
    }
}
impl Latest<'_> {
    /// Check this actual body's selected outbound carrier before response/event admission.
    /// A497B body may fit512B EVENT LATEST while failing RESP and RECORD; no body is truncated.
    /// # Errors
    /// Returns the selected carrier's explicit bound/overflow error.
    pub fn check_carrier(
        self,
        carrier: ResultCarrier,
        outbound_max: usize,
    ) -> Result<usize, Error> {
        carrier.checked_length(self.payload.len(), outbound_max)
    }
}

#[cfg(any(
    feature = "heart-rate",
    feature = "blood-pressure",
    feature = "oxygen",
    feature = "temperature"
))]
mod threshold {
    use crate::{Error, ProductDeclaration, Provenance, ResultInfo, Timestamp};
    /// Threshold episode transition; this is not a measurement validity or provenance dictionary.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    #[repr(u8)]
    pub enum ThresholdTransition {
        /// First valid abnormal observation in an episode.
        Trigger = 1,
        /// Actual valid observation cleared the condition.
        ConditionClear = 2,
        /// Device user silenced an active episode using its last actual valid observation.
        DeviceSilence = 3,
    }
    impl ThresholdTransition {
        fn decode(value: u8) -> Result<Self, Error> {
            match value {
                1 => Ok(Self::Trigger),
                2 => Ok(Self::ConditionClear),
                3 => Ok(Self::DeviceSilence),
                _ => Err(Error::InvalidField),
            }
        }
    }
    /// The actual valid target field and its configured threshold, in the module's declared unit.
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub enum ThresholdSample {
        /// Module20, BPM. An event carries an actual present HR field.
        #[cfg(feature = "heart-rate")]
        HeartRate {
            /// Actual BPM.
            value: u16,
            /// Positive configured BPM threshold.
            threshold: u16,
        },
        /// Module25, actual systolic mmHg, never diastolic or pulse substitution.
        #[cfg(feature = "blood-pressure")]
        BloodPressure {
            /// Actual systolic mmHg.
            value: u16,
            /// Positive configured systolic threshold.
            threshold: u16,
        },
        /// Module26, `SpO2` percentage times100, never a PI substitution.
        #[cfg(feature = "oxygen")]
        Oxygen {
            /// Actual `SpO2` percentage times100.
            value: u16,
            /// Configured low threshold in percentage times100.
            threshold: u16,
        },
        /// Module28, signed centi-Celsius in the actual supported configured mode.
        #[cfg(feature = "temperature")]
        Temperature {
            /// Actual centi-Celsius.
            value: i16,
            /// Configured high threshold in centi-Celsius.
            threshold: i16,
            /// Actual supported measurement mode.
            mode: crate::TemperatureMode,
        },
    }
    impl core::fmt::Debug for ThresholdSample {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.debug_struct("ThresholdSample").finish_non_exhaustive()
        }
    }
    impl ThresholdSample {
        /// Owning selected module; no runtime availability is asserted.
        #[must_use]
        pub const fn module(self) -> u8 {
            match self {
                #[cfg(feature = "heart-rate")]
                Self::HeartRate { .. } => 0x20,
                #[cfg(feature = "blood-pressure")]
                Self::BloodPressure { .. } => 0x25,
                #[cfg(feature = "oxygen")]
                Self::Oxygen { .. } => 0x26,
                #[cfg(feature = "temperature")]
                Self::Temperature { .. } => 0x28,
            }
        }
        fn record_type(self) -> u16 {
            match self.module() {
                0x20 => 0x20,
                0x25 => 0x2501,
                0x26 => 0x2601,
                _ => 0x2801,
            }
        }
        fn validate(self) -> Result<(), Error> {
            let valid = match self {
                #[cfg(feature = "heart-rate")]
                Self::HeartRate { threshold, .. } => threshold > 0,
                #[cfg(feature = "blood-pressure")]
                Self::BloodPressure { threshold, .. } => threshold > 0,
                #[cfg(feature = "oxygen")]
                Self::Oxygen { value, threshold } => value <= 10000 && threshold <= 10000,
                #[cfg(feature = "temperature")]
                Self::Temperature { .. } => true,
            };
            if valid {
                Ok(())
            } else {
                Err(Error::InvalidField)
            }
        }
        fn wire_values(self) -> ([u8; 2], [u8; 2]) {
            match self {
                #[cfg(feature = "heart-rate")]
                Self::HeartRate { value, threshold } => {
                    (value.to_le_bytes(), threshold.to_le_bytes())
                }
                #[cfg(feature = "blood-pressure")]
                Self::BloodPressure { value, threshold } => {
                    (value.to_le_bytes(), threshold.to_le_bytes())
                }
                #[cfg(feature = "oxygen")]
                Self::Oxygen { value, threshold } => (value.to_le_bytes(), threshold.to_le_bytes()),
                #[cfg(feature = "temperature")]
                Self::Temperature {
                    value, threshold, ..
                } => (value.to_le_bytes(), threshold.to_le_bytes()),
            }
        }
    }
    /// E83 ordinary body:22B for20/25/26,23B for28. No public status, source byte or CRC is appended.
    /// This codec does not implement a crossing policy, episode scheduler or authorized event source.
    #[derive(Clone, Copy, PartialEq, Eq)]
    pub struct ThresholdEvent {
        /// Nonzero module+boot episode identity, retained for clear/silence. Runtime owns no-reuse.
        pub alarm_id: u32,
        /// Actual episode transition, decided by a real selected policy engine.
        pub transition: ThresholdTransition,
        /// Actual same-module App job, or explicit device/local observation0; never an App fallback.
        pub job: u32,
        /// Actual observation UTC. Silence retains the previous actual observation time.
        pub timestamp: Timestamp,
        /// Actual present field and immutable threshold/mode snapshot.
        pub sample: ThresholdSample,
    }
    impl core::fmt::Debug for ThresholdEvent {
        fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            f.debug_struct("ThresholdEvent")
                .field("encoded_length", &self.encoded_length())
                .finish_non_exhaustive()
        }
    }
    impl ThresholdEvent {
        /// Exact logical body length, without an outer frame or CRC.
        #[must_use]
        pub const fn encoded_length(self) -> usize {
            if self.sample.module() == 0x28 { 23 } else { 22 }
        }
        fn validate(self) -> Result<(), Error> {
            if self.alarm_id == 0 {
                return Err(Error::InvalidField);
            }
            self.sample.validate()
        }
        /// Decode one complete selected module's E83 body; absent samples have no representation.
        /// # Errors
        /// Rejects disabled/unadopted modules, lengths, IDs, transition, UTC flags, units or modes.
        pub fn decode(module: u8, bytes: &[u8]) -> Result<Self, Error> {
            crate::Module::from_wire(module)?;
            if !matches!(module, 0x20 | 0x25 | 0x26 | 0x28) {
                return Err(Error::Unsupported);
            }
            if bytes.len() != if module == 0x28 { 23 } else { 22 } {
                return Err(Error::InvalidLength);
            }
            let mut utc = [0; 8];
            utc.copy_from_slice(&bytes[9..17]);
            let valid = match bytes[17] {
                0 => false,
                1 => true,
                _ => return Err(Error::InvalidField),
            };
            let value = u16::from_le_bytes([bytes[18], bytes[19]]);
            let threshold = u16::from_le_bytes([bytes[20], bytes[21]]);
            let sample = match module {
                #[cfg(feature = "heart-rate")]
                0x20 => ThresholdSample::HeartRate { value, threshold },
                #[cfg(feature = "blood-pressure")]
                0x25 => ThresholdSample::BloodPressure { value, threshold },
                #[cfg(feature = "oxygen")]
                0x26 => ThresholdSample::Oxygen { value, threshold },
                #[cfg(feature = "temperature")]
                0x28 => ThresholdSample::Temperature {
                    value: i16::from_le_bytes(value.to_le_bytes()),
                    threshold: i16::from_le_bytes(threshold.to_le_bytes()),
                    mode: crate::TemperatureMode::from_wire(bytes[22])?,
                },
                _ => return Err(Error::Unsupported),
            };
            let event = Self {
                alarm_id: u32::from_le_bytes(
                    bytes[..4].try_into().map_err(|_| Error::InvalidLength)?,
                ),
                transition: ThresholdTransition::decode(bytes[4])?,
                job: u32::from_le_bytes(bytes[5..9].try_into().map_err(|_| Error::InvalidLength)?),
                timestamp: Timestamp::new(i64::from_le_bytes(utc), valid)?,
                sample,
            };
            event.validate()?;
            Ok(event)
        }
        /// Encode a complete validated body; this does not register or produce a runtime alarm.
        /// # Errors
        /// Invalid scalar fields or insufficient output fails before writing.
        pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
            self.validate()?;
            let n = self.encoded_length();
            if out.len() < n {
                return Err(Error::Capacity);
            }
            out[..4].copy_from_slice(&self.alarm_id.to_le_bytes());
            out[4] = self.transition as u8;
            out[5..9].copy_from_slice(&self.job.to_le_bytes());
            out[9..17].copy_from_slice(&self.timestamp.utc_ms().to_le_bytes());
            out[17] = u8::from(self.timestamp.is_valid());
            let (value, threshold) = self.sample.wire_values();
            out[18..20].copy_from_slice(&value);
            out[20..22].copy_from_slice(&threshold);
            #[cfg(feature = "temperature")]
            match self.sample {
                ThresholdSample::Temperature { mode, .. } => out[22] = mode as u8,
                #[cfg(any(feature = "heart-rate", feature = "blood-pressure", feature = "oxygen"))]
                _ => {}
            }
            Ok(n)
        }
        /// Bind fields to the actual immutable observation and its declared source before use.
        /// Imports/estimates retain their classification; validation never promotes sensor capability.
        /// Missing/invalid target fields return `NotFound`: no trigger or clearance may be inferred.
        /// Crossing, job ownership, episode/no-reuse policy and live authorization remain runtime work.
        /// # Errors
        /// Rejects mismatched identity/time/schema/value/mode or declaration/provenance; absent target is explicit.
        pub fn validate_observation(
            self,
            info: ResultInfo,
            payload: &[u8],
            declaration: &ProductDeclaration<'_>,
            provenance: &Provenance<'_>,
        ) -> Result<(), Error> {
            self.validate()?;
            if info.sample_id == 0
                || info.record_type != self.sample.record_type()
                || info.source != provenance.source
                || info.length != payload.len()
                || info.timestamp != self.timestamp
                || declaration.module.wire() != self.sample.module()
                || declaration.record_type != info.record_type
            {
                return Err(Error::InvalidField);
            }
            if info.schema != 1 {
                return Err(Error::Unsupported);
            }
            crate::validate_result_body(
                self.sample.module(),
                info.record_type,
                info.schema,
                payload,
                info.source,
            )?;
            match self.sample {
                #[cfg(feature = "heart-rate")]
                ThresholdSample::HeartRate { value, .. } => {
                    let body = crate::HeartRate::decode(payload)?;
                    declaration.validate(&body, provenance)?;
                    expected(body.bpm, value)
                }
                #[cfg(feature = "blood-pressure")]
                ThresholdSample::BloodPressure { value, .. } => {
                    let body = crate::BloodPressure::decode(payload)?;
                    declaration.validate(&body, provenance)?;
                    expected(body.systolic, value)
                }
                #[cfg(feature = "oxygen")]
                ThresholdSample::Oxygen { value, .. } => {
                    let body = crate::Oxygen::decode(payload)?;
                    declaration.validate(&body, provenance)?;
                    expected(body.spo2_x100, value)
                }
                #[cfg(feature = "temperature")]
                ThresholdSample::Temperature { value, mode, .. } => {
                    let body = crate::Temperature::decode(payload)?;
                    declaration.validate(&body, provenance)?;
                    if body.mode != mode {
                        return Err(Error::InvalidField);
                    }
                    expected(body.centi_c, value)
                }
            }
        }
    }
    fn expected<T: PartialEq + Copy>(actual: Option<T>, value: T) -> Result<(), Error> {
        match actual {
            None => Err(Error::NotFound),
            Some(actual) if actual == value => Ok(()),
            Some(_) => Err(Error::InvalidField),
        }
    }
}
#[cfg(any(
    feature = "heart-rate",
    feature = "blood-pressure",
    feature = "oxygen",
    feature = "temperature"
))]
pub use threshold::{ThresholdEvent, ThresholdSample, ThresholdTransition};
