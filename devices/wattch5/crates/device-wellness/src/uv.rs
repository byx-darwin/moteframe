//! Concrete 2E result,policy and events. No sampling algorithm or installed sensor backend.
use crate::{Error, Timestamp, boolean, exact};
/// Proposed2E01/schema1,independent result presence. Some(0) may be a real reading,None is absent.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct UvReading {
    /// Actual UV index*100,not clamped to a legacy0..11 range.
    pub index_x100: Option<u16>,
    /// Actual intensity in micro-W/cm²,not inferred from index.
    pub intensity_uw_cm2: Option<u32>,
}
redacted!(UvReading, "UvReading", 7);
impl UvReading {
    /// Exact body size.
    pub const LENGTH: usize = 7;
    /// Decode maskbits0index/1intensity and canonical0 absent slots.
    /// # Errors
    /// Unknown mask,nonzero absent slot or wrong length fails.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        exact(bytes, 7)?;
        let index = u16::from_le_bytes([bytes[0], bytes[1]]);
        let intensity = u32::from_le_bytes([bytes[2], bytes[3], bytes[4], bytes[5]]);
        let mask = bytes[6];
        if mask & !3 != 0 || (mask & 1 == 0 && index != 0) || (mask & 2 == 0 && intensity != 0) {
            return Err(Error::InvalidField);
        }
        Ok(Self {
            index_x100: if mask & 1 != 0 { Some(index) } else { None },
            intensity_uw_cm2: if mask & 2 != 0 { Some(intensity) } else { None },
        })
    }
    /// Whether any actual result field is present; this does not certify sensor accuracy.
    #[must_use]
    pub const fn has_value(self) -> bool {
        self.index_x100.is_some() || self.intensity_uw_cm2.is_some()
    }
    /// Encode exact7B without inventing an absent index/intensity.
    /// # Errors
    /// Short output fails before writing.
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
        if out.len() < 7 {
            return Err(Error::Capacity);
        }
        out[..2].copy_from_slice(&self.index_x100.unwrap_or(0).to_le_bytes());
        out[2..6].copy_from_slice(&self.intensity_uw_cm2.unwrap_or(0).to_le_bytes());
        out[6] =
            u8::from(self.index_x100.is_some()) | u8::from(self.intensity_uw_cm2.is_some()) << 1;
        Ok(7)
    }
}
/// Proposed UV policy; report timing is independent of sensor detection and alarm evaluation.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct UvPolicy {
    /// Enable E80 reports,not the sensor algorithm.
    pub report_enabled: bool,
    /// Alarm enable independent of reports.
    pub alarm_enabled: bool,
    /// Configured index threshold*100.
    pub threshold_x100: u16,
    /// Minimum E80 report interval in seconds.
    pub interval_s: u32,
}
redacted!(UvPolicy, "UvPolicy", 8);
impl UvPolicy {
    /// Decode exact8B property,strict booleans and enabled interval.
    /// # Errors
    /// Invalid length/boolean or enabled interval0 fails.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        exact(bytes, 8)?;
        let value = Self {
            report_enabled: boolean(bytes[0])?,
            alarm_enabled: boolean(bytes[1])?,
            threshold_x100: u16::from_le_bytes([bytes[2], bytes[3]]),
            interval_s: u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]),
        };
        value.validate()?;
        Ok(value)
    }
    /// Validate only the candidate policy encoding; actual ranges are product-declared.
    /// # Errors
    /// Enabled report interval must be positive.
    pub const fn validate(self) -> Result<(), Error> {
        if self.report_enabled && self.interval_s == 0 {
            Err(Error::InvalidField)
        } else {
            Ok(())
        }
    }
    /// Encode exact8B without changing a disabled retained policy.
    /// # Errors
    /// Invalid policy or short output fails before writing.
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
        self.validate()?;
        if out.len() < 8 {
            return Err(Error::Capacity);
        }
        out[0] = u8::from(self.report_enabled);
        out[1] = u8::from(self.alarm_enabled);
        out[2..4].copy_from_slice(&self.threshold_x100.to_le_bytes());
        out[4..8].copy_from_slice(&self.interval_s.to_le_bytes());
        Ok(8)
    }
}
/// Trigger source defined by the UV candidate; not an algorithm/sensor-truth provenance claim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum UvSource {
    /// Device sampling.
    DeviceSampling = 1,
    /// Explicit device user action.
    DeviceUser = 2,
    /// App request.
    AppRequest = 3,
}
impl UvSource {
    /// Decode the strict source dictionary.
    /// # Errors
    /// Unknown source is rejected.
    pub const fn from_wire(value: u8) -> Result<Self, Error> {
        match value {
            1 => Ok(Self::DeviceSampling),
            2 => Ok(Self::DeviceUser),
            3 => Ok(Self::AppRequest),
            _ => Err(Error::InvalidField),
        }
    }
}
/// Alarm transition references the original alarm ID,not a new cancellation ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum AlarmTransition {
    /// New actual threshold crossing.
    Triggered = 1,
    /// Device user silenced current cycle.
    DeviceCancel = 2,
    /// App silenced current cycle.
    AppCancel = 3,
    /// Actual valid index fell below threshold.
    ConditionClear = 4,
}
impl AlarmTransition {
    /// Decode the strict transition dictionary.
    /// # Errors
    /// Unknown transition is rejected.
    pub const fn from_wire(value: u8) -> Result<Self, Error> {
        match value {
            1 => Ok(Self::Triggered),
            2 => Ok(Self::DeviceCancel),
            3 => Ok(Self::AppCancel),
            4 => Ok(Self::ConditionClear),
            _ => Err(Error::InvalidField),
        }
    }
}
fn timestamp(bytes: &[u8]) -> Result<Timestamp, Error> {
    let mut utc = [0; 8];
    utc.copy_from_slice(&bytes[..8]);
    Timestamp::new(i64::from_le_bytes(utc), boolean(bytes[8])?)
}
fn put_time(value: Timestamp, out: &mut [u8]) {
    out[..8].copy_from_slice(&value.utc_ms().to_le_bytes());
    out[8] = u8::from(value.is_valid());
}
/// Module-schema2 E80,exact21B. No command status/LATEST header is prepended.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct UvReportEvent {
    /// Nonzero boot-scoped report identity.
    pub event_id: u32,
    /// Actual trigger source.
    pub source: UvSource,
    /// Explicit UTC validity.
    pub timestamp: Timestamp,
    /// Actual independent field validity.
    pub reading: UvReading,
}
redacted!(UvReportEvent, "UvReportEvent", 21);
impl UvReportEvent {
    /// Decode exact21B event.
    /// # Errors
    /// Invalid ID/source/time/mask/length fails.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        exact(bytes, 21)?;
        let value = Self {
            event_id: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            source: UvSource::from_wire(bytes[4])?,
            timestamp: timestamp(&bytes[5..14])?,
            reading: UvReading::decode(&bytes[14..21])?,
        };
        if value.event_id == 0 {
            return Err(Error::InvalidField);
        }
        Ok(value)
    }
    /// Encode exact21B after full capacity/identity check.
    /// # Errors
    /// ID0/short output fails without partial writes.
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
        if self.event_id == 0 {
            return Err(Error::InvalidField);
        }
        if out.len() < 21 {
            return Err(Error::Capacity);
        }
        out[..4].copy_from_slice(&self.event_id.to_le_bytes());
        out[4] = self.source as u8;
        put_time(self.timestamp, &mut out[5..14]);
        self.reading.encode(&mut out[14..21])?;
        Ok(21)
    }
}
/// Module-schema2 E81,exact22B,with explicit crossing/cancel/clear transition.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct UvAlarmEvent {
    /// Original nonzero alarm identity for every related transition.
    pub event_id: u32,
    /// Actual trigger source.
    pub source: UvSource,
    /// Explicit transition,not inferred from a missing value.
    pub transition: AlarmTransition,
    /// Actual event UTC validity,independent of sample validity.
    pub timestamp: Timestamp,
    /// Last actual reading for cancel,actual new reading for crossing/clear.
    pub reading: UvReading,
}
redacted!(UvAlarmEvent, "UvAlarmEvent", 22);
impl UvAlarmEvent {
    /// Decode exact22B with explicit identity/source/transition/time/validity.
    /// # Errors
    /// Invalid field relation or length fails.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        exact(bytes, 22)?;
        let value = Self {
            event_id: u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
            source: UvSource::from_wire(bytes[4])?,
            transition: AlarmTransition::from_wire(bytes[5])?,
            timestamp: timestamp(&bytes[6..15])?,
            reading: UvReading::decode(&bytes[15..22])?,
        };
        value.validate()?;
        Ok(value)
    }
    /// Cancellation source must agree with the explicit device/App transition.
    /// # Errors
    /// ID0 or contradictory source fails.
    pub const fn validate(self) -> Result<(), Error> {
        if self.event_id == 0
            || matches!(
                (self.transition, self.source),
                (
                    AlarmTransition::DeviceCancel,
                    UvSource::DeviceSampling | UvSource::AppRequest
                ) | (
                    AlarmTransition::AppCancel,
                    UvSource::DeviceSampling | UvSource::DeviceUser
                )
            )
        {
            Err(Error::InvalidField)
        } else {
            Ok(())
        }
    }
    /// Encode exact22B without changing the original alarm ID.
    /// # Errors
    /// Invalid fields/short output fails before writing.
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
        self.validate()?;
        if out.len() < 22 {
            return Err(Error::Capacity);
        }
        out[..4].copy_from_slice(&self.event_id.to_le_bytes());
        out[4] = self.source as u8;
        out[5] = self.transition as u8;
        put_time(self.timestamp, &mut out[6..15]);
        self.reading.encode(&mut out[15..22])?;
        Ok(22)
    }
}
/// Exact 4B cancel request. Successful response has zero business bytes,not an event body.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct UvCancelRequest {
    /// Original nonzero alarm identity.
    pub event_id: u32,
}
redacted!(UvCancelRequest, "UvCancelRequest", 4);
impl UvCancelRequest {
    /// Decode exact4B nonzero identity.
    /// # Errors
    /// ID0 or extra/missing bytes fails.
    pub fn decode(bytes: &[u8]) -> Result<Self, Error> {
        exact(bytes, 4)?;
        let value = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
        if value == 0 {
            Err(Error::InvalidField)
        } else {
            Ok(Self { event_id: value })
        }
    }
    /// Encode exact4B;does not cancel any physical or local action by itself.
    /// # Errors
    /// ID0/short output fails before writing.
    pub fn encode(self, out: &mut [u8]) -> Result<usize, Error> {
        if self.event_id == 0 {
            return Err(Error::InvalidField);
        }
        if out.len() < 4 {
            return Err(Error::Capacity);
        }
        out[..4].copy_from_slice(&self.event_id.to_le_bytes());
        Ok(4)
    }
}
/// Actual sampled/imported data supplied by a selected trusted backend,not generated by this engine.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct UvSample {
    /// Nonzero strictly increasing producer identity;retry is immutable.
    pub sample_id: u64,
    /// Actual monotonic milliseconds,independent of UTC.
    pub monotonic_ms: u64,
    /// Explicit trustworthy UTC or canonical unknown.
    pub timestamp: Timestamp,
    /// Actual trigger source,not a sensor-truth claim.
    pub source: UvSource,
    /// Actual result presence;all absent is not a successful latest value.
    pub reading: UvReading,
}
redacted!(UvSample, "UvSample", 7);
#[derive(Clone, Copy)]
struct ActiveAlarm {
    id: u32,
    threshold: u16,
    cancelled: bool,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Admission {
    Ready,
    AwaitCancelResponse,
}
#[derive(Clone, Copy)]
struct PendingAlarm {
    event: UvAlarmEvent,
    admission: Admission,
}
/// One bounded UV policy engine with two immutable event slots and N boot-scoped cancellation
/// tombstones. It never evicts a known cancellation silently:full retention rejects new alarm cycles.
/// Product sizing/persistence and authorization are external;this is not a GATT registration.
pub struct UvEngine<const N: usize> {
    policy: UvPolicy,
    next_id: Option<u32>,
    latest: Option<UvSample>,
    last_report_ms: Option<u64>,
    report: Option<UvReportEvent>,
    alarm: Option<PendingAlarm>,
    active: Option<ActiveAlarm>,
    cancelled: [Option<u32>; N],
    last_report_ack: Option<u32>,
    last_alarm_ack: Option<(u32, AlarmTransition)>,
    last_sample_ack: Option<u64>,
}
impl<const N: usize> core::fmt::Debug for UvEngine<N> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("UvEngine")
            .field("retained_event_capacity", &2)
            .field("cancel_retention_capacity", &N)
            .finish()
    }
}
impl<const N: usize> UvEngine<N> {
    /// New boot-scoped engine with explicit nonzero next ID. Do not reconstruct/reset mid-boot and
    /// forget cancellation tombstones or reuse IDs;product persistence/lifecycle is external.
    /// # Errors
    /// Invalid policy/ID0 fails;no real sample is created.
    pub fn new(policy: UvPolicy, first_id: u32) -> Result<Self, Error> {
        policy.validate()?;
        if first_id == 0 {
            return Err(Error::InvalidField);
        }
        Ok(Self {
            policy,
            next_id: Some(first_id),
            latest: None,
            last_report_ms: None,
            report: None,
            alarm: None,
            active: None,
            cancelled: [None; N],
            last_report_ack: None,
            last_alarm_ack: None,
            last_sample_ack: None,
        })
    }
    /// Read actual retained latest metadata. No sample means NotFound,not a zero successful reading.
    /// # Errors
    /// No actual sample yet returns `NotFound`.
    pub fn latest(&self) -> Result<UvSample, Error> {
        self.latest.ok_or(Error::NotFound)
    }
    /// Current validated policy. Reading configuration is not a sensor availability claim.
    #[must_use]
    pub const fn policy(&self) -> UvPolicy {
        self.policy
    }
    /// Change policy without rewriting an active cycle's meaning or dropping queued events.
    /// # Errors
    /// Pending events or active-cycle alarm threshold/enable changes return Busy. Product may offer
    /// a separately specified policy migration;this conservative engine does not invent one.
    pub fn configure(&mut self, value: UvPolicy) -> Result<(), Error> {
        value.validate()?;
        if self.report.is_some()
            || self.alarm.is_some()
            || self.active.is_some()
                && (value.threshold_x100 != self.policy.threshold_x100
                    || value.alarm_enabled != self.policy.alarm_enabled)
        {
            return Err(Error::Busy);
        }
        self.policy = value;
        Ok(())
    }
    /// Ingest one actual sample. No sensor algorithm is evaluated;threshold policy only compares an
    /// explicitly present UV index. Full event slots reject new data instead of silently overwriting.
    /// # Errors
    /// Missing data,identity/time reversal,changed retries,ID exhaustion or full retention is explicit.
    pub fn ingest(&mut self, value: UvSample) -> Result<bool, Error> {
        if value.sample_id == 0 {
            return Err(Error::InvalidField);
        }
        if !value.reading.has_value() {
            return Err(Error::NotFound);
        }
        if let Some(old) = self.latest {
            if old.sample_id == value.sample_id {
                return if old == value {
                    Ok(false)
                } else {
                    Err(Error::Sequence)
                };
            }
            if value.sample_id < old.sample_id {
                return Err(Error::Sequence);
            }
            if value.monotonic_ms < old.monotonic_ms {
                return Err(Error::Time);
            }
        }
        if self.report.is_some() || self.alarm.is_some() {
            return Err(Error::Busy);
        }
        let report_due = self.policy.report_enabled
            && self.last_report_ms.is_none_or(|last| {
                value.monotonic_ms - last >= u64::from(self.policy.interval_s) * 1000
            });
        let transition = self.next_transition(value.reading);
        if transition == Some(AlarmTransition::Triggered)
            && !self.cancelled.iter().any(Option::is_none)
        {
            return Err(Error::Capacity);
        }
        let mut next = self.next_id;
        let report_id = if report_due {
            Some(allocate(&mut next)?)
        } else {
            None
        };
        let trigger_id = if transition == Some(AlarmTransition::Triggered) {
            Some(allocate(&mut next)?)
        } else {
            None
        };
        let alarm = match transition {
            Some(AlarmTransition::Triggered) => Some(UvAlarmEvent {
                event_id: trigger_id.ok_or(Error::InvalidState)?,
                source: value.source,
                transition: AlarmTransition::Triggered,
                timestamp: value.timestamp,
                reading: value.reading,
            }),
            Some(AlarmTransition::ConditionClear) => Some(UvAlarmEvent {
                event_id: self.active.ok_or(Error::InvalidState)?.id,
                source: value.source,
                transition: AlarmTransition::ConditionClear,
                timestamp: value.timestamp,
                reading: value.reading,
            }),
            _ => None,
        };
        self.next_id = next;
        self.latest = Some(value);
        if let Some(id) = report_id {
            self.report = Some(UvReportEvent {
                event_id: id,
                source: value.source,
                timestamp: value.timestamp,
                reading: value.reading,
            });
            self.last_report_ms = Some(value.monotonic_ms);
        }
        if let Some(event) = alarm {
            match event.transition {
                AlarmTransition::Triggered => {
                    self.active = Some(ActiveAlarm {
                        id: event.event_id,
                        threshold: self.policy.threshold_x100,
                        cancelled: false,
                    });
                }
                AlarmTransition::ConditionClear => self.active = None,
                _ => {}
            }
            self.alarm = Some(PendingAlarm {
                event,
                admission: Admission::Ready,
            });
        }
        Ok(true)
    }
    fn next_transition(&self, reading: UvReading) -> Option<AlarmTransition> {
        let index = reading.index_x100?;
        match self.active {
            Some(active) if index < active.threshold => Some(AlarmTransition::ConditionClear),
            None if self.policy.alarm_enabled && index >= self.policy.threshold_x100 => {
                Some(AlarmTransition::Triggered)
            }
            _ => None,
        }
    }
    /// Peek the same E80 until actual transport admission. No payload is logged by Debug.
    #[must_use]
    pub const fn pending_report(&self) -> Option<UvReportEvent> {
        self.report
    }
    /// Peek E81 only after any cancel response was actually admitted first.
    #[must_use]
    pub fn pending_alarm(&self) -> Option<UvAlarmEvent> {
        self.alarm
            .filter(|alarm| alarm.admission == Admission::Ready)
            .map(|alarm| alarm.event)
    }
    /// Confirm E80 queue admission;duplicate acknowledgement cannot clear another report.
    /// # Errors
    /// Unknown report ID fails.
    pub fn confirm_report(&mut self, id: u32) -> Result<(), Error> {
        if self.last_report_ack == Some(id) {
            return Ok(());
        }
        if self.report.is_none_or(|report| report.event_id != id) {
            return Err(Error::NotFound);
        }
        self.report = None;
        self.last_report_ack = Some(id);
        Ok(())
    }
    /// Confirm the exact E81 transition,not merely its reused original alarm ID.
    /// # Errors
    /// Unknown/mismatched ID/transition or premature cancel event fails.
    pub fn confirm_alarm(&mut self, id: u32, transition: AlarmTransition) -> Result<(), Error> {
        if self.last_alarm_ack == Some((id, transition)) {
            return Ok(());
        }
        let pending = self.alarm.ok_or(Error::NotFound)?;
        if pending.event.event_id != id || pending.event.transition != transition {
            return Err(Error::NotFound);
        }
        if pending.admission != Admission::Ready {
            return Err(Error::InvalidState);
        }
        self.alarm = None;
        self.last_alarm_ack = Some((id, transition));
        Ok(())
    }
    /// Apply a current-cycle cancel exactly once. True means newly silenced,false known cancelled.
    /// The C30 successful response body is empty;its admission must precede the resulting E81.
    /// # Errors
    /// Unknown ID,invalid origin,full retention or occupied event slot fails without fake success.
    pub fn request_cancel(
        &mut self,
        id: u32,
        source: UvSource,
        when: Timestamp,
    ) -> Result<bool, Error> {
        if id == 0 || source == UvSource::DeviceSampling {
            return Err(Error::InvalidField);
        }
        if self.cancelled.contains(&Some(id)) {
            return Ok(false);
        }
        let mut active = self
            .active
            .filter(|alarm| alarm.id == id)
            .ok_or(Error::NotFound)?;
        if self.alarm.is_some() {
            return Err(Error::Busy);
        }
        if active.cancelled {
            return Err(Error::InvalidState);
        }
        let slot = self
            .cancelled
            .iter()
            .position(Option::is_none)
            .ok_or(Error::Capacity)?;
        let latest = self.latest.ok_or(Error::NotFound)?;
        let transition = match source {
            UvSource::DeviceUser => AlarmTransition::DeviceCancel,
            UvSource::AppRequest => AlarmTransition::AppCancel,
            UvSource::DeviceSampling => return Err(Error::InvalidField),
        };
        self.cancelled[slot] = Some(id);
        active.cancelled = true;
        self.active = Some(active);
        self.alarm = Some(PendingAlarm {
            event: UvAlarmEvent {
                event_id: id,
                source,
                transition,
                timestamp: when,
                reading: latest.reading,
            },
            admission: Admission::AwaitCancelResponse,
        });
        Ok(true)
    }
    /// Confirm actual C30 response admission;old duplicate confirmation never rearms another event.
    /// # Errors
    /// Unknown cancellation ID fails.
    pub fn confirm_cancel_response(&mut self, id: u32) -> Result<(), Error> {
        if !self.cancelled.contains(&Some(id)) {
            return Err(Error::NotFound);
        }
        if let Some(pending) = self.alarm.as_mut()
            && pending.event.event_id == id
            && matches!(
                pending.event.transition,
                AlarmTransition::DeviceCancel | AlarmTransition::AppCancel
            )
        {
            pending.admission = Admission::Ready;
        }
        Ok(())
    }
    /// Poll only actual selected backend data;None remains absence and unavailable is an error.
    /// # Errors
    /// Missing/unversioned provider or actual backend/sequence/capacity failure is explicit.
    pub fn poll<B: UvBackend>(&mut self, backend: &mut B) -> Result<bool, Error> {
        if !backend.available() {
            return Err(Error::Unavailable);
        }
        let metadata = backend.provenance();
        metadata.validate()?;
        let Some(sample) = backend.peek()? else {
            return Ok(false);
        };
        self.ingest(sample)
    }
    /// Release the backend's actual retained sample only after all generated events are admitted.
    /// This acknowledges producer buffer admission,not durable storage or delivery to a phone.
    /// # Errors
    /// Pending events,unknown sample or actual backend fault prevents acknowledgement.
    pub fn confirm_sample<B: UvBackend>(
        &mut self,
        backend: &mut B,
        sample_id: u64,
    ) -> Result<(), Error> {
        if self.last_sample_ack == Some(sample_id) {
            return Ok(());
        }
        if self
            .latest
            .is_none_or(|sample| sample.sample_id != sample_id)
        {
            return Err(Error::NotFound);
        }
        if self.report.is_some() || self.alarm.is_some() {
            return Err(Error::Busy);
        }
        backend.acknowledge(sample_id)?;
        self.last_sample_ack = Some(sample_id);
        Ok(())
    }
}
fn allocate(next: &mut Option<u32>) -> Result<u32, Error> {
    let id = next.ok_or(Error::Exhausted)?;
    *next = id.checked_add(1);
    Ok(id)
}
/// Actual value acquisition class,separate from the event's device/user/App trigger source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UvValueOrigin {
    /// Product-declared actual sensor path,subject to physical validation.
    Sensor,
    /// Explicit estimate,not promoted automatically to sensor measurement.
    DerivedEstimate,
    /// Explicit authorized external import,not a device sensor measurement.
    PhoneImport,
}
/// Trusted product/provider metadata,not extra wire bytes or a peer assertion of sensor truth.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct UvProvenance<'a> {
    /// Actual acquisition class.
    pub origin: UvValueOrigin,
    /// Actual selected provider ID.
    pub provider_id: &'a str,
    /// Actual selected provider version.
    pub provider_version: &'a str,
}
impl core::fmt::Debug for UvProvenance<'_> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("UvProvenance")
            .field("provider_id_length", &self.provider_id.len())
            .field("provider_version_length", &self.provider_version.len())
            .finish()
    }
}
impl UvProvenance<'_> {
    /// Validate explicit actual metadata,not physical sensor accuracy.
    /// # Errors
    /// Missing/NUL-containing actual provider identity/version fails.
    pub fn validate(self) -> Result<(), Error> {
        if self.provider_id.is_empty()
            || self.provider_version.is_empty()
            || self.provider_id.as_bytes().contains(&0)
            || self.provider_version.as_bytes().contains(&0)
        {
            Err(Error::InvalidField)
        } else {
            Ok(())
        }
    }
}
/// Actual UV acquisition/import port. Immutable peek data remains retained until acknowledgement.
/// This crate installs no real or simulated production implementation. Product authorization,actual
/// source declaration,units/calibration and sensor availability are external trust responsibilities.
pub trait UvBackend {
    /// Actual selected capability availability.
    fn available(&self) -> bool;
    /// Actual product/provider metadata,distinct from wire trigger source.
    fn provenance(&self) -> UvProvenance<'_>;
    /// Actual retained data,or explicit no sample. Never fabricate successful zeros.
    /// # Errors
    /// Actual backend faults/unavailability remain explicit.
    fn peek(&mut self) -> Result<Option<UvSample>, crate::BackendError>;
    /// Release actual data only after required downstream admission.
    /// # Errors
    /// On error retain the same actual sample for retry.
    fn acknowledge(&mut self, sample_id: u64) -> Result<(), crate::BackendError>;
}
